use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deadpool::managed::{self, Metrics, Object, Pool, RecycleError, RecycleResult};
use tokio_postgres::types::{Kind as PgKind, Type as PgType};
use tokio_postgres::{AsyncMessage, CancelToken, Client, Config, SimpleQueryMessage};

use crate::connectors::{
  Capability, ColumnKind, Connection, Connector, Introspect, QueryColumn, QueryResult,
  ServerNotice, SortDirection, SqlQuery, SqlSession, StatementResult, TableRowsRequest,
};
use crate::error::Error;
use crate::profiles::{ConnectionProfile, SslMode};

mod introspect;
mod tls;

const POOL_MAX_SIZE: usize = 4;

pub struct PostgresConnector;

#[async_trait::async_trait]
impl Connector for PostgresConnector {
  fn capabilities(&self) -> &'static [Capability] {
    &[Capability::SqlQuery, Capability::Introspection]
  }

  async fn connect(
    &self,
    profile: &ConnectionProfile,
    secret: Option<&str>,
  ) -> Result<Box<dyn Connection>, Error> {
    let mut config = Config::new();
    config
      .host(&profile.host)
      .port(profile.port)
      .dbname(&profile.database)
      .user(&profile.user)
      .application_name("soquel")
      .ssl_mode(tls::config_ssl_mode(profile.ssl_mode))
      .connect_timeout(Duration::from_secs(10));
    if let Some(secret) = secret {
      config.password(secret);
    }
    let connection = PostgresConnection::new(config, profile.ssl_mode)?;
    // Surface auth/reachability/TLS errors now, not on the first query.
    drop(connection.checkout().await?);
    Ok(Box::new(connection))
  }
}

pub(super) struct PooledPg {
  pub(super) client: Client,
  notices: Arc<Mutex<Vec<ServerNotice>>>,
}

pub(super) struct PgManager {
  config: Config,
  ssl_mode: SslMode,
}

async fn connect_pg(config: &Config, ssl_mode: SslMode) -> Result<PooledPg, Error> {
  let tls = tls::connector(ssl_mode)?;
  let (client, mut connection) = config.connect(tls).await?;
  let notices: Arc<Mutex<Vec<ServerNotice>>> = Arc::default();
  let sink = notices.clone();
  tauri::async_runtime::spawn(async move {
    while let Some(message) = std::future::poll_fn(|cx| connection.poll_message(cx)).await {
      match message {
        Ok(AsyncMessage::Notice(notice)) => sink.lock().unwrap().push(ServerNotice {
          severity: notice.severity().to_string(),
          message: notice.message().to_string(),
        }),
        Ok(_) => {}
        Err(err) => {
          log::warn!("postgres connection closed: {err}");
          break;
        }
      }
    }
  });
  Ok(PooledPg { client, notices })
}

/// Prepare-for-types, run over the simple protocol, drain this client's notices.
async fn run_script(pg: &PooledPg, sql: &str) -> Result<QueryResult, Error> {
  pg.notices.lock().unwrap().clear();
  let start = Instant::now();
  // Single statements get type metadata from a prepare; multi-statement
  // scripts fail to prepare and degrade to names only.
  let types = pg.client.prepare(sql).await.ok().map(|statement| {
    statement
      .columns()
      .iter()
      .map(|c| c.type_().clone())
      .collect::<Vec<_>>()
  });
  let messages = pg.client.simple_query(sql).await?;
  let mut statements = collect_statements(messages);
  if let (Some(types), [statement]) = (&types, &mut statements[..]) {
    apply_types(statement, types);
  }
  let notices = std::mem::take(&mut *pg.notices.lock().unwrap());
  Ok(QueryResult {
    statements,
    notices,
    duration_ms: start.elapsed().as_secs_f64() * 1000.0,
  })
}

impl managed::Manager for PgManager {
  type Type = PooledPg;
  type Error = Error;

  async fn create(&self) -> Result<PooledPg, Error> {
    connect_pg(&self.config, self.ssl_mode).await
  }

  async fn recycle(&self, pg: &mut PooledPg, _: &Metrics) -> RecycleResult<Error> {
    if pg.client.is_closed() {
      return Err(RecycleError::message("connection closed"));
    }
    Ok(())
  }
}

