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
use tauri_specta::Event as _;
use tokio_util::sync::CancellationToken;

use crate::connectors::TableRowsRequest;
use crate::error::Error;
use crate::profiles::{AgentAccess, ConnectionProfile, ConnectorKind};
use crate::secrets::SecretKey;
use crate::{commands, AppState};

// Debug builds get their own port and agent-facing name, like the data dir
// and keychain scope: dev and an installed release can run side by side.
pub const DEFAULT_PORT: u16 = if cfg!(debug_assertions) { 52701 } else { 52700 };
/// A write nobody answers is a write nobody wanted.
const APPROVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const SERVER_NAME: &str = if cfg!(debug_assertions) {
  "soquel-dev"
} else {
  "soquel"
};
/// Agents get capped result sets; the UI streams, agents paginate.
const MAX_AGENT_ROWS: usize = 500;

pub struct McpRunning {
  pub port: u16,
  pub cancel: CancellationToken,
}

/// The toggle survives restarts: an enabled server comes back on launch.
#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpSettings {
  enabled: bool,
  port: u16,
}

impl Default for McpSettings {
  fn default() -> Self {
    Self {
      enabled: false,
      port: DEFAULT_PORT,
    }
  }
}

fn load_settings(state: &AppState) -> McpSettings {
  std::fs::read_to_string(state.data_dir.join("mcp.json"))
    .ok()
    .and_then(|raw| serde_json::from_str(&raw).ok())
    .unwrap_or_default()
}

fn save_settings(state: &AppState, settings: McpSettings) {
  let path = state.data_dir.join("mcp.json");
  let written = serde_json::to_string_pretty(&settings)
    .map_err(std::io::Error::other)
    .and_then(|raw| std::fs::write(&path, raw));
  if let Err(err) = written {
    log::warn!("mcp settings write failed: {err}");
  }
}

pub fn autostart(app: &AppHandle) {
  let state = app.state::<AppState>();
  let settings = load_settings(state.inner());
  if !settings.enabled {
    return;
  }
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    if let Err(err) = start(app, settings.port).await {
      log::error!("mcp autostart failed: {err}");
    }
  });
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
  /// Milliseconds since the epoch; f64 because specta forbids u64 in bindings.
  pub ts: f64,
  pub tool: String,
  pub connection: Option<String>,
  pub detail: Option<String>,
  pub ok: bool,
  pub error: Option<String>,
  pub duration_ms: f64,
}

/// The MCP call stays blocked until this is answered.
#[derive(Debug, Clone, Serialize, serde::Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct McpApprovalRequest {
  pub id: String,
  pub connection_id: String,
  pub connection_name: String,
  pub sql: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
  pub running: bool,
  pub port: u16,
  pub endpoint: String,
  pub token: String,
  pub server_name: String,
}

fn new_token() -> String {
  format!(
    "{}{}",
    uuid::Uuid::new_v4().simple(),
    uuid::Uuid::new_v4().simple()
  )
}

pub fn ensure_token(secrets: &dyn crate::secrets::SecretStore) -> Result<String, Error> {
  if let Some(token) = secrets.get(&SecretKey::McpToken)? {
    return Ok(token);
  }
  let token = new_token();
  secrets.set(&SecretKey::McpToken, &token)?;
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
  state.secrets.set(&SecretKey::McpToken, &token)?;
  Ok(token)
}

