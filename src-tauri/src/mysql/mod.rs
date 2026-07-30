//! MySQL connector: mysql_async pool, text values rendered to strings.
//! Browse/edit surfaces land in a later round; only queries ship here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use mysql_async::consts::ColumnType;
use mysql_async::prelude::Queryable;
use mysql_async::{
  Column, Opts, OptsBuilder, Pool, PoolConstraints, PoolOpts, Row, SslOpts, Value,
};

use crate::connectors::{
  ApplyResult, Capability, ColumnKind, Connection, Connector, LocalForward, QueryColumn,
  QueryResult, RowsChunk, SqlQuery, SqlSession, StatementResult, StreamSummary, TableChanges,
  TableRowsRequest,
};
use crate::error::Error;
use crate::profiles::{ConnectionProfile, SslMode};

const POOL_MAX_SIZE: usize = 4;

pub struct MysqlConnector;

#[async_trait::async_trait]
impl Connector for MysqlConnector {
  fn capabilities(&self) -> &'static [Capability] {
    // Introspection follows in the next round.
    &[Capability::SqlQuery]
  }

  async fn connect(
    &self,
    profile: &ConnectionProfile,
    secret: Option<&str>,
    forward: Option<LocalForward>,
  ) -> Result<Box<dyn Connection>, Error> {
    let opts = build_opts(profile, secret, forward, ssl_opts(profile, forward));
    let connection = match MysqlConnection::open(opts).await {
      Ok(connection) => connection,
      // libpq-style prefer: retry in plaintext when the TLS attempt fails.
      Err(_) if profile.ssl_mode == SslMode::Prefer => {
        let opts = build_opts(profile, secret, forward, None);
        MysqlConnection::open(opts).await?
      }
      Err(err) => return Err(err),
    };
    Ok(Box::new(connection))
  }
}

fn build_opts(
  profile: &ConnectionProfile,
  secret: Option<&str>,
  forward: Option<LocalForward>,
  ssl: Option<SslOpts>,
) -> Opts {
  let (host, port) = match forward {
    Some(forward) => ("127.0.0.1".to_string(), forward.port),
    None => (profile.host.clone(), profile.port),
  };
  OptsBuilder::default()
    .ip_or_hostname(host)
    .tcp_port(port)
    .db_name(Some(profile.database.clone()))
    .user(Some(profile.user.clone()))
    .pass(secret.map(str::to_string))
    .ssl_opts(ssl)
    .pool_opts(
      PoolOpts::default()
        .with_constraints(PoolConstraints::new(0, POOL_MAX_SIZE).expect("static pool bounds")),
    )
    .into()
}

/// mysql_async has no host/hostaddr split: through a tunnel, verify-full
/// degrades to verify-ca (chain checked, hostname not).
fn ssl_opts(profile: &ConnectionProfile, forward: Option<LocalForward>) -> Option<SslOpts> {
  match profile.ssl_mode {
    SslMode::Disable => None,
    // Encrypt without verifying, like the postgres AcceptAll verifier.
    SslMode::Prefer | SslMode::Require => Some(
      SslOpts::default()
        .with_danger_accept_invalid_certs(true)
        .with_danger_skip_domain_validation(true),
    ),
    SslMode::VerifyFull => {
      let mut ssl = SslOpts::default();
      if let Some(path) = &profile.ssl_root_cert {
        ssl = ssl.with_root_certs(vec![std::path::PathBuf::from(path).into()]);
      }
      if forward.is_some() {
        ssl = ssl.with_danger_skip_domain_validation(true);
      }
      Some(ssl)
    }
  }
}

pub struct MysqlConnection {
  pool: Pool,
  /// Kept for sessions: dedicated conns live outside the pool.
  opts: Opts,
  server_version: OnceLock<String>,
  /// guard id -> thread id, for KILL QUERY on a side connection.
  active_threads: Mutex<HashMap<u64, u32>>,
  next_guard_id: AtomicU64,
}