pub struct PostgresConnection {
  pool: Pool<PgManager>,
  ssl_mode: SslMode,
  cancels: Mutex<HashMap<u64, CancelToken>>,
  next_cancel_id: AtomicU64,
}

impl PostgresConnection {
  fn new(config: Config, ssl_mode: SslMode) -> Result<Self, Error> {
    let pool = Pool::builder(PgManager { config, ssl_mode })
      .max_size(POOL_MAX_SIZE)
      .build()
      .map_err(|err| Error::Database {
        message: format!("connection pool: {err}"),
      })?;
    Ok(Self {
      pool,
      ssl_mode,
      cancels: Mutex::new(HashMap::new()),
      next_cancel_id: AtomicU64::new(0),
    })
  }

  pub(super) async fn checkout(&self) -> Result<Object<PgManager>, Error> {
    self.pool.get().await.map_err(|err| match err {
      managed::PoolError::Backend(err) => err,
      other => Error::Database {
        message: format!("connection pool: {other}"),
      },
    })
  }

  fn register_cancel(&self, token: CancelToken) -> CancelGuard<'_> {
    let id = self.next_cancel_id.fetch_add(1, Ordering::Relaxed);
    self.cancels.lock().unwrap().insert(id, token);
    CancelGuard {
      connection: self,
      id,
    }
  }

  async fn execute_script(&self, sql: &str) -> Result<QueryResult, Error> {
    let pg = self.checkout().await?;
    let _guard = self.register_cancel(pg.client.cancel_token());
    run_script(&pg, sql).await
  }
}

pub struct PostgresSession {
  pg: PooledPg,
  ssl_mode: SslMode,
  cancel: CancelToken,
}

#[async_trait::async_trait]
impl SqlSession for PostgresSession {
  async fn run_query(&self, sql: &str) -> Result<QueryResult, Error> {
    run_script(&self.pg, sql).await
  }

  async fn cancel(&self) -> Result<(), Error> {
    self
      .cancel
      .clone()
      .cancel_query(tls::connector(self.ssl_mode)?)
      .await?;
    Ok(())
  }

  // Dropping the client terminates the connection task.
  async fn close(&self) -> Result<(), Error> {
    Ok(())
  }
}

struct CancelGuard<'a> {
  connection: &'a PostgresConnection,
  id: u64,
}

impl Drop for CancelGuard<'_> {
  fn drop(&mut self) {
    self.connection.cancels.lock().unwrap().remove(&self.id);
  }
}

#[async_trait::async_trait]
impl Connection for PostgresConnection {
  async fn health(&self) -> Result<(), Error> {
    let pg = self.checkout().await?;
    pg.client.simple_query("SELECT 1").await?;
    Ok(())
  }

  async fn close(&self) -> Result<(), Error> {
    self.pool.close();
    Ok(())
  }

  fn sql(&self) -> Option<&dyn SqlQuery> {
    Some(self)
  }

  fn introspect(&self) -> Option<&dyn Introspect> {
    Some(self)
  }
}

#[async_trait::async_trait]
impl SqlQuery for PostgresConnection {
  async fn run_query(&self, sql: &str) -> Result<QueryResult, Error> {
    self.execute_script(sql).await
  }

  async fn cancel(&self) -> Result<(), Error> {
    let tokens: Vec<CancelToken> = self.cancels.lock().unwrap().values().cloned().collect();
    for token in tokens {
      token.cancel_query(tls::connector(self.ssl_mode)?).await?;
    }
    Ok(())
  }

  async fn table_rows(&self, request: &TableRowsRequest) -> Result<QueryResult, Error> {
    let mut sql = format!(
      "SELECT * FROM {}.{}",
      quote_ident(&request.schema),
      quote_ident(&request.table)
    );
    if let Some(sort) = &request.sort {
      let direction = match sort.direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
      };
      sql.push_str(&format!(
        " ORDER BY {} {direction}",
        quote_ident(&sort.column)
      ));
    }
    sql.push_str(&format!(
      " LIMIT {} OFFSET {}",
      request.limit.min(1000),
      request.offset
    ));
    self.run_query(&sql).await
  }

  async fn open_session(&self) -> Result<Box<dyn SqlSession>, Error> {
    let manager = self.pool.manager();
    let pg = connect_pg(&manager.config, manager.ssl_mode).await?;
    Ok(Box::new(PostgresSession {
      cancel: pg.client.cancel_token(),
      ssl_mode: manager.ssl_mode,
      pg,
    }))
  }
}

