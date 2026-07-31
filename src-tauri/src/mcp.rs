//! Local MCP server: agents reach opted-in connections through the same
//! command layer as the UI; secrets never leave the core.

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
  StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;

use crate::connectors::TableRowsRequest;
use crate::error::Error;
use crate::profiles::{AgentAccess, ConnectionProfile, ConnectorKind};
use crate::{commands, AppState};

pub const DEFAULT_PORT: u16 = 52700;
const TOKEN_SECRET_ID: &str = "soquel-mcp-token";
/// Agents get capped result sets; the UI streams, agents paginate.
const MAX_AGENT_ROWS: usize = 500;

pub struct McpRunning {
  pub port: u16,
  pub cancel: CancellationToken,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
  pub running: bool,
  pub port: u16,
  pub endpoint: String,
  pub token: String,
}

fn new_token() -> String {
  format!(
    "{}{}",
    uuid::Uuid::new_v4().simple(),
    uuid::Uuid::new_v4().simple()
  )
}

pub fn ensure_token(secrets: &dyn crate::secrets::SecretStore) -> Result<String, Error> {
  if let Some(token) = secrets.get(TOKEN_SECRET_ID)? {
    return Ok(token);
  }
  let token = new_token();
  secrets.set(TOKEN_SECRET_ID, &token)?;
  Ok(token)
}

/// Only while stopped: the running middleware holds a snapshot of the token.
pub async fn regenerate_token(state: &AppState) -> Result<String, Error> {
  if state.mcp.lock().await.is_some() {
    return Err(Error::Unsupported {
      message: "stop the MCP server before regenerating its token".to_string(),
    });
  }
  let token = new_token();
  state.secrets.set(TOKEN_SECRET_ID, &token)?;
  Ok(token)
}

pub async fn status(state: &AppState) -> Result<McpStatus, Error> {
  let running = state.mcp.lock().await;
  let port = running.as_ref().map_or(DEFAULT_PORT, |r| r.port);
  Ok(McpStatus {
    running: running.is_some(),
    port,
    endpoint: format!("http://127.0.0.1:{port}/mcp"),
    token: ensure_token(state.secrets.as_ref())?,
  })
}

pub async fn start(app: AppHandle, port: u16) -> Result<(), Error> {
  let state = app.state::<AppState>();
  if state.mcp.lock().await.is_some() {
    return Err(Error::Unsupported {
      message: "MCP server already running".to_string(),
    });
  }
  let token = ensure_token(state.secrets.as_ref())?;
  // std bind: reports "port in use" synchronously and defers reactor
  // registration to the runtime that actually serves.
  let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
  listener.set_nonblocking(true)?;
  let cancel = CancellationToken::new();

  let handler_app = app.clone();
  let service = StreamableHttpService::new(
    move || Ok(SoquelMcp::new(handler_app.clone())),
    LocalSessionManager::default().into(),
    StreamableHttpServerConfig::default().with_cancellation_token(cancel.clone()),
  );
  let expected = Arc::new(format!("Bearer {token}"));
  let router = axum::Router::new()
    .nest_service("/mcp", service)
    .layer(axum::middleware::from_fn(
      move |req: axum::extract::Request, next: axum::middleware::Next| {
        let expected = expected.clone();
        async move { require_bearer(expected, req, next).await }
      },
    ));

  let serve_cancel = cancel.clone();
  tauri::async_runtime::spawn(async move {
    let served = async {
      let listener = tokio::net::TcpListener::from_std(listener)?;
      axum::serve(listener, router)
        .with_graceful_shutdown(async move { serve_cancel.cancelled().await })
        .await
    }
    .await;
    if let Err(err) = served {
      log::error!("mcp server stopped: {err}");
    }
  });
  *state.mcp.lock().await = Some(McpRunning { port, cancel });
  Ok(())
}

pub async fn stop(state: &AppState) -> Result<(), Error> {
  if let Some(running) = state.mcp.lock().await.take() {
    running.cancel.cancel();
  }
  Ok(())
}