impl MysqlConnection {
  async fn open(opts: Opts) -> Result<Self, Error> {
    let pool = Pool::new(opts.clone());
    let connection = Self {
      pool,
      opts,
      server_version: OnceLock::new(),
      active_threads: Mutex::new(HashMap::new()),
      next_guard_id: AtomicU64::new(0),
    };
    // Surface auth/reachability/TLS errors now, not on the first query.
    let mut conn = connection.pool.get_conn().await?;
    let version: Option<String> = conn.query_first("SELECT VERSION()").await?;
    if let Some(version) = version {
      let _ = connection.server_version.set(version);
    }
    drop(conn);
    Ok(connection)
  }

  fn register(&self, thread_id: u32) -> ThreadGuard<'_> {
    let id = self.next_guard_id.fetch_add(1, Ordering::Relaxed);
    self.active_threads.lock().unwrap().insert(id, thread_id);
    ThreadGuard {
      connection: self,
      id,
    }
  }
}

struct ThreadGuard<'a> {
  connection: &'a MysqlConnection,
  id: u64,
}

impl Drop for ThreadGuard<'_> {
  fn drop(&mut self) {
    self
      .connection
      .active_threads
      .lock()
      .unwrap()
      .remove(&self.id);
  }
}

#[async_trait::async_trait]
impl Connection for MysqlConnection {
  async fn health(&self) -> Result<(), Error> {
    let mut conn = self.pool.get_conn().await?;
    conn.query_drop("SELECT 1").await?;
    Ok(())
  }

  async fn close(&self) -> Result<(), Error> {
    self.pool.clone().disconnect().await?;
    Ok(())
  }

  fn server_version(&self) -> Option<String> {
    self.server_version.get().cloned()
  }

  fn sql(&self) -> Option<&dyn SqlQuery> {
    Some(self)
  }
}

#[async_trait::async_trait]
impl SqlQuery for MysqlConnection {
  async fn run_query(&self, sql: &str) -> Result<QueryResult, Error> {
    let mut conn = self.pool.get_conn().await?;
    let _guard = self.register(conn.id());
    run_script(&mut conn, sql).await
  }

  async fn cancel(&self) -> Result<(), Error> {
    let threads: Vec<u32> = self
      .active_threads
      .lock()
      .unwrap()
      .values()
      .copied()
      .collect();
    let mut conn = self.pool.get_conn().await?;
    for thread in threads {
      conn.query_drop(format!("KILL QUERY {thread}")).await?;
    }
    Ok(())
  }

  async fn table_rows(&self, _request: &TableRowsRequest) -> Result<QueryResult, Error> {
    Err(browse_unsupported())
  }

  async fn stream_rows(
    &self,
    _request: &TableRowsRequest,
    _on_chunk: Box<dyn Fn(RowsChunk) -> bool + Send>,
  ) -> Result<StreamSummary, Error> {
    Err(browse_unsupported())
  }

  async fn apply_changes(&self, _changes: &TableChanges) -> Result<ApplyResult, Error> {
    Err(browse_unsupported())
  }

  async fn open_session(&self) -> Result<Box<dyn SqlSession>, Error> {
    let conn = mysql_async::Conn::new(self.opts.clone()).await?;
    let thread_id = conn.id();
    Ok(Box::new(MysqlSession {
      conn: tokio::sync::Mutex::new(conn),
      pool: self.pool.clone(),
      thread_id,
    }))
  }
}

fn browse_unsupported() -> Error {
  Error::Unsupported {
    message: "table browsing for mysql lands in a later round".to_string(),
  }
}

/// A dedicated client outside the pool: session state (SET, transactions)
/// sticks, and cancel targets only this session's thread.
pub struct MysqlSession {
  conn: tokio::sync::Mutex<mysql_async::Conn>,
  pool: Pool,
  thread_id: u32,
}

#[async_trait::async_trait]
impl SqlSession for MysqlSession {
  async fn run_query(&self, sql: &str) -> Result<QueryResult, Error> {
    let mut conn = self.conn.lock().await;
    run_script(&mut conn, sql).await
  }

  async fn cancel(&self) -> Result<(), Error> {
    let mut side = self.pool.get_conn().await?;
    side
      .query_drop(format!("KILL QUERY {}", self.thread_id))
      .await?;
    Ok(())
  }

  async fn close(&self) -> Result<(), Error> {
    // Dropping the detached conn closes it; nothing to hand back.
    Ok(())
  }
}