// Identifiers come from the UI: quoting is the injection boundary.
fn quote_ident(ident: &str) -> String {
  format!("\"{}\"", ident.replace('"', "\"\""))
}

fn collect_statements(messages: Vec<SimpleQueryMessage>) -> Vec<StatementResult> {
  let mut statements = Vec::new();
  let mut current = StatementResult::default();
  for message in messages {
    match message {
      SimpleQueryMessage::RowDescription(columns) => {
        current.columns = columns
          .iter()
          .map(|c| QueryColumn {
            name: c.name().to_string(),
            data_type: None,
            kind: ColumnKind::Other,
          })
          .collect();
      }
      SimpleQueryMessage::Row(row) => {
        current.rows.push(
          (0..row.len())
            .map(|i| row.get(i).map(String::from))
            .collect(),
        );
      }
      SimpleQueryMessage::CommandComplete(count) => {
        current.rows_affected = count as f64;
        statements.push(std::mem::take(&mut current));
      }
      _ => {}
    }
  }
  statements
}

fn apply_types(statement: &mut StatementResult, types: &[PgType]) {
  if statement.columns.len() != types.len() {
    return;
  }
  for (column, ty) in statement.columns.iter_mut().zip(types) {
    column.data_type = Some(type_name(ty));
    column.kind = column_kind(ty);
  }
}

fn type_name(ty: &PgType) -> String {
  match ty.kind() {
    PgKind::Array(inner) => format!("{}[]", inner.name()),
    _ => ty.name().to_string(),
  }
}

fn column_kind(ty: &PgType) -> ColumnKind {
  if matches!(ty.kind(), PgKind::Array(_)) {
    ColumnKind::Array
  } else if *ty == PgType::BOOL {
    ColumnKind::Bool
  } else if [
    PgType::INT2,
    PgType::INT4,
    PgType::INT8,
    PgType::FLOAT4,
    PgType::FLOAT8,
    PgType::NUMERIC,
    PgType::OID,
  ]
  .contains(ty)
  {
    ColumnKind::Number
  } else if [PgType::JSON, PgType::JSONB].contains(ty) {
    ColumnKind::Json
  } else if *ty == PgType::BYTEA {
    ColumnKind::Bytes
  } else if [
    PgType::TIMESTAMP,
    PgType::TIMESTAMPTZ,
    PgType::DATE,
    PgType::TIME,
    PgType::TIMETZ,
    PgType::INTERVAL,
  ]
  .contains(ty)
  {
    ColumnKind::DateTime
  } else if *ty == PgType::UUID {
    ColumnKind::Uuid
  } else if [
    PgType::TEXT,
    PgType::VARCHAR,
    PgType::BPCHAR,
    PgType::NAME,
    PgType::CHAR,
  ]
  .contains(ty)
  {
    ColumnKind::Text
  } else {
    ColumnKind::Other
  }
}

#[cfg(test)]
pub mod tests {
  use super::*;
  use crate::profiles::ConnectorKind;

  // Convention: integration_* tests run via `pnpm test:integration` (needs `pnpm db:test`),
  // each gated by its connector's env var, and skip silently otherwise.
  #[test]
  fn quote_ident_escapes_quotes() {
    assert_eq!(quote_ident("customers"), "\"customers\"");
    assert_eq!(quote_ident("MiXeD"), "\"MiXeD\"");
    assert_eq!(quote_ident("evil\"; DROP--"), "\"evil\"\"; DROP--\"");
  }

  #[test]
  fn column_kind_maps_common_types() {
    assert_eq!(column_kind(&PgType::INT8), ColumnKind::Number);
    assert_eq!(column_kind(&PgType::NUMERIC), ColumnKind::Number);
    assert_eq!(column_kind(&PgType::JSONB), ColumnKind::Json);
    assert_eq!(column_kind(&PgType::TIMESTAMPTZ), ColumnKind::DateTime);
    assert_eq!(column_kind(&PgType::TEXT_ARRAY), ColumnKind::Array);
    assert_eq!(column_kind(&PgType::POINT), ColumnKind::Other);
    assert_eq!(type_name(&PgType::TEXT_ARRAY), "text[]");
  }