async fn require_bearer(
  expected: Arc<String>,
  req: axum::extract::Request,
  next: axum::middleware::Next,
) -> axum::response::Response {
  use axum::response::IntoResponse;
  let authorized = req
    .headers()
    .get(axum::http::header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .is_some_and(|value| value == expected.as_str());
  if authorized {
    next.run(req).await
  } else {
    axum::http::StatusCode::UNAUTHORIZED.into_response()
  }
}

#[derive(Clone)]
pub struct SoquelMcp {
  app: AppHandle,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ConnectionArgs {
  /// Connection id from list_connections.
  connection_id: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct QueryArgs {
  /// Connection id from list_connections.
  connection_id: String,
  /// One SQL statement; executed with engine-enforced read-only semantics.
  sql: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct TableArgs {
  /// Connection id from list_connections.
  connection_id: String,
  schema: String,
  table: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct SampleArgs {
  /// Connection id from list_connections.
  connection_id: String,
  schema: String,
  table: String,
  /// Rows to return (default 100, capped at 500).
  limit: Option<u32>,
  offset: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentConnection {
  id: String,
  name: String,
  kind: ConnectorKind,
  access: AgentAccess,
  connected: bool,
  server_version: Option<String>,
}

/// Opted-in profiles only: everything else does not exist for agents.
pub fn agent_visible(profiles: Vec<ConnectionProfile>) -> Vec<ConnectionProfile> {
  profiles
    .into_iter()
    .filter(|profile| profile.agent_access != AgentAccess::None)
    .collect()
}

/// Cap statement rows for agent consumption; flag when anything was dropped.
pub fn capped(mut result: crate::connectors::QueryResult) -> serde_json::Value {
  let mut truncated = false;
  for statement in &mut result.statements {
    if statement.rows.len() > MAX_AGENT_ROWS {
      statement.rows.truncate(MAX_AGENT_ROWS);
      truncated = true;
    }
  }
  serde_json::json!({ "truncated": truncated, "result": result })
}

fn respond(outcome: Result<serde_json::Value, Error>) -> Result<CallToolResult, McpError> {
  match outcome {
    Ok(value) => Ok(CallToolResult::success(vec![ContentBlock::text(
      value.to_string(),
    )])),
    Err(err) => Err(McpError::internal_error(
      err.to_string(),
      serde_json::to_value(&err).ok(),
    )),
  }
}

fn opted_in(state: &AppState, id: &str) -> Result<ConnectionProfile, Error> {
  let profile = state.profiles.lock().unwrap().get(id)?;
  if profile.agent_access == AgentAccess::None {
    // Indistinguishable from a missing connection on purpose.
    return Err(Error::NotFound {
      message: format!("connection {id} not found"),
    });
  }
  Ok(profile)
}

async fn ensure_connected(state: &AppState, id: &str) -> Result<(), Error> {
  if state.connections.lock().await.contains_key(id) {
    return Ok(());
  }
  commands::connect_impl(state, id.to_string()).await
}

fn audit(
  state: &AppState,
  tool: &str,
  connection: Option<&str>,
  detail: Option<&str>,
  outcome: &Result<serde_json::Value, Error>,
  started: Instant,
) {
  let entry = serde_json::json!({
    "ts": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
    "tool": tool,
    "connection": connection,
    "detail": detail,
    "ok": outcome.is_ok(),
    "error": outcome.as_ref().err().map(|err| err.to_string()),
    "durationMs": started.elapsed().as_secs_f64() * 1000.0,
  });
  let path = state.data_dir.join("mcp-audit.jsonl");
  let written = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(&path)
    .and_then(|mut file| writeln!(file, "{entry}"));
  if let Err(err) = written {
    log::warn!("mcp audit append failed: {err}");
  }
}

async fn list_connections_impl(state: &AppState) -> Result<serde_json::Value, Error> {
  let profiles = agent_visible(state.profiles.lock().unwrap().list());
  let versions: HashMap<String, Option<String>> = {
    let connections = state.connections.lock().await;
    connections
      .iter()
      .map(|(id, active)| (id.clone(), active.connection.server_version()))
      .collect()
  };
  let list: Vec<AgentConnection> = profiles
    .into_iter()
    .map(|profile| AgentConnection {
      connected: versions.contains_key(&profile.id),
      server_version: versions.get(&profile.id).cloned().flatten(),
      kind: profile.params.kind(),
      access: profile.agent_access,
      id: profile.id,
      name: profile.name,
    })
    .collect();
  Ok(serde_json::to_value(list)?)
}

async fn get_schema_impl(
  state: &AppState,
  args: &ConnectionArgs,
) -> Result<serde_json::Value, Error> {
  opted_in(state, &args.connection_id)?;
  ensure_connected(state, &args.connection_id).await?;
  let connection = commands::active(state, &args.connection_id).await?;
  let introspect = connection.introspect().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support schema introspection".to_string(),
  })?;
  Ok(serde_json::to_value(introspect.schema_snapshot().await?)?)
}

async fn get_table_ddl_impl(
  state: &AppState,
  args: &TableArgs,
) -> Result<serde_json::Value, Error> {
  opted_in(state, &args.connection_id)?;
  ensure_connected(state, &args.connection_id).await?;
  let connection = commands::active(state, &args.connection_id).await?;
  let introspect = connection.introspect().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support schema introspection".to_string(),
  })?;
  let ddl = introspect.table_ddl(&args.schema, &args.table).await?;
  Ok(serde_json::Value::String(ddl))
}

async fn run_query_impl(state: &AppState, args: &QueryArgs) -> Result<serde_json::Value, Error> {
  opted_in(state, &args.connection_id)?;
  ensure_connected(state, &args.connection_id).await?;
  let connection = commands::active(state, &args.connection_id).await?;
  let sql = connection.sql().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support SQL".to_string(),
  })?;
  Ok(capped(sql.run_read_only_query(&args.sql).await?))
}