async fn run_script(conn: &mut mysql_async::Conn, sql: &str) -> Result<QueryResult, Error> {
  let start = Instant::now();
  let mut result = conn.query_iter(sql).await?;
  let mut statements = Vec::new();
  // columns() is None once no result set is pending; is_empty() cannot tell a
  // pending OK-packet set (empty columns) apart from the end of the script.
  while let Some(meta) = result.columns() {
    let columns: Vec<QueryColumn> = meta.iter().map(query_column).collect();
    // For OK-packet sets the count must be read before collect() advances.
    let affected = result.affected_rows() as f64;
    let rows: Vec<Row> = result.collect().await?;
    let rows_affected = if columns.is_empty() {
      affected
    } else {
      rows.len() as f64
    };
    statements.push(StatementResult {
      columns,
      rows: rows.into_iter().map(row_text).collect(),
      rows_affected,
    });
  }
  Ok(QueryResult {
    statements,
    notices: Vec::new(),
    duration_ms: start.elapsed().as_secs_f64() * 1000.0,
  })
}

fn query_column(column: &Column) -> QueryColumn {
  QueryColumn {
    name: column.name_str().to_string(),
    data_type: Some(type_name(column).to_string()),
    kind: column_kind(column),
  }
}

fn row_text(mut row: Row) -> Vec<Option<String>> {
  let columns = row.columns();
  (0..row.len())
    .map(|index| {
      let value = row.take::<Value, _>(index)?;
      value_text(value, &columns[index])
    })
    .collect()
}

// The 'binary' pseudo-charset distinguishes BLOB from TEXT, VARBINARY from VARCHAR.
const BINARY_CHARSET: u16 = 63;

fn is_binary(column: &Column) -> bool {
  column.character_set() == BINARY_CHARSET
    && matches!(
      column.column_type(),
      ColumnType::MYSQL_TYPE_BLOB
        | ColumnType::MYSQL_TYPE_TINY_BLOB
        | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
        | ColumnType::MYSQL_TYPE_LONG_BLOB
        | ColumnType::MYSQL_TYPE_STRING
        | ColumnType::MYSQL_TYPE_VAR_STRING
        | ColumnType::MYSQL_TYPE_VARCHAR
    )
}

fn value_text(value: Value, column: &Column) -> Option<String> {
  match value {
    Value::NULL => None,
    Value::Bytes(bytes) => Some(if is_binary(column) {
      let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
      format!("0x{hex}")
    } else {
      String::from_utf8_lossy(&bytes).into_owned()
    }),
    Value::Int(value) => Some(value.to_string()),
    Value::UInt(value) => Some(value.to_string()),
    Value::Float(value) => Some(value.to_string()),
    Value::Double(value) => Some(value.to_string()),
    Value::Date(year, month, day, hour, minute, second, micros) => Some(if micros > 0 {
      format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}")
    } else if hour == 0
      && minute == 0
      && second == 0
      && column.column_type() == ColumnType::MYSQL_TYPE_DATE
    {
      format!("{year:04}-{month:02}-{day:02}")
    } else {
      format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
    }),
    Value::Time(negative, days, hours, minutes, seconds, micros) => {
      let sign = if negative { "-" } else { "" };
      let hours = u32::from(hours) + days * 24;
      Some(if micros > 0 {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}.{micros:06}")
      } else {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}")
      })
    }
  }
}