  fn connection_from_url(url: &str, ssl_mode: SslMode) -> PostgresConnection {
    let mut config: Config = url.parse().unwrap();
    config.ssl_mode(tls::config_ssl_mode(ssl_mode));
    PostgresConnection::new(config, ssl_mode).unwrap()
  }

  pub async fn test_connection_from_env() -> Option<PostgresConnection> {
    let url = std::env::var("SOQUEL_TEST_PG").ok()?;
    Some(connection_from_url(&url, SslMode::Prefer))
  }

  #[tokio::test]
  async fn integration_postgres_query_roundtrip() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };

    pg.health().await.unwrap();

    let result = pg
      .run_query("SELECT 1 AS one; SELECT 'a' AS a, NULL AS b")
      .await
      .unwrap();
    assert_eq!(result.statements.len(), 2);
    assert_eq!(result.statements[0].columns[0].name, "one");
    assert_eq!(result.statements[0].rows_affected, 1.0);
    assert_eq!(
      result.statements[1].rows[0],
      vec![Some("a".to_string()), None]
    );
    // Multi-statement scripts cannot be prepared: no type metadata.
    assert_eq!(result.statements[0].columns[0].data_type, None);
  }

  #[tokio::test]
  async fn integration_postgres_single_statement_carries_types() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let result = pg
      .run_query("SELECT 1 AS one, 'a'::text AS a, now() AS ts")
      .await
      .unwrap();
    let columns = &result.statements[0].columns;
    assert_eq!(columns[0].data_type.as_deref(), Some("int4"));
    assert_eq!(columns[0].kind, ColumnKind::Number);
    assert_eq!(columns[1].kind, ColumnKind::Text);
    assert_eq!(columns[2].data_type.as_deref(), Some("timestamptz"));
    assert_eq!(columns[2].kind, ColumnKind::DateTime);
  }

  #[tokio::test]
  async fn integration_postgres_notices_surface_in_results() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let result = pg
      .run_query("DO $$ BEGIN RAISE NOTICE 'soquel test notice'; END $$")
      .await
      .unwrap();
    assert!(result
      .notices
      .iter()
      .any(|n| n.message == "soquel test notice" && n.severity == "NOTICE"));

    // The buffer is per query: a follow-up query starts clean.
    let clean = pg.run_query("SELECT 1").await.unwrap();
    assert!(clean.notices.is_empty());
  }

  #[tokio::test(flavor = "multi_thread")]
  async fn integration_postgres_pool_unblocks_concurrent_queries() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let pg = Arc::new(pg);
    let slow = tokio::spawn({
      let pg = pg.clone();
      async move { pg.run_query("SELECT pg_sleep(2)").await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;

    let start = Instant::now();
    pg.run_query("SELECT 1").await.unwrap();
    assert!(
      start.elapsed() < Duration::from_secs(1),
      "quick query waited on the slow one"
    );
    slow.await.unwrap().unwrap();
  }

  #[tokio::test(flavor = "multi_thread")]
  async fn integration_postgres_cancel_kills_running_query() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let pg = Arc::new(pg);
    let slow = tokio::spawn({
      let pg = pg.clone();
      async move { pg.run_query("SELECT pg_sleep(30)").await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;

    pg.cancel().await.unwrap();
    let Err(Error::Database { message }) = slow.await.unwrap() else {
      panic!("expected the canceled query to fail");
    };
    assert!(
      message.contains("canceling statement due to user request"),
      "unexpected message: {message}"
    );
  }

  #[tokio::test(flavor = "multi_thread")]
  async fn integration_postgres_session_pins_state() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let session = pg.open_session().await.unwrap();
    session
      .run_query("SET soquel.flag = 'pinned'")
      .await
      .unwrap();
    let shown = session.run_query("SHOW soquel.flag").await.unwrap();
    assert_eq!(
      shown.statements[0].rows[0][0].as_deref(),
      Some("pinned"),
      "session state must stick across its own runs"
    );
    // Custom GUCs are per-backend: the pool must not know this one.
    assert!(pg.run_query("SHOW soquel.flag").await.is_err());
    session.close().await.unwrap();
    pg.run_query("SELECT 1").await.unwrap();
  }

  #[tokio::test(flavor = "multi_thread")]
  async fn integration_postgres_session_cancel_kills_query() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let session: Arc<dyn SqlSession> = pg.open_session().await.unwrap().into();
    let slow = tokio::spawn({
      let session = session.clone();
      async move { session.run_query("SELECT pg_sleep(30)").await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;

    session.cancel().await.unwrap();
    let Err(Error::Database { message }) = slow.await.unwrap() else {
      panic!("expected the canceled query to fail");
    };
    assert!(
      message.contains("canceling statement due to user request"),
      "unexpected message: {message}"
    );
    // The session survives a cancel.
    session.run_query("SELECT 1").await.unwrap();
  }

  #[tokio::test]
  async fn integration_postgres_require_tls_fails_on_plaintext_server() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG") else {
      return;
    };
    // The compose postgres has no TLS: require must fail, prefer falls back.
    let pg = connection_from_url(&url, SslMode::Require);
    let Err(Error::Database { message }) = pg.health().await else {
      panic!("expected require to fail against a plaintext server");
    };
    assert!(!message.is_empty());
  }

  #[tokio::test]
  async fn integration_postgres_table_rows_sorts_and_paginates() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let result = pg
      .table_rows(&TableRowsRequest {
        schema: "app".to_string(),
        table: "customers".to_string(),
        limit: 2,
        offset: 0,
        sort: Some(crate::connectors::SortSpec {
          column: "name".to_string(),
          direction: SortDirection::Desc,
        }),
      })
      .await
      .unwrap();
    let statement = &result.statements[0];
    assert_eq!(statement.rows.len(), 2);
    assert_eq!(statement.rows[0][1], Some("Grace Hopper".to_string()));
    let tags = statement.columns.iter().find(|c| c.name == "tags").unwrap();
    assert_eq!(tags.data_type.as_deref(), Some("text[]"));
    assert_eq!(tags.kind, ColumnKind::Array);

    let next = pg
      .table_rows(&TableRowsRequest {
        schema: "app".to_string(),
        table: "customers".to_string(),
        limit: 2,
        offset: 2,
        sort: Some(crate::connectors::SortSpec {
          column: "name".to_string(),
          direction: SortDirection::Desc,
        }),
      })
      .await
      .unwrap();
    assert_eq!(next.statements[0].rows.len(), 1);
    assert_eq!(next.statements[0].rows[0][1], Some("Ada Lovelace".to_string()));
  }

  fn profile_from_env_url(url: &str) -> ConnectionProfile {
    let config: Config = url.parse().unwrap();
    let tokio_postgres::config::Host::Tcp(host) = &config.get_hosts()[0] else {
      panic!("expected a tcp host");
    };
    ConnectionProfile {
      id: String::new(),
      name: "test".to_string(),
      env: crate::profiles::Env::Dev,
      kind: ConnectorKind::Postgres,
      host: host.clone(),
      port: config.get_ports()[0],
      database: config.get_dbname().unwrap().to_string(),
      user: config.get_user().unwrap().to_string(),
      ssl_mode: SslMode::Prefer,
      tunnel_id: None,
    }
  }

  #[tokio::test]
  async fn integration_postgres_auth_failure_maps_to_database_error() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG") else {
      return;
    };
    let profile = profile_from_env_url(&url);
    let result = PostgresConnector
      .connect(&profile, Some("definitely-wrong"))
      .await;
    let Err(Error::Database { message }) = result.map(|_| ()) else {
      panic!("expected a database error");
    };
    assert!(
      message.contains("password authentication failed"),
      "unhelpful message: {message}"
    );
  }

  #[tokio::test]
  async fn integration_postgres_unreachable_maps_to_database_error() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG") else {
      return;
    };
    let mut profile = profile_from_env_url(&url);
    profile.port = 59999;
    let result = PostgresConnector.connect(&profile, None).await;
    let Err(Error::Database { message }) = result.map(|_| ()) else {
      panic!("expected a database error");
    };
    assert!(!message.is_empty());
  }
}