async fn sample_rows_impl(state: &AppState, args: &SampleArgs) -> Result<serde_json::Value, Error> {
  opted_in(state, &args.connection_id)?;
  ensure_connected(state, &args.connection_id).await?;
  let connection = commands::active(state, &args.connection_id).await?;
  let sql = connection.sql().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support table browsing".to_string(),
  })?;
  let request = TableRowsRequest {
    schema: args.schema.clone(),
    table: args.table.clone(),
    limit: Some(args.limit.unwrap_or(100).min(MAX_AGENT_ROWS as u32)),
    offset: args.offset.unwrap_or(0),
    sort: None,
    filters: Vec::new(),
    include_ctid: false,
    include_xmin: false,
  };
  Ok(capped(sql.table_rows(&request).await?))
}

#[tool_router]
impl SoquelMcp {
  pub fn new(app: AppHandle) -> Self {
    Self { app }
  }

  fn state(&self) -> State<'_, AppState> {
    self.app.state::<AppState>()
  }

  #[tool(
    description = "List the database connections exposed to agents (opt-in per connection in the Soquel UI). Returns id, kind, access level and connected state."
  )]
  async fn list_connections(&self) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = list_connections_impl(&state).await;
    audit(&state, "list_connections", None, None, &outcome, started);
    respond(outcome)
  }

  #[tool(
    description = "Schema snapshot of a connection: schemas, tables, columns, primary keys, foreign keys and indexes."
  )]
  async fn get_schema(
    &self,
    Parameters(args): Parameters<ConnectionArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = get_schema_impl(&state, &args).await;
    audit(
      &state,
      "get_schema",
      Some(&args.connection_id),
      None,
      &outcome,
      started,
    );
    respond(outcome)
  }

  #[tool(description = "DDL of one table (CREATE TABLE and related statements).")]
  async fn get_table_ddl(
    &self,
    Parameters(args): Parameters<TableArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = get_table_ddl_impl(&state, &args).await;
    audit(
      &state,
      "get_table_ddl",
      Some(&args.connection_id),
      Some(&format!("{}.{}", args.schema, args.table)),
      &outcome,
      started,
    );
    respond(outcome)
  }

  #[tool(
    description = "Run one read-only SQL statement on a connection. Read-only is enforced by the engine; results are capped, paginate with LIMIT/OFFSET."
  )]
  async fn run_query(
    &self,
    Parameters(args): Parameters<QueryArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = run_query_impl(&state, &args).await;
    audit(
      &state,
      "run_query",
      Some(&args.connection_id),
      Some(&args.sql),
      &outcome,
      started,
    );
    respond(outcome)
  }

  #[tool(description = "Sample rows from a table without writing SQL. Paginated.")]
  async fn sample_rows(
    &self,
    Parameters(args): Parameters<SampleArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = sample_rows_impl(&state, &args).await;
    audit(
      &state,
      "sample_rows",
      Some(&args.connection_id),
      Some(&format!("{}.{}", args.schema, args.table)),
      &outcome,
      started,
    );
    respond(outcome)
  }
}

#[tool_handler]
impl ServerHandler for SoquelMcp {
  fn get_info(&self) -> ServerInfo {
    let mut info = ServerInfo::default();
    info.instructions = Some(
      "Soquel exposes the user's database connections to agents. Connections are opted in \
       per profile from the app UI and queries run read-only with engine-level enforcement. \
       Start with list_connections."
        .to_string(),
    );
    info.capabilities = ServerCapabilities::builder().enable_tools().build();
    info
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::connectors::{QueryResult, StatementResult};
  use crate::profiles::{ConnectorParams, Env};
  use crate::secrets::InMemoryStore;

  #[test]
  fn ensure_token_is_stable() {
    let secrets = InMemoryStore::default();
    let first = ensure_token(&secrets).unwrap();
    assert_eq!(first.len(), 64);
    assert_eq!(ensure_token(&secrets).unwrap(), first);
  }

  fn profile(name: &str, access: AgentAccess) -> ConnectionProfile {
    ConnectionProfile {
      id: name.to_string(),
      name: name.to_string(),
      env: Env::Dev,
      group: None,
      agent_access: access,
      params: ConnectorParams::Sqlite {
        path: "app.db".to_string(),
      },
    }
  }

  #[test]
  fn agent_visible_hides_non_opted_profiles() {
    let visible = agent_visible(vec![
      profile("hidden", AgentAccess::None),
      profile("read", AgentAccess::ReadOnly),
      profile("write", AgentAccess::WriteWithApproval),
    ]);
    let names: Vec<&str> = visible.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["read", "write"]);
  }