fn column_kind(column: &Column) -> ColumnKind {
  match column.column_type() {
    ColumnType::MYSQL_TYPE_TINY
    | ColumnType::MYSQL_TYPE_SHORT
    | ColumnType::MYSQL_TYPE_LONG
    | ColumnType::MYSQL_TYPE_LONGLONG
    | ColumnType::MYSQL_TYPE_INT24
    | ColumnType::MYSQL_TYPE_FLOAT
    | ColumnType::MYSQL_TYPE_DOUBLE
    | ColumnType::MYSQL_TYPE_DECIMAL
    | ColumnType::MYSQL_TYPE_NEWDECIMAL
    | ColumnType::MYSQL_TYPE_YEAR => ColumnKind::Number,
    ColumnType::MYSQL_TYPE_TIMESTAMP
    | ColumnType::MYSQL_TYPE_DATETIME
    | ColumnType::MYSQL_TYPE_DATE
    | ColumnType::MYSQL_TYPE_NEWDATE
    | ColumnType::MYSQL_TYPE_TIME => ColumnKind::DateTime,
    ColumnType::MYSQL_TYPE_JSON => ColumnKind::Json,
    ColumnType::MYSQL_TYPE_BLOB
    | ColumnType::MYSQL_TYPE_TINY_BLOB
    | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
    | ColumnType::MYSQL_TYPE_LONG_BLOB
    | ColumnType::MYSQL_TYPE_STRING
    | ColumnType::MYSQL_TYPE_VAR_STRING
    | ColumnType::MYSQL_TYPE_VARCHAR => {
      if is_binary(column) {
        ColumnKind::Bytes
      } else {
        ColumnKind::Text
      }
    }
    _ => ColumnKind::Other,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::connectors::Connector;
  use crate::profiles::{ConnectorKind, Env};

  fn profile_from_env() -> Option<ConnectionProfile> {
    let addr = std::env::var("SOQUEL_TEST_MYSQL").ok()?;
    let (host, port) = addr
      .split_once(':')
      .expect("SOQUEL_TEST_MYSQL is host:port");
    Some(ConnectionProfile {
      id: String::new(),
      name: "test".to_string(),
      env: Env::Dev,
      kind: ConnectorKind::Mysql,
      host: host.to_string(),
      port: port.parse().unwrap(),
      database: "soquel_test".to_string(),
      user: "soquel".to_string(),
      ssl_mode: SslMode::Prefer,
      ssl_root_cert: None,
      tunnel_id: None,
      group: None,
    })
  }

  async fn test_connection_from_env() -> Option<Box<dyn Connection>> {
    let profile = profile_from_env()?;
    Some(
      MysqlConnector
        .connect(&profile, Some("soquel"), None)
        .await
        .unwrap(),
    )
  }

  #[tokio::test]
  async fn integration_mysql_query_roundtrip_with_multi_statements() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    connection.health().await.unwrap();

    let sql = connection.sql().unwrap();
    let result = sql
      .run_query("SELECT 1 AS one; SELECT name FROM customers WHERE email IS NULL")
      .await
      .unwrap();
    assert_eq!(result.statements.len(), 2);
    assert_eq!(result.statements[0].columns[0].name, "one");
    assert_eq!(result.statements[0].rows[0][0].as_deref(), Some("1"));
    assert_eq!(
      result.statements[1].rows[0][0].as_deref(),
      Some("Grace Hopper")
    );

    let ddl = sql
      .run_query("CREATE TEMPORARY TABLE tmp_probe (id INT); DROP TEMPORARY TABLE tmp_probe")
      .await
      .unwrap();
    assert_eq!(ddl.statements.len(), 2);
    assert!(ddl.statements.iter().all(|s| s.columns.is_empty()));
  }

  #[tokio::test]
  async fn integration_mysql_values_render_as_text() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let result = connection
      .sql()
      .unwrap()
      .run_query(
        "SELECT c.id, c.name, c.email, c.meta, o.amount, o.receipt, o.placed_at \
         FROM customers c JOIN orders o ON o.customer_id = c.id \
         WHERE o.note = 'first order'",
      )
      .await
      .unwrap();
    let statement = &result.statements[0];
    let kinds: Vec<ColumnKind> = statement.columns.iter().map(|c| c.kind).collect();
    assert_eq!(
      kinds,
      vec![
        ColumnKind::Number,
        ColumnKind::Text,
        ColumnKind::Text,
        ColumnKind::Json,
        ColumnKind::Number,
        ColumnKind::Bytes,
        ColumnKind::DateTime,
      ]
    );
    let row = &statement.rows[0];
    assert_eq!(row[0].as_deref(), Some("1"));
    assert_eq!(row[1].as_deref(), Some("Ada Lovelace"));
    assert!(row[3].as_deref().unwrap().contains("\"plan\""), "{row:?}");
    assert_eq!(row[4].as_deref(), Some("129.90"));
    assert_eq!(row[5].as_deref(), Some("0xdeadbeef"));

    let nulls = connection
      .sql()
      .unwrap()
      .run_query("SELECT email, meta FROM customers WHERE name = 'Grace Hopper'")
      .await
      .unwrap();
    assert_eq!(nulls.statements[0].rows[0], vec![None, None]);
  }

  #[tokio::test]
  async fn integration_mysql_server_version_is_captured() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let version = connection.server_version().expect("captured at connect");
    assert!(
      version.starts_with(|c: char| c.is_ascii_digit()),
      "{version}"
    );
  }

  #[tokio::test]
  async fn integration_mysql_session_pins_state_and_cancel_kills_query() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let session: std::sync::Arc<dyn SqlSession> = connection
      .sql()
      .unwrap()
      .open_session()
      .await
      .unwrap()
      .into();

    // Session variables stick across statements on the pinned conn.
    session.run_query("SET @probe = 41").await.unwrap();
    let result = session
      .run_query("SELECT @probe + 1 AS answer")
      .await
      .unwrap();
    assert_eq!(result.statements[0].rows[0][0].as_deref(), Some("42"));

    // Cancel from the outside kills only this session's running query.
    let runner = session.clone();
    let query = tokio::spawn(async move { runner.run_query("SELECT SLEEP(30)").await });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let started = Instant::now();
    session.cancel().await.unwrap();
    let outcome = query.await.unwrap();
    assert!(
      started.elapsed() < std::time::Duration::from_secs(5),
      "cancel took {:?}",
      started.elapsed()
    );
    // SLEEP interrupted by KILL QUERY returns 1 (not an error) per mysql docs.
    let interrupted = outcome.unwrap();
    assert_eq!(interrupted.statements[0].rows[0][0].as_deref(), Some("1"));

    // The session survives the cancel.
    session.run_query("SELECT 1").await.unwrap();
    session.close().await.unwrap();
  }

  #[tokio::test]
  async fn integration_mysql_browse_surfaces_are_gated() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let request = TableRowsRequest {
      schema: "soquel_test".to_string(),
      table: "customers".to_string(),
      limit: Some(10),
      offset: 0,
      sort: None,
      filters: vec![],
      include_ctid: false,
      include_xmin: false,
    };
    assert!(matches!(
      connection.sql().unwrap().table_rows(&request).await,
      Err(Error::Unsupported { .. })
    ));
    assert!(connection.introspect().is_none());
  }
}