pub async fn status(state: &AppState) -> Result<McpStatus, Error> {
  let running = state.mcp.lock().await;
  let port = running
    .as_ref()
    .map_or_else(|| load_settings(state).port, |r| r.port);
  Ok(McpStatus {
    running: running.is_some(),
    port,
    endpoint: format!("http://127.0.0.1:{port}/mcp"),
    token: ensure_token(state.secrets.as_ref())?,
    server_name: SERVER_NAME.to_string(),
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
  save_settings(
    state.inner(),
    McpSettings {
      enabled: true,
      port,
    },
  );
  Ok(())
}

pub async fn stop(state: &AppState) -> Result<(), Error> {
  if let Some(running) = state.mcp.lock().await.take() {
    running.cancel.cancel();
  }
  save_settings(
    state,
    McpSettings {
      enabled: false,
      ..load_settings(state)
    },
  );
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

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct KeyScanArgs {
  /// Connection id from list_connections.
  connection_id: String,
  /// Glob pattern, default "*".
  pattern: Option<String>,
  /// Continuation cursor from a previous page.
  cursor: Option<String>,
  /// Keys per page (default 100, capped at 500).
  count: Option<u32>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct KeyArgs {
  /// Connection id from list_connections.
  connection_id: String,
  key: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct DatabaseArgs {
  /// Connection id from list_connections.
  connection_id: String,
  database: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct CollectionArgs {
  /// Connection id from list_connections.
  connection_id: String,
  database: String,
  collection: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct DocFindArgs {
  /// Connection id from list_connections.
  connection_id: String,
  database: String,
  collection: String,
  /// Extended JSON filter object, e.g. {"status": "paid"}.
  filter: Option<String>,
  /// Extended JSON sort object, e.g. {"createdAt": -1}.
  sort: Option<String>,
  /// Documents per page (default 20, capped at 500).
  limit: Option<u32>,
  /// Continuation cursor from a previous page.
  cursor: Option<String>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct DocCountArgs {
  /// Connection id from list_connections.
  connection_id: String,
  database: String,
  collection: String,
  /// Extended JSON filter; omit for the fast collection estimate.
  filter: Option<String>,
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

/// Flags `truncated` so an agent knows it is looking at a partial result.
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
  commands::connect_impl(state, id.to_string()).await.map_err(
    // Nobody can answer a prompt, or vouch for a command, on this side.
    |err| match err {
      Error::SecretRequired { target_name, .. } => Error::Unsupported {
        message: format!(
          "{target_name} asks for its password at each connection: open it in soquel first, then retry"
        ),
      },
      Error::CommandApprovalRequired { target_name, .. } => Error::Unsupported {
        message: format!(
          "{target_name} gets its password from a command nobody approved yet: approve it in soquel first, then retry"
        ),
      },
      other => other,
    },
  )
}

fn audit(
  state: &AppState,
  tool: &str,
  connection: Option<&str>,
  detail: Option<&str>,
  outcome: &Result<serde_json::Value, Error>,
  started: Instant,
) {
  let entry = AuditEntry {
    ts: SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map_or(0.0, |since| since.as_millis() as f64),
    tool: tool.to_string(),
    connection: connection.map(str::to_string),
    detail: detail.map(str::to_string),
    ok: outcome.is_ok(),
    error: outcome.as_ref().err().map(|err| err.to_string()),
    duration_ms: started.elapsed().as_secs_f64() * 1000.0,
  };
  let path = state.data_dir.join("mcp-audit.jsonl");
  let written = serde_json::to_string(&entry)
    .map_err(std::io::Error::other)
    .and_then(|line| {
      std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| writeln!(file, "{line}"))
    });
  if let Err(err) = written {
    log::warn!("mcp audit append failed: {err}");
  }
}

/// Newest first; unparseable lines are skipped rather than failing the read.
pub fn audit_log(state: &AppState, limit: usize) -> Result<Vec<AuditEntry>, Error> {
  let raw = match std::fs::read_to_string(state.data_dir.join("mcp-audit.jsonl")) {
    Ok(raw) => raw,
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
    Err(err) => return Err(err.into()),
  };
  let mut entries: Vec<AuditEntry> = raw
    .lines()
    .filter_map(|line| serde_json::from_str(line).ok())
    .collect();
  entries.reverse();
  entries.truncate(limit);
  Ok(entries)
}

/// Answer a pending write request; unknown ids are stale (timed out) requests.
pub async fn resolve_approval(state: &AppState, id: &str, approved: bool) -> Result<(), Error> {
  let waiting = state.approvals.lock().await.remove(id);
  match waiting {
    Some(sender) => {
      let _ = sender.send(approved);
      Ok(())
    }
    None => Err(Error::NotFound {
      message: format!("approval request {id} is no longer pending"),
    }),
  }
}

/// How a pending write gets an answer; the app asks the user, tests decide.
#[async_trait::async_trait]
pub trait Approver: Send + Sync {
  async fn request(&self, state: &AppState, request: McpApprovalRequest) -> bool;
}

/// Emits to the webview and waits for the dialog; a silent UI denies by timeout.
pub struct DialogApprover {
  pub app: AppHandle,
}

#[async_trait::async_trait]
impl Approver for DialogApprover {
  async fn request(&self, state: &AppState, request: McpApprovalRequest) -> bool {
    let id = request.id.clone();
    let receiver = register_approval(state, &id).await;
    if request.emit(&self.app).is_err() {
      state.approvals.lock().await.remove(&id);
      return false;
    }
    await_approval(state, &id, receiver, APPROVAL_TIMEOUT).await
  }
}

async fn register_approval(state: &AppState, id: &str) -> tokio::sync::oneshot::Receiver<bool> {
  let (sender, receiver) = tokio::sync::oneshot::channel();
  state.approvals.lock().await.insert(id.to_string(), sender);
  receiver
}

/// Anything other than an explicit yes refuses: timeout, closed dialog, no answer.
async fn await_approval(
  state: &AppState,
  id: &str,
  receiver: tokio::sync::oneshot::Receiver<bool>,
  timeout: std::time::Duration,
) -> bool {
  let answer = tokio::time::timeout(timeout, receiver).await;
  state.approvals.lock().await.remove(id);
  matches!(answer, Ok(Ok(true)))
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
  let connection = agent_connection(state, &args.connection_id).await?;
  let introspect = connection.introspect().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support schema introspection".to_string(),
  })?;
  Ok(serde_json::to_value(introspect.schema_snapshot().await?)?)
}

async fn get_table_ddl_impl(
  state: &AppState,
  args: &TableArgs,
) -> Result<serde_json::Value, Error> {
  let connection = agent_connection(state, &args.connection_id).await?;
  let introspect = connection.introspect().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support schema introspection".to_string(),
  })?;
  let ddl = introspect.table_ddl(&args.schema, &args.table).await?;
  Ok(serde_json::Value::String(ddl))
}

async fn run_query_impl(
  state: &AppState,
  approver: &dyn Approver,
  args: &QueryArgs,
) -> Result<serde_json::Value, Error> {
  let profile = opted_in(state, &args.connection_id)?;
  let connection = agent_connection(state, &args.connection_id).await?;
  let sql = connection.sql().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support SQL".to_string(),
  })?;
  // Reads always take the engine-enforced read-only path: classification only
  // decides whether to ask, never what the engine allows.
  if crate::connectors::is_read_statement(&args.sql) {
    return Ok(capped(sql.run_read_only_query(&args.sql).await?));
  }
  if profile.agent_access != AgentAccess::WriteWithApproval {
    return Err(Error::Unsupported {
      message: "this connection is read-only for agents".to_string(),
    });
  }
  let request = McpApprovalRequest {
    id: uuid::Uuid::new_v4().to_string(),
    connection_id: profile.id.clone(),
    connection_name: profile.name.clone(),
    sql: args.sql.clone(),
  };
  if !approver.request(state, request).await {
    return Err(Error::Unsupported {
      message: "the write was not approved".to_string(),
    });
  }
  Ok(capped(sql.run_query(&args.sql).await?))
}

async fn sample_rows_impl(state: &AppState, args: &SampleArgs) -> Result<serde_json::Value, Error> {
  let connection = agent_connection(state, &args.connection_id).await?;
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

async fn agent_connection(
  state: &AppState,
  id: &str,
) -> Result<Arc<dyn crate::connectors::Connection>, Error> {
  opted_in(state, id)?;
  ensure_connected(state, id).await?;
  commands::active(state, id).await
}

async fn list_keys_impl(state: &AppState, args: &KeyScanArgs) -> Result<serde_json::Value, Error> {
  let connection = agent_connection(state, &args.connection_id).await?;
  let kv = connection.kv().ok_or_else(|| Error::Unsupported {
    message: "this connection is not a key-value store".to_string(),
  })?;
  let page = kv
    .scan_keys(
      args.pattern.as_deref().unwrap_or("*"),
      args.cursor.as_deref(),
      args.count.unwrap_or(100).min(MAX_AGENT_ROWS as u32),
    )
    .await?;
  Ok(serde_json::to_value(page)?)
}

async fn get_key_impl(state: &AppState, args: &KeyArgs) -> Result<serde_json::Value, Error> {
  let connection = agent_connection(state, &args.connection_id).await?;
  let kv = connection.kv().ok_or_else(|| Error::Unsupported {
    message: "this connection is not a key-value store".to_string(),
  })?;
  Ok(serde_json::to_value(kv.key_detail(&args.key).await?)?)
}

/// Redis reports a count, mongo a named list: one tool for either engine.
async fn list_databases_impl(
  state: &AppState,
  args: &ConnectionArgs,
) -> Result<serde_json::Value, Error> {
  let connection = agent_connection(state, &args.connection_id).await?;
  if let Some(doc) = connection.doc() {
    return Ok(serde_json::to_value(doc.databases().await?)?);
  }
  if let Some(kv) = connection.kv() {
    return Ok(serde_json::to_value(kv.databases().await?)?);
  }
  Err(Error::Unsupported {
    message: "this connection has no databases to list; use get_schema".to_string(),
  })
}

async fn list_collections_impl(
  state: &AppState,
  args: &DatabaseArgs,
) -> Result<serde_json::Value, Error> {
  let connection = agent_connection(state, &args.connection_id).await?;
  let doc = doc_surface(&connection)?;
  Ok(serde_json::to_value(
    doc.collections(&args.database).await?,
  )?)
}

async fn find_documents_impl(
  state: &AppState,
  args: &DocFindArgs,
) -> Result<serde_json::Value, Error> {
  let connection = agent_connection(state, &args.connection_id).await?;
  let doc = doc_surface(&connection)?;
  let request = crate::connectors::DocFindRequest {
    db: args.database.clone(),
    collection: args.collection.clone(),
    filter: args.filter.clone(),
    sort: args.sort.clone(),
    limit: args.limit.unwrap_or(20).min(MAX_AGENT_ROWS as u32),
    cursor: args.cursor.clone(),
  };
  Ok(serde_json::to_value(doc.find_docs(&request).await?)?)
}

async fn count_documents_impl(
  state: &AppState,
  args: &DocCountArgs,
) -> Result<serde_json::Value, Error> {
  let connection = agent_connection(state, &args.connection_id).await?;
  let doc = doc_surface(&connection)?;
  let count = doc
    .count_docs(&args.database, &args.collection, args.filter.as_deref())
    .await?;
  Ok(serde_json::to_value(count)?)
}

async fn list_indexes_impl(
  state: &AppState,
  args: &CollectionArgs,
) -> Result<serde_json::Value, Error> {
  let connection = agent_connection(state, &args.connection_id).await?;
  let doc = doc_surface(&connection)?;
  Ok(serde_json::to_value(
    doc.indexes(&args.database, &args.collection).await?,
  )?)
}

fn doc_surface(
  connection: &Arc<dyn crate::connectors::Connection>,
) -> Result<&dyn crate::connectors::DocBrowse, Error> {
  connection.doc().ok_or_else(|| Error::Unsupported {
    message: "this connection is not a document store".to_string(),
  })
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
    let approver = DialogApprover {
      app: self.app.clone(),
    };
    let outcome = run_query_impl(&state, &approver, &args).await;
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
  #[tool(
    description = "List the databases a key-value or document connection exposes. SQL connections use get_schema instead."
  )]
  async fn list_databases(
    &self,
    Parameters(args): Parameters<ConnectionArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = list_databases_impl(&state, &args).await;
    audit(
      &state,
      "list_databases",
      Some(&args.connection_id),
      None,
      &outcome,
      started,
    );
    respond(outcome)
  }

  #[tool(
    description = "Scan keys on a Redis connection, paginated. Operates on the database the connection is on; agents cannot switch database."
  )]
  async fn list_keys(
    &self,
    Parameters(args): Parameters<KeyScanArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = list_keys_impl(&state, &args).await;
    audit(
      &state,
      "list_keys",
      Some(&args.connection_id),
      args.pattern.as_deref(),
      &outcome,
      started,
    );
    respond(outcome)
  }

  #[tool(description = "Read one Redis key: its type, TTL and value.")]
  async fn get_key(
    &self,
    Parameters(args): Parameters<KeyArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = get_key_impl(&state, &args).await;
    audit(
      &state,
      "get_key",
      Some(&args.connection_id),
      Some(&args.key),
      &outcome,
      started,
    );
    respond(outcome)
  }

  #[tool(description = "List the collections of a MongoDB database.")]
  async fn list_collections(
    &self,
    Parameters(args): Parameters<DatabaseArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = list_collections_impl(&state, &args).await;
    audit(
      &state,
      "list_collections",
      Some(&args.connection_id),
      Some(&args.database),
      &outcome,
      started,
    );
    respond(outcome)
  }

  #[tool(
    description = "Find documents in a MongoDB collection with an optional extended-JSON filter and sort. Paginated."
  )]
  async fn find_documents(
    &self,
    Parameters(args): Parameters<DocFindArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = find_documents_impl(&state, &args).await;
    audit(
      &state,
      "find_documents",
      Some(&args.connection_id),
      Some(&format!(
        "{}.{} {}",
        args.database,
        args.collection,
        args.filter.as_deref().unwrap_or("{}")
      )),
      &outcome,
      started,
    );
    respond(outcome)
  }

  #[tool(description = "Count documents in a MongoDB collection, with or without a filter.")]
  async fn count_documents(
    &self,
    Parameters(args): Parameters<DocCountArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = count_documents_impl(&state, &args).await;
    audit(
      &state,
      "count_documents",
      Some(&args.connection_id),
      Some(&format!("{}.{}", args.database, args.collection)),
      &outcome,
      started,
    );
    respond(outcome)
  }

  #[tool(description = "List the indexes of a MongoDB collection.")]
  async fn list_indexes(
    &self,
    Parameters(args): Parameters<CollectionArgs>,
  ) -> Result<CallToolResult, McpError> {
    let started = Instant::now();
    let state = self.state();
    let outcome = list_indexes_impl(&state, &args).await;
    audit(
      &state,
      "list_indexes",
      Some(&args.connection_id),
      Some(&format!("{}.{}", args.database, args.collection)),
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
      credential: Default::default(),
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
        credential: Default::default(),
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
        credential: Default::default(),
        params: pg_params(url),
        password: None,
      })
      .unwrap();
    let secrets = InMemoryStore::default();
    secrets
      .set(&SecretKey::Connection(opted.id.clone()), "soquel")
      .unwrap();
    secrets
      .set(&SecretKey::Connection(hidden.id.clone()), "soquel")
      .unwrap();
    let state = AppState {
      profiles: std::sync::Mutex::new(profiles),
      tunnels: std::sync::Mutex::new(TunnelStore::load(dir.path().join("tunnels.json")).unwrap()),
      known_hosts: std::sync::Mutex::new(
        KnownHostsStore::load(dir.path().join("known_hosts.json")).unwrap(),
      ),
      command_approvals: std::sync::Mutex::new(
        crate::command_approvals::CommandApprovalsStore::load(
          dir.path().join("command_approvals.json"),
        )
        .unwrap(),
      ),
      secrets: Box::new(secrets),
      session_secrets: Default::default(),
      connections: tokio::sync::Mutex::new(HashMap::new()),
      sessions: tokio::sync::Mutex::new(HashMap::new()),
      data_dir: dir.path().to_path_buf(),
      mcp: tokio::sync::Mutex::new(None),
      approvals: tokio::sync::Mutex::new(HashMap::new()),
    };
    (state, opted.id, hidden.id)
  }

  /// Tripwire: adding a tool means visiting the gating test below, which proves
  /// the tool refuses a connection the user never opted in.
  #[test]
  fn the_agent_surface_is_exactly_these_tools() {
    let mut names: Vec<String> = SoquelMcp::tool_router()
      .list_all()
      .into_iter()
      .map(|tool| tool.name.to_string())
      .collect();
    names.sort();
    assert_eq!(
      names,
      [
        "count_documents",
        "find_documents",
        "get_key",
        "get_schema",
        "get_table_ddl",
        "list_collections",
        "list_connections",
        "list_databases",
        "list_indexes",
        "list_keys",
        "run_query",
        "sample_rows",
      ]
    );
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
        &FixedApprover(true),
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
    // The kv/doc tools gate on opt-in before they even check the engine kind:
    // this profile is postgres, and the refusal must still be "not found".
    assert_hidden(
      list_databases_impl(
        &state,
        &ConnectionArgs {
          connection_id: hidden.clone(),
        },
      )
      .await,
      "list_databases",
    );
    assert_hidden(
      list_keys_impl(
        &state,
        &KeyScanArgs {
          connection_id: hidden.clone(),
          pattern: None,
          cursor: None,
          count: None,
        },
      )
      .await,
      "list_keys",
    );
    assert_hidden(
      get_key_impl(
        &state,
        &KeyArgs {
          connection_id: hidden.clone(),
          key: "any".to_string(),
        },
      )
      .await,
      "get_key",
    );
    assert_hidden(
      list_collections_impl(
        &state,
        &DatabaseArgs {
          connection_id: hidden.clone(),
          database: "app".to_string(),
        },
      )
      .await,
      "list_collections",
    );
    assert_hidden(
      find_documents_impl(
        &state,
        &DocFindArgs {
          connection_id: hidden.clone(),
          database: "app".to_string(),
          collection: "customers".to_string(),
          filter: None,
          sort: None,
          limit: None,
          cursor: None,
        },
      )
      .await,
      "find_documents",
    );
    assert_hidden(
      count_documents_impl(
        &state,
        &DocCountArgs {
          connection_id: hidden.clone(),
          database: "app".to_string(),
          collection: "customers".to_string(),
          filter: None,
        },
      )
      .await,
      "count_documents",
    );
    assert_hidden(
      list_indexes_impl(
        &state,
        &CollectionArgs {
          connection_id: hidden.clone(),
          database: "app".to_string(),
          collection: "customers".to_string(),
        },
      )
      .await,
      "list_indexes",
    );
    // Gating happens before any connection attempt.
    assert!(state.connections.lock().await.is_empty());

    // Same code path lets the opted-in profile through (auto-connect included).
    let value = run_query_impl(
      &state,
      &FixedApprover(true),
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
      &FixedApprover(true),
      &QueryArgs {
        connection_id: opted.clone(),
        sql: "UPDATE app.customers SET name = name".to_string(),
      },
    )
    .await
    .unwrap_err();
    let Error::Unsupported { message } = err else {
      panic!("expected the agent guard to refuse: {err:?}");
    };
    assert!(message.contains("read-only for agents"), "{message}");

    // Defense in depth: a write the classifier reads as a read still dies in
    // the engine's read-only transaction, not on the guard.
    let leaked = run_query_impl(
      &state,
      &FixedApprover(true),
      &QueryArgs {
        connection_id: opted.clone(),
        sql: "WITH touched AS (UPDATE app.customers SET name = name RETURNING id) SELECT * FROM touched"
          .to_string(),
      },
    )
    .await
    .unwrap_err();
    let Error::Database { message } = leaked else {
      panic!("expected a database error: {leaked:?}");
    };
    assert!(message.contains("read-only"), "{message}");

    let value = run_query_impl(
      &state,
      &FixedApprover(true),
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

  fn bare_state(dir: &tempfile::TempDir) -> AppState {
    AppState {
      profiles: std::sync::Mutex::new(
        ProfileStore::load(dir.path().join("connections.json")).unwrap(),
      ),
      tunnels: std::sync::Mutex::new(TunnelStore::load(dir.path().join("tunnels.json")).unwrap()),
      known_hosts: std::sync::Mutex::new(
        KnownHostsStore::load(dir.path().join("known_hosts.json")).unwrap(),
      ),
      command_approvals: std::sync::Mutex::new(
        crate::command_approvals::CommandApprovalsStore::load(
          dir.path().join("command_approvals.json"),
        )
        .unwrap(),
      ),
      secrets: Box::new(InMemoryStore::default()),
      session_secrets: Default::default(),
      connections: tokio::sync::Mutex::new(HashMap::new()),
      sessions: tokio::sync::Mutex::new(HashMap::new()),
      data_dir: dir.path().to_path_buf(),
      mcp: tokio::sync::Mutex::new(None),
      approvals: tokio::sync::Mutex::new(HashMap::new()),
    }
  }

  #[tokio::test]
  async fn regenerate_token_requires_a_stopped_server() {
    let dir = tempfile::tempdir().unwrap();
    let state = bare_state(&dir);
    let first = ensure_token(state.secrets.as_ref()).unwrap();

    *state.mcp.lock().await = Some(McpRunning {
      port: 1,
      cancel: CancellationToken::new(),
    });
    let err = regenerate_token(&state).await.unwrap_err();
    assert!(matches!(err, Error::Unsupported { .. }), "{err:?}");
    assert_eq!(ensure_token(state.secrets.as_ref()).unwrap(), first);

    *state.mcp.lock().await = None;
    let fresh = regenerate_token(&state).await.unwrap();
    assert_ne!(fresh, first);
    assert_eq!(ensure_token(state.secrets.as_ref()).unwrap(), fresh);
  }

  #[test]
  fn settings_default_off_and_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let state = bare_state(&dir);
    let settings = load_settings(&state);
    assert!(!settings.enabled);
    assert_eq!(settings.port, DEFAULT_PORT);

    save_settings(
      &state,
      McpSettings {
        enabled: true,
        port: 4242,
      },
    );
    let loaded = load_settings(&state);
    assert!(loaded.enabled);
    assert_eq!(loaded.port, 4242);
  }

  /// Answers without a dialog: the decision under test, not the transport.
  struct FixedApprover(bool);

  #[async_trait::async_trait]
  impl Approver for FixedApprover {
    async fn request(&self, _state: &AppState, _request: McpApprovalRequest) -> bool {
      self.0
    }
  }

  struct DenyingApprover;

  #[async_trait::async_trait]
  impl Approver for DenyingApprover {
    async fn request(&self, _state: &AppState, _request: McpApprovalRequest) -> bool {
      false
    }
  }

  #[test]
  fn classifies_reads_generously_and_writes_as_writes() {
    for sql in [
      "SELECT 1",
      "  with x as (select 1) select * from x",
      "/* lead */ (SELECT 1)",
      "EXPLAIN SELECT 1",
      "SHOW TABLES",
    ] {
      assert!(crate::connectors::is_read_statement(sql), "{sql}");
    }
    for sql in [
      "INSERT INTO t VALUES (1)",
      "UPDATE t SET a = 1",
      "DELETE FROM t",
      "CREATE TABLE t (id int)",
      "DROP TABLE t",
      "TRUNCATE t",
      "",
    ] {
      assert!(!crate::connectors::is_read_statement(sql), "{sql}");
    }
  }

  #[tokio::test]
  async fn resolve_approval_answers_a_pending_request() {
    let dir = tempfile::tempdir().unwrap();
    let state = bare_state(&dir);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    state
      .approvals
      .lock()
      .await
      .insert("req-1".to_string(), sender);

    resolve_approval(&state, "req-1", true).await.unwrap();
    assert!(receiver.await.unwrap());
    // The slot is consumed: answering twice is a stale request.
    let err = resolve_approval(&state, "req-1", true).await.unwrap_err();
    assert!(matches!(err, Error::NotFound { .. }), "{err:?}");
  }

  #[tokio::test]
  async fn silence_denies_the_write() {
    let dir = tempfile::tempdir().unwrap();
    let state = bare_state(&dir);
    let receiver = register_approval(&state, "req-quiet").await;

    // Nobody answers: the default must be no, and the slot must not leak.
    let approved = await_approval(
      &state,
      "req-quiet",
      receiver,
      std::time::Duration::from_millis(30),
    )
    .await;
    assert!(!approved);
    assert!(state.approvals.lock().await.is_empty());
  }

  #[tokio::test]
  async fn a_closed_dialog_denies_the_write() {
    let dir = tempfile::tempdir().unwrap();
    let state = bare_state(&dir);
    let receiver = register_approval(&state, "req-gone").await;
    // Dropping the sender is what a vanished webview looks like.
    state.approvals.lock().await.remove("req-gone");

    assert!(
      !await_approval(
        &state,
        "req-gone",
        receiver,
        std::time::Duration::from_secs(5)
      )
      .await
    );
  }

  #[tokio::test]
  async fn concurrent_requests_resolve_independently() {
    let dir = tempfile::tempdir().unwrap();
    let state = bare_state(&dir);
    let first = register_approval(&state, "req-a").await;
    let second = register_approval(&state, "req-b").await;

    // Answering out of order must not cross the wires.
    resolve_approval(&state, "req-b", true).await.unwrap();
    resolve_approval(&state, "req-a", false).await.unwrap();

    let timeout = std::time::Duration::from_secs(5);
    assert!(!await_approval(&state, "req-a", first, timeout).await);
    assert!(await_approval(&state, "req-b", second, timeout).await);
    assert!(state.approvals.lock().await.is_empty());
  }

  /// Blocks until released, and records how many callers were inside at once.
  struct CountingApprover {
    inside: Arc<std::sync::atomic::AtomicUsize>,
    peak: Arc<std::sync::atomic::AtomicUsize>,
    release: Arc<tokio::sync::Notify>,
  }

  #[async_trait::async_trait]
  impl Approver for CountingApprover {
    async fn request(&self, _state: &AppState, _request: McpApprovalRequest) -> bool {
      use std::sync::atomic::Ordering;
      let now = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
      self.peak.fetch_max(now, Ordering::SeqCst);
      self.release.notified().await;
      self.inside.fetch_sub(1, Ordering::SeqCst);
      true
    }
  }

  #[tokio::test]
  async fn an_agent_cannot_answer_a_password_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let (state, id) = sqlite_state(&dir);
    let mut profile = state.profiles.lock().unwrap().get(&id).unwrap();
    profile.credential = crate::profiles::CredentialSource::Prompt;
    state
      .profiles
      .lock()
      .unwrap()
      .replace_all(vec![profile])
      .unwrap();

    let Err(Error::Unsupported { message }) = agent_connection(&state, &id).await.map(|_| ())
    else {
      panic!("the agent must get a plain refusal, not a prompt");
    };
    assert!(message.contains("open it in soquel first"), "{message}");
  }

  #[tokio::test]
  async fn an_agent_cannot_vouch_for_a_credential_command() {
    let dir = tempfile::tempdir().unwrap();
    let (state, id) = sqlite_state(&dir);
    let mut profile = state.profiles.lock().unwrap().get(&id).unwrap();
    profile.credential = crate::profiles::CredentialSource::Command {
      command: "curl evil.example.com".to_string(),
      refresh_after_secs: None,
    };
    state
      .profiles
      .lock()
      .unwrap()
      .replace_all(vec![profile])
      .unwrap();

    let Err(Error::Unsupported { message }) = agent_connection(&state, &id).await.map(|_| ())
    else {
      panic!("an unapproved command must not run for an agent");
    };
    assert!(message.contains("approve it in soquel first"), "{message}");
  }

  fn sqlite_state(dir: &tempfile::TempDir) -> (AppState, String) {
    let path = dir.path().join("agent.db");
    std::fs::write(&path, "").unwrap();
    let mut profiles = ProfileStore::load(dir.path().join("connections.json")).unwrap();
    let profile = profiles
      .create(&ConnectionInput {
        name: "agent sqlite".to_string(),
        env: Env::Dev,
        group: None,
        agent_access: AgentAccess::WriteWithApproval,
        credential: Default::default(),
        params: ConnectorParams::Sqlite {
          path: path.to_string_lossy().into_owned(),
        },
        password: None,
      })
      .unwrap();
    let state = AppState {
      profiles: std::sync::Mutex::new(profiles),
      tunnels: std::sync::Mutex::new(TunnelStore::load(dir.path().join("tunnels.json")).unwrap()),
      known_hosts: std::sync::Mutex::new(
        KnownHostsStore::load(dir.path().join("known_hosts.json")).unwrap(),
      ),
      command_approvals: std::sync::Mutex::new(
        crate::command_approvals::CommandApprovalsStore::load(
          dir.path().join("command_approvals.json"),
        )
        .unwrap(),
      ),
      secrets: Box::new(InMemoryStore::default()),
      session_secrets: Default::default(),
      connections: tokio::sync::Mutex::new(HashMap::new()),
      sessions: tokio::sync::Mutex::new(HashMap::new()),
      data_dir: dir.path().to_path_buf(),
      mcp: tokio::sync::Mutex::new(None),
      approvals: tokio::sync::Mutex::new(HashMap::new()),
    };
    (state, profile.id)
  }

  #[tokio::test(flavor = "multi_thread")]
  async fn two_writes_wait_on_approval_at_the_same_time() {
    use std::sync::atomic::Ordering;

    let dir = tempfile::tempdir().unwrap();
    let (state, id) = sqlite_state(&dir);
    let approver = CountingApprover {
      inside: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
      peak: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
      release: Arc::new(tokio::sync::Notify::new()),
    };
    let first_args = QueryArgs {
      connection_id: id.clone(),
      sql: "CREATE TABLE one (id integer)".to_string(),
    };
    let second_args = QueryArgs {
      connection_id: id.clone(),
      sql: "CREATE TABLE two (id integer)".to_string(),
    };

    // Nothing in the request path may hold a lock across the approval await.
    let both = tokio::join!(run_query_impl(&state, &approver, &first_args), async {
      while approver.inside.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
      }
      let second = run_query_impl(&state, &approver, &second_args);
      let releaser = async {
        while approver.inside.load(Ordering::SeqCst) < 2 {
          tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        approver.release.notify_waiters();
        approver.release.notify_waiters();
      };
      let (result, ()) = tokio::join!(second, releaser);
      result
    });
    assert_eq!(approver.peak.load(Ordering::SeqCst), 2);
    both.0.unwrap();
    both.1.unwrap();
  }

  #[tokio::test]
  async fn audit_log_reads_newest_first_and_survives_junk() {
    let dir = tempfile::tempdir().unwrap();
    let state = bare_state(&dir);
    assert!(audit_log(&state, 10).unwrap().is_empty());

    for tool in ["first", "second"] {
      audit(
        &state,
        tool,
        Some("conn"),
        Some("SELECT 1"),
        &Ok(serde_json::Value::Null),
        Instant::now(),
      );
    }
    std::fs::OpenOptions::new()
      .append(true)
      .open(dir.path().join("mcp-audit.jsonl"))
      .and_then(|mut file| writeln!(file, "not json"))
      .unwrap();
    audit(
      &state,
      "third",
      None,
      None,
      &Err(Error::Unsupported {
        message: "the write was not approved".to_string(),
      }),
      Instant::now(),
    );

    let entries = audit_log(&state, 10).unwrap();
    let tools: Vec<&str> = entries.iter().map(|entry| entry.tool.as_str()).collect();
    assert_eq!(tools, ["third", "second", "first"]);
    assert!(!entries[0].ok);
    assert_eq!(
      entries[0].error.as_deref(),
      Some("the write was not approved")
    );
    assert_eq!(audit_log(&state, 1).unwrap().len(), 1);
  }

  /// One opted-in profile against a non-SQL engine, keyed by its params.
  async fn kind_state(dir: &tempfile::TempDir, params: ConnectorParams) -> (AppState, String) {
    let mut profiles = ProfileStore::load(dir.path().join("connections.json")).unwrap();
    let profile = profiles
      .create(&ConnectionInput {
        name: "agent target".to_string(),
        env: Env::Dev,
        group: None,
        agent_access: AgentAccess::ReadOnly,
        credential: Default::default(),
        params,
        password: None,
      })
      .unwrap();
    let secrets = InMemoryStore::default();
    secrets
      .set(&SecretKey::Connection(profile.id.clone()), "soquel")
      .unwrap();
    let state = AppState {
      profiles: std::sync::Mutex::new(profiles),
      tunnels: std::sync::Mutex::new(TunnelStore::load(dir.path().join("tunnels.json")).unwrap()),
      known_hosts: std::sync::Mutex::new(
        KnownHostsStore::load(dir.path().join("known_hosts.json")).unwrap(),
      ),
      command_approvals: std::sync::Mutex::new(
        crate::command_approvals::CommandApprovalsStore::load(
          dir.path().join("command_approvals.json"),
        )
        .unwrap(),
      ),
      secrets: Box::new(secrets),
      session_secrets: Default::default(),
      connections: tokio::sync::Mutex::new(HashMap::new()),
      sessions: tokio::sync::Mutex::new(HashMap::new()),
      data_dir: dir.path().to_path_buf(),
      mcp: tokio::sync::Mutex::new(None),
      approvals: tokio::sync::Mutex::new(HashMap::new()),
    };
    (state, profile.id)
  }

  #[tokio::test]
  async fn integration_mcp_kv_tools_read_redis() {
    let Ok(addr) = std::env::var("SOQUEL_TEST_REDIS") else {
      return;
    };
    let (host, port) = addr.split_once(':').expect("host:port");
    let dir = tempfile::tempdir().unwrap();
    let (state, id) = kind_state(
      &dir,
      ConnectorParams::Redis(crate::profiles::RedisParams {
        host: host.to_string(),
        port: port.parse().unwrap(),
        db: 0,
        username: None,
        tls: false,
        tunnel_id: None,
      }),
    )
    .await;

    // Seed through the app's own surface, then read it back as an agent would.
    let connection = agent_connection(&state, &id).await.unwrap();
    connection
      .kv()
      .unwrap()
      .set_string("soquel_test:mcp:key", "hello")
      .await
      .unwrap();

    let databases = list_databases_impl(
      &state,
      &ConnectionArgs {
        connection_id: id.clone(),
      },
    )
    .await
    .unwrap();
    assert!(databases["total"].as_u64().unwrap() >= 1, "{databases}");

    let page = list_keys_impl(
      &state,
      &KeyScanArgs {
        connection_id: id.clone(),
        pattern: Some("soquel_test:mcp:*".to_string()),
        cursor: None,
        count: None,
      },
    )
    .await
    .unwrap();
    let names: Vec<&str> = page["keys"]
      .as_array()
      .unwrap()
      .iter()
      .map(|key| key["key"].as_str().unwrap())
      .collect();
    assert!(names.contains(&"soquel_test:mcp:key"), "{page}");

    let detail = get_key_impl(
      &state,
      &KeyArgs {
        connection_id: id.clone(),
        key: "soquel_test:mcp:key".to_string(),
      },
    )
    .await
    .unwrap();
    assert_eq!(detail["key"], "soquel_test:mcp:key");
    assert_eq!(detail["value"]["kind"], "string");
    assert_eq!(detail["value"]["value"], "hello");

    // SQL tools refuse a key-value connection instead of half-working.
    let err = get_schema_impl(
      &state,
      &ConnectionArgs {
        connection_id: id.clone(),
      },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::Unsupported { .. }), "{err:?}");

    connection
      .kv()
      .unwrap()
      .delete_key("soquel_test:mcp:key")
      .await
      .unwrap();
  }

  #[tokio::test]
  async fn integration_mcp_doc_tools_read_mongo() {
    let Ok(addr) = std::env::var("SOQUEL_TEST_MONGO") else {
      return;
    };
    let (host, port) = addr.split_once(':').expect("host:port");
    let dir = tempfile::tempdir().unwrap();
    let (state, id) = kind_state(
      &dir,
      ConnectorParams::Mongo(crate::profiles::MongoParams {
        host: host.to_string(),
        port: port.parse().unwrap(),
        database: None,
        username: Some("soquel".to_string()),
        auth_source: None,
        tls: false,
        tunnel_id: None,
      }),
    )
    .await;

    // The compose mongo seeds soquel_e2e for the e2e spec; read that.
    let databases = list_databases_impl(
      &state,
      &ConnectionArgs {
        connection_id: id.clone(),
      },
    )
    .await
    .unwrap();
    let names: Vec<&str> = databases
      .as_array()
      .unwrap()
      .iter()
      .map(|db| db["name"].as_str().unwrap())
      .collect();
    assert!(names.contains(&"soquel_e2e"), "{databases}");

    let collections = list_collections_impl(
      &state,
      &DatabaseArgs {
        connection_id: id.clone(),
        database: "soquel_e2e".to_string(),
      },
    )
    .await
    .unwrap();
    let collection = collections.as_array().unwrap()[0]["name"]
      .as_str()
      .unwrap()
      .to_string();

    let page = find_documents_impl(
      &state,
      &DocFindArgs {
        connection_id: id.clone(),
        database: "soquel_e2e".to_string(),
        collection: collection.clone(),
        filter: None,
        sort: None,
        limit: Some(3),
        cursor: None,
      },
    )
    .await
    .unwrap();
    assert!(!page["docs"].as_array().unwrap().is_empty(), "{page}");

    let count = count_documents_impl(
      &state,
      &DocCountArgs {
        connection_id: id.clone(),
        database: "soquel_e2e".to_string(),
        collection: collection.clone(),
        filter: None,
      },
    )
    .await
    .unwrap();
    assert!(count["count"].as_f64().unwrap() >= 1.0, "{count}");

    let indexes = list_indexes_impl(
      &state,
      &CollectionArgs {
        connection_id: id.clone(),
        database: "soquel_e2e".to_string(),
        collection,
      },
    )
    .await
    .unwrap();
    assert!(!indexes.as_array().unwrap().is_empty(), "{indexes}");
  }

  #[tokio::test]
  async fn integration_mcp_write_needs_approval() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG") else {
      return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (state, read_only, _hidden) = test_state(&dir, &url);
    let write = |sql: &str| QueryArgs {
      connection_id: read_only.clone(),
      sql: sql.to_string(),
    };

    // read-only never reaches the approver, whatever it would answer.
    let err = run_query_impl(
      &state,
      &FixedApprover(true),
      &write("CREATE TABLE app.leak (id int)"),
    )
    .await
    .unwrap_err();
    let Error::Unsupported { message } = err else {
      panic!("expected unsupported: {err:?}");
    };
    assert!(message.contains("read-only for agents"), "{message}");

    // Same profile, upgraded to write-with-approval.
    {
      let mut profiles = state.profiles.lock().unwrap();
      let input = crate::profiles::ConnectionInput {
        name: "agent-visible".to_string(),
        env: Env::Dev,
        group: None,
        agent_access: AgentAccess::WriteWithApproval,
        credential: Default::default(),
        params: pg_params(&url),
        password: None,
      };
      profiles.update(&read_only, &input).unwrap();
    }

    let denied = run_query_impl(
      &state,
      &DenyingApprover,
      &write("CREATE TABLE app.denied (id int)"),
    )
    .await
    .unwrap_err();
    let Error::Unsupported { message } = denied else {
      panic!("expected unsupported: {denied:?}");
    };
    assert!(message.contains("not approved"), "{message}");
    // Denial means nothing ran.
    let missing = run_query_impl(
      &state,
      &FixedApprover(true),
      &write("SELECT to_regclass('app.denied') IS NULL AS absent"),
    )
    .await
    .unwrap();
    assert_eq!(missing["result"]["statements"][0]["rows"][0][0], "t");

    // Approved: the write lands, outside the read-only path.
    run_query_impl(
      &state,
      &FixedApprover(true),
      &write("CREATE TABLE app.approved_probe (id int)"),
    )
    .await
    .unwrap();
    let exists = run_query_impl(
      &state,
      &FixedApprover(true),
      &write("SELECT to_regclass('app.approved_probe') IS NOT NULL AS there"),
    )
    .await
    .unwrap();
    assert_eq!(exists["result"]["statements"][0]["rows"][0][0], "t");
    run_query_impl(
      &state,
      &FixedApprover(true),
      &write("DROP TABLE app.approved_probe"),
    )
    .await
    .unwrap();
  }
}