  #[tokio::test]
  async fn bearer_middleware_gates_requests() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let expected = Arc::new("Bearer sesame".to_string());
    let router = axum::Router::new()
      .route("/mcp", axum::routing::get(|| async { "ok" }))
      .layer(axum::middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
          let expected = expected.clone();
          async move { require_bearer(expected, req, next).await }
        },
      ));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
      .await
      .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let request = |auth: Option<&'static str>| async move {
      let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
      let header = auth
        .map(|a| format!("Authorization: {a}\r\n"))
        .unwrap_or_default();
      let raw =
        format!("GET /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\n{header}Connection: close\r\n\r\n");
      stream.write_all(raw.as_bytes()).await.unwrap();
      let mut response = String::new();
      stream.read_to_string(&mut response).await.unwrap();
      response.lines().next().unwrap().to_string()
    };

    assert!(request(None).await.contains("401"));
    assert!(request(Some("Bearer wrong")).await.contains("401"));
    assert!(request(Some("Bearer sesame")).await.contains("200"));
  }

  #[test]
  fn capped_truncates_rows() {
    let result = QueryResult {
      statements: vec![StatementResult {
        columns: Vec::new(),
        rows: vec![vec![None]; 501],
        rows_affected: 501.0,
      }],
      notices: Vec::new(),
      duration_ms: 1.0,
    };
    let value = capped(result);
    assert_eq!(value["truncated"], true);
    let rows = value["result"]["statements"][0]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 500);
  }

  use crate::known_hosts::KnownHostsStore;
  use crate::profiles::{ConnectionInput, ProfileStore, SqlServerParams, SslMode};
  use crate::secrets::SecretStore;
  use crate::tunnels::TunnelStore;

  fn pg_params(url: &str) -> ConnectorParams {
    let config: tokio_postgres::Config = url.parse().unwrap();
    let tokio_postgres::config::Host::Tcp(host) = &config.get_hosts()[0] else {
      panic!("expected a tcp host");
    };
    ConnectorParams::Postgres(SqlServerParams {
      host: host.clone(),
      port: config.get_ports()[0],
      database: config.get_dbname().unwrap().to_string(),
      user: config.get_user().unwrap().to_string(),
      ssl_mode: SslMode::Prefer,
      ssl_root_cert: None,
      tunnel_id: None,
    })
  }

  /// Two profiles against the compose postgres: one opted in, one hidden.
  fn test_state(dir: &tempfile::TempDir, url: &str) -> (AppState, String, String) {
    let mut profiles = ProfileStore::load(dir.path().join("connections.json")).unwrap();
    let opted = profiles
      .create(&ConnectionInput {
        name: "agent-visible".to_string(),
        env: Env::Dev,
        group: None,
        agent_access: AgentAccess::ReadOnly,
        params: pg_params(url),
        password: None,
      })
      .unwrap();
    let hidden = profiles
      .create(&ConnectionInput {
        name: "hidden".to_string(),
        env: Env::Dev,
        group: None,
        agent_access: AgentAccess::None,
        params: pg_params(url),
        password: None,
      })
      .unwrap();
    let secrets = InMemoryStore::default();
    secrets.set(&opted.id, "soquel").unwrap();
    secrets.set(&hidden.id, "soquel").unwrap();
    let state = AppState {
      profiles: std::sync::Mutex::new(profiles),
      tunnels: std::sync::Mutex::new(TunnelStore::load(dir.path().join("tunnels.json")).unwrap()),
      known_hosts: std::sync::Mutex::new(
        KnownHostsStore::load(dir.path().join("known_hosts.json")).unwrap(),
      ),
      secrets: Box::new(secrets),
      connections: tokio::sync::Mutex::new(HashMap::new()),
      sessions: tokio::sync::Mutex::new(HashMap::new()),
      data_dir: dir.path().to_path_buf(),
      mcp: tokio::sync::Mutex::new(None),
    };
    (state, opted.id, hidden.id)
  }

  fn assert_hidden(outcome: Result<serde_json::Value, Error>, tool: &str) {
    let Err(Error::NotFound { message }) = outcome else {
      panic!("{tool} must not reach a non-opted-in profile");
    };
    assert!(message.contains("not found"), "{message}");
  }

  #[tokio::test]
  async fn integration_mcp_opt_in_gates_every_tool() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG") else {
      return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (state, opted, hidden) = test_state(&dir, &url);

    let list = list_connections_impl(&state).await.unwrap();
    let names: Vec<&str> = list
      .as_array()
      .unwrap()
      .iter()
      .map(|c| c["name"].as_str().unwrap())
      .collect();
    assert_eq!(names, ["agent-visible"]);

    assert_hidden(
      run_query_impl(
        &state,
        &QueryArgs {
          connection_id: hidden.clone(),
          sql: "SELECT 1".to_string(),
        },
      )
      .await,
      "run_query",
    );
    assert_hidden(
      get_schema_impl(
        &state,
        &ConnectionArgs {
          connection_id: hidden.clone(),
        },
      )
      .await,
      "get_schema",
    );
    assert_hidden(
      get_table_ddl_impl(
        &state,
        &TableArgs {
          connection_id: hidden.clone(),
          schema: "app".to_string(),
          table: "customers".to_string(),
        },
      )
      .await,
      "get_table_ddl",
    );
    assert_hidden(
      sample_rows_impl(
        &state,
        &SampleArgs {
          connection_id: hidden.clone(),
          schema: "app".to_string(),
          table: "customers".to_string(),
          limit: None,
          offset: None,
        },
      )
      .await,
      "sample_rows",
    );
    // Gating happens before any connection attempt.
    assert!(state.connections.lock().await.is_empty());

    // Same code path lets the opted-in profile through (auto-connect included).
    let value = run_query_impl(
      &state,
      &QueryArgs {
        connection_id: opted.clone(),
        sql: "SELECT 1 AS one".to_string(),
      },
    )
    .await
    .unwrap();
    assert_eq!(value["result"]["statements"][0]["rows"][0][0], "1");
    assert!(state.connections.lock().await.contains_key(&opted));
  }

  #[tokio::test]
  async fn integration_mcp_tools_read_only_capped_audited() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG") else {
      return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (state, opted, _hidden) = test_state(&dir, &url);

    let err = run_query_impl(
      &state,
      &QueryArgs {
        connection_id: opted.clone(),
        sql: "UPDATE app.customers SET name = name".to_string(),
      },
    )
    .await
    .unwrap_err();
    let Error::Database { message } = err else {
      panic!("expected a database error");
    };
    assert!(message.contains("read-only"), "{message}");

    let value = run_query_impl(
      &state,
      &QueryArgs {
        connection_id: opted.clone(),
        sql: "SELECT generate_series(1, 600)".to_string(),
      },
    )
    .await
    .unwrap();
    assert_eq!(value["truncated"], true);
    let rows = value["result"]["statements"][0]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 500);

    let schema = get_schema_impl(
      &state,
      &ConnectionArgs {
        connection_id: opted.clone(),
      },
    )
    .await
    .unwrap();
    assert!(
      schema["schemas"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == "app"),
      "{schema}"
    );

    let ddl = get_table_ddl_impl(
      &state,
      &TableArgs {
        connection_id: opted.clone(),
        schema: "app".to_string(),
        table: "customers".to_string(),
      },
    )
    .await
    .unwrap();
    assert!(ddl.as_str().unwrap().contains("CREATE TABLE"), "{ddl}");

    let sample = sample_rows_impl(
      &state,
      &SampleArgs {
        connection_id: opted.clone(),
        schema: "app".to_string(),
        table: "customers".to_string(),
        limit: Some(3),
        offset: None,
      },
    )
    .await
    .unwrap();
    let rows = sample["result"]["statements"][0]["rows"]
      .as_array()
      .unwrap();
    assert_eq!(rows.len(), 3);

    audit(
      &state,
      "run_query",
      Some(&opted),
      Some("SELECT 1"),
      &Ok(serde_json::Value::Null),
      Instant::now(),
    );
    let raw = std::fs::read_to_string(state.data_dir.join("mcp-audit.jsonl")).unwrap();
    let entry: serde_json::Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
    assert_eq!(entry["tool"], "run_query");
    assert_eq!(entry["ok"], true);
    assert_eq!(entry["connection"].as_str().unwrap(), opted);
  }
}