fn type_name(column: &Column) -> &'static str {
  match column.column_type() {
    ColumnType::MYSQL_TYPE_TINY => "tinyint",
    ColumnType::MYSQL_TYPE_SHORT => "smallint",
    ColumnType::MYSQL_TYPE_LONG => "int",
    ColumnType::MYSQL_TYPE_LONGLONG => "bigint",
    ColumnType::MYSQL_TYPE_INT24 => "mediumint",
    ColumnType::MYSQL_TYPE_FLOAT => "float",
    ColumnType::MYSQL_TYPE_DOUBLE => "double",
    ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => "decimal",
    ColumnType::MYSQL_TYPE_YEAR => "year",
    ColumnType::MYSQL_TYPE_TIMESTAMP => "timestamp",
    ColumnType::MYSQL_TYPE_DATETIME => "datetime",
    ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "date",
    ColumnType::MYSQL_TYPE_TIME => "time",
    ColumnType::MYSQL_TYPE_JSON => "json",
    ColumnType::MYSQL_TYPE_BLOB
    | ColumnType::MYSQL_TYPE_TINY_BLOB
    | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
    | ColumnType::MYSQL_TYPE_LONG_BLOB => "blob",
    ColumnType::MYSQL_TYPE_STRING => "char",
    ColumnType::MYSQL_TYPE_VAR_STRING | ColumnType::MYSQL_TYPE_VARCHAR => "varchar",
    ColumnType::MYSQL_TYPE_BIT => "bit",
    ColumnType::MYSQL_TYPE_ENUM => "enum",
    ColumnType::MYSQL_TYPE_SET => "set",
    ColumnType::MYSQL_TYPE_GEOMETRY => "geometry",
    _ => "unknown",
  }
}
