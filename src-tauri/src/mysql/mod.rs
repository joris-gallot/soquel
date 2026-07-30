//! MySQL connector: mysql_async pool, text values rendered to strings.

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
  ApplyResult, Capability, ColumnKind, Connection, Connector, Introspect, LocalForward,
  QueryColumn, QueryResult, RowsChunk, SqlQuery, SqlSession, StatementResult, StreamSummary,
  TableChanges, TableRowsRequest,
};
use crate::error::Error;
use crate::profiles::{ConnectionProfile, SslMode};

mod browse;
mod introspect;

use browse::{build_change_statements, build_select, ChangeKind, ChangeStatement};

const POOL_MAX_SIZE: usize = 4;
const CHUNK_ROWS: usize = 200;

// Identifiers come from the UI: backtick quoting is the injection boundary.
pub(super) fn quote_ident(ident: &str) -> String {
  format!("`{}`", ident.replace('`', "``"))
}

pub struct MysqlConnector;

#[async_trait::async_trait]
impl Connector for MysqlConnector {
  fn capabilities(&self) -> &'static [Capability] {
    &[Capability::SqlQuery, Capability::Introspection]
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
  pub(super) pool: Pool,
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

  fn introspect(&self) -> Option<&dyn Introspect> {
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

  async fn table_rows(&self, request: &TableRowsRequest) -> Result<QueryResult, Error> {
    let mut conn = self.pool.get_conn().await?;
    let _guard = self.register(conn.id());
    let start = Instant::now();
    let names = base_columns(&mut conn, &request.schema, &request.table).await?;
    let plan = build_select(&names, request)?;
    let mut result = conn.exec_iter(plan.sql, text_params(&plan.params)).await?;

    let columns: Vec<QueryColumn> = result
      .columns()
      .map(|meta| meta.iter().map(query_column).collect())
      .unwrap_or_default();
    let rows: Vec<Row> = result.collect().await?;
    drop(result);
    let statement = StatementResult {
      columns,
      rows_affected: rows.len() as f64,
      rows: rows.into_iter().map(row_text).collect(),
    };
    Ok(QueryResult {
      statements: vec![statement],
      notices: Vec::new(),
      duration_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
  }

  async fn stream_rows(
    &self,
    request: &TableRowsRequest,
    on_chunk: Box<dyn Fn(RowsChunk) -> bool + Send>,
  ) -> Result<StreamSummary, Error> {
    use futures_util::TryStreamExt;

    let mut conn = self.pool.get_conn().await?;
    let _guard = self.register(conn.id());
    let start = Instant::now();
    let names = base_columns(&mut conn, &request.schema, &request.table).await?;
    let plan = build_select(&names, request)?;
    let mut result = conn.exec_iter(plan.sql, text_params(&plan.params)).await?;

    let mut columns: Option<Vec<QueryColumn>> = Some(
      result
        .columns()
        .map(|meta| meta.iter().map(query_column).collect())
        .unwrap_or_default(),
    );
    let mut total = 0u64;
    let mut chunk: Vec<Vec<Option<String>>> = Vec::with_capacity(CHUNK_ROWS);
    if let Some(mut stream) = result.stream::<Row>().await? {
      while let Some(row) = stream.try_next().await? {
        chunk.push(row_text(row));
        total += 1;
        if chunk.len() == CHUNK_ROWS
          && !on_chunk(RowsChunk {
            columns: columns.take(),
            rows: std::mem::take(&mut chunk),
          })
        {
          // Receiver gone: stop reading; the driver drains the rest on reuse.
          break;
        }
      }
    }
    if columns.is_some() || !chunk.is_empty() {
      on_chunk(RowsChunk {
        columns: columns.take(),
        rows: std::mem::take(&mut chunk),
      });
    }

    Ok(StreamSummary {
      rows: total as f64,
      duration_ms: start.elapsed().as_secs_f64() * 1000.0,
      notices: Vec::new(),
    })
  }

  async fn apply_changes(&self, changes: &TableChanges) -> Result<ApplyResult, Error> {
    let mut conn = self.pool.get_conn().await?;
    let names = base_columns(&mut conn, &changes.schema, &changes.table).await?;
    let statements = build_change_statements(&names, changes)?;
    if statements.is_empty() {
      return Err(Error::Unsupported {
        message: "no changes to apply".to_string(),
      });
    }

    let start = Instant::now();
    let mut tx = conn
      .start_transaction(mysql_async::TxOpts::default())
      .await?;
    match run_change_statements(&mut tx, &statements).await {
      Ok((updated, inserted, deleted)) => {
        tx.commit().await?;
        Ok(ApplyResult {
          updated,
          inserted,
          deleted,
          duration_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
      }
      Err(err) => {
        // The pooled client must go back clean.
        let _ = tx.rollback().await;
        Err(err)
      }
    }
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

/// Column identity for filter/sort/change validation, without executing.
async fn base_columns(
  conn: &mut mysql_async::Conn,
  schema: &str,
  table: &str,
) -> Result<Vec<String>, Error> {
  let stmt = conn
    .prep(format!(
      "SELECT * FROM {}.{}",
      quote_ident(schema),
      quote_ident(table)
    ))
    .await?;
  let names = stmt
    .columns()
    .iter()
    .map(|column| column.name_str().to_string())
    .collect();
  conn.close(stmt).await?;
  Ok(names)
}

fn text_params(params: &[String]) -> mysql_async::Params {
  if params.is_empty() {
    mysql_async::Params::Empty
  } else {
    mysql_async::Params::Positional(
      params
        .iter()
        .map(|value| Value::from(value.clone()))
        .collect(),
    )
  }
}

fn change_params(params: &[Option<String>]) -> mysql_async::Params {
  if params.is_empty() {
    mysql_async::Params::Empty
  } else {
    mysql_async::Params::Positional(
      params
        .iter()
        .map(|value| Value::from(value.clone()))
        .collect(),
    )
  }
}

async fn run_change_statements(
  tx: &mut mysql_async::Transaction<'_>,
  statements: &[ChangeStatement],
) -> Result<(u32, u32, u32), Error> {
  let (mut updated, mut inserted, mut deleted) = (0u32, 0u32, 0u32);
  for statement in statements {
    let result = tx
      .exec_iter(statement.sql.as_str(), change_params(&statement.params))
      .await?;
    let affected = result.affected_rows();
    drop(result);
    match statement.kind {
      ChangeKind::Update | ChangeKind::Delete => {
        if affected != 1 {
          let kind = if statement.kind == ChangeKind::Update {
            "row update"
          } else {
            "row delete"
          };
          let hint = if affected == 0 {
            "; the row may have been changed or deleted since it was loaded - refresh and retry"
          } else {
            ""
          };
          return Err(Error::Database {
            message: format!(
              "a {kind} matched {affected} rows instead of exactly 1; nothing was applied{hint}"
            ),
          });
        }
        if statement.kind == ChangeKind::Update {
          updated += 1;
        } else {
          deleted += 1;
        }
      }
      ChangeKind::Insert => inserted += u32::try_from(affected).unwrap_or(0),
    }
  }
  Ok((updated, inserted, deleted))
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
  use crate::connectors::{Connector, TableKind};
  use crate::profiles::{ConnectorKind, Env};

  #[test]
  fn quote_ident_doubles_backticks() {
    assert_eq!(quote_ident("plain"), "`plain`");
    assert_eq!(quote_ident("weird`name"), "`weird``name`");
  }

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

    // Formatting branches: date-only, negative time, float.
    let literals = connection
      .sql()
      .unwrap()
      .run_query(
        "SELECT DATE '2026-01-02' AS d, CAST('-25:00:00' AS TIME) AS t, CAST(1.5 AS FLOAT) AS f",
      )
      .await
      .unwrap();
    let row = &literals.statements[0].rows[0];
    assert_eq!(row[0].as_deref(), Some("2026-01-02"));
    assert_eq!(row[1].as_deref(), Some("-25:00:00"));
    assert_eq!(row[2].as_deref(), Some("1.5"));
  }

  #[tokio::test]
  async fn integration_mysql_server_errors_carry_the_message() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let result = connection
      .sql()
      .unwrap()
      .run_query("SELECT * FROM nope_table")
      .await;
    let Err(Error::Database { message }) = result else {
      panic!("expected a database error");
    };
    assert!(message.contains("doesn't exist"), "{message}");
  }

  #[tokio::test]
  async fn integration_mysql_connection_cancel_kills_pooled_query() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let connection: std::sync::Arc<Box<dyn Connection>> = connection.into();
    let runner = connection.clone();
    let query =
      tokio::spawn(async move { runner.sql().unwrap().run_query("SELECT SLEEP(30)").await });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let started = Instant::now();
    connection.sql().unwrap().cancel().await.unwrap();
    let outcome = query.await.unwrap().unwrap();
    assert!(
      started.elapsed() < std::time::Duration::from_secs(5),
      "cancel took {:?}",
      started.elapsed()
    );
    // KILL QUERY interrupts SLEEP, which then returns 1.
    assert_eq!(outcome.statements[0].rows[0][0].as_deref(), Some("1"));
    // The pool hands out a healthy connection afterwards.
    connection.health().await.unwrap();
  }

  #[tokio::test]
  async fn integration_mysql_ssl_mode_controls_encryption() {
    let Some(mut profile) = profile_from_env() else {
      return;
    };
    let cipher = |result: QueryResult| result.statements[0].rows[0][1].clone();

    // mysql 8 auto-generates certs: require must yield an encrypted session.
    profile.ssl_mode = SslMode::Require;
    let encrypted = MysqlConnector
      .connect(&profile, Some("soquel"), None)
      .await
      .unwrap();
    let status = encrypted
      .sql()
      .unwrap()
      .run_query("SHOW STATUS LIKE 'Ssl_cipher'")
      .await
      .unwrap();
    assert!(
      !cipher(status).unwrap_or_default().is_empty(),
      "require must encrypt"
    );

    profile.ssl_mode = SslMode::Disable;
    let plaintext = MysqlConnector
      .connect(&profile, Some("soquel"), None)
      .await
      .unwrap();
    let status = plaintext
      .sql()
      .unwrap()
      .run_query("SHOW STATUS LIKE 'Ssl_cipher'")
      .await
      .unwrap();
    assert_eq!(cipher(status).unwrap_or_default(), "");
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

  fn rows_request(
    table: &str,
    limit: Option<u32>,
    filters: Vec<crate::connectors::ColumnFilter>,
    sort: Option<crate::connectors::SortSpec>,
  ) -> TableRowsRequest {
    TableRowsRequest {
      schema: "soquel_test".to_string(),
      table: table.to_string(),
      limit,
      offset: 0,
      sort,
      filters,
      // pg-only hints, ignored by this connector.
      include_ctid: true,
      include_xmin: true,
    }
  }

  fn text_filter(
    column: &str,
    op: crate::connectors::FilterOp,
    value: &str,
  ) -> crate::connectors::ColumnFilter {
    crate::connectors::ColumnFilter {
      column: column.to_string(),
      op,
      value: Some(value.to_string()),
    }
  }

  #[tokio::test]
  async fn integration_mysql_table_rows_sorts_filters_paginates() {
    use crate::connectors::{FilterOp, SortDirection, SortSpec};

    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let sql = connection.sql().unwrap();
    // events: immutable across the suite, unlike customers (the apply test
    // inserts rows there concurrently).
    let result = sql
      .table_rows(&rows_request(
        "events",
        Some(2),
        vec![text_filter("kind", FilterOp::Eq, "view")],
        Some(SortSpec {
          column: "n".to_string(),
          direction: SortDirection::Desc,
        }),
      ))
      .await
      .unwrap();
    let statement = &result.statements[0];
    // Odd n seeds as 'view': 999 then 997 in desc order.
    assert_eq!(statement.rows.len(), 2);
    assert_eq!(statement.rows[0][2].as_deref(), Some("999"));
    assert_eq!(statement.rows[1][2].as_deref(), Some("997"));
    let kind = statement.columns.iter().find(|c| c.name == "kind").unwrap();
    assert_eq!(kind.kind, ColumnKind::Text);

    // Numeric coercion of text params: no explicit casts here, the server
    // must compare `n > '990'` numerically, not lexicographically.
    let numeric = sql
      .table_rows(&rows_request(
        "events",
        Some(100),
        vec![text_filter("n", FilterOp::Gt, "990")],
        None,
      ))
      .await
      .unwrap();
    assert_eq!(numeric.statements[0].rows.len(), 10);

    let unknown = sql
      .table_rows(&rows_request(
        "customers",
        Some(2),
        vec![text_filter("nope", FilterOp::Eq, "x")],
        None,
      ))
      .await;
    assert!(matches!(unknown, Err(Error::Unsupported { .. })));
  }

  #[tokio::test]
  async fn integration_mysql_stream_abort_leaves_connection_usable() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let sql = connection.sql().unwrap();

    let delivered: std::sync::Arc<std::sync::Mutex<usize>> = std::sync::Arc::default();
    let sink = delivered.clone();
    let summary = sql
      .stream_rows(
        &rows_request("events", None, vec![], None),
        Box::new(move |chunk| {
          *sink.lock().unwrap() += chunk.rows.len();
          // Receiver gone after the first chunk.
          false
        }),
      )
      .await
      .unwrap();
    assert!(summary.rows < 1000.0, "abort must stop the stream early");

    // The pooled conn went back with a half-read result set: the driver must
    // drain it before the next query.
    let after = sql.run_query("SELECT COUNT(*) FROM events").await.unwrap();
    assert_eq!(after.statements[0].rows[0][0].as_deref(), Some("1000"));
  }

  #[tokio::test]
  async fn integration_mysql_default_only_insert() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let applied = connection
      .sql()
      .unwrap()
      .apply_changes(&TableChanges {
        schema: "soquel_test".to_string(),
        table: "defaults_probe".to_string(),
        updates: vec![],
        deletes: vec![],
        inserts: vec![crate::connectors::RowInsert { values: vec![] }],
      })
      .await
      .unwrap();
    assert_eq!(applied.inserted, 1);
    let rows = connection
      .sql()
      .unwrap()
      .run_query("SELECT label FROM defaults_probe ORDER BY id DESC LIMIT 1")
      .await
      .unwrap();
    assert_eq!(rows.statements[0].rows[0][0].as_deref(), Some("fresh"));
  }

  #[tokio::test]
  async fn integration_mysql_stream_rows_chunks_and_streams_unlimited() {
    use std::sync::{Arc as StdArc, Mutex as StdMutex};

    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let sql = connection.sql().unwrap();

    let chunks: StdArc<StdMutex<Vec<RowsChunk>>> = StdArc::default();
    let sink = chunks.clone();
    let summary = sql
      .stream_rows(
        &rows_request("events", Some(500), vec![], None),
        Box::new(move |chunk| {
          sink.lock().unwrap().push(chunk);
          true
        }),
      )
      .await
      .unwrap();
    assert_eq!(summary.rows, 500.0);
    let chunks = chunks.lock().unwrap();
    assert_eq!(chunks.len(), 3, "500 rows in 200-row chunks");
    assert!(chunks[0].columns.is_some());
    assert!(chunks[1..].iter().all(|c| c.columns.is_none()));

    let total: StdArc<StdMutex<usize>> = StdArc::default();
    let sink = total.clone();
    let summary = sql
      .stream_rows(
        &rows_request("events", None, vec![], None),
        Box::new(move |chunk| {
          *sink.lock().unwrap() += chunk.rows.len();
          true
        }),
      )
      .await
      .unwrap();
    assert_eq!(summary.rows, 1000.0, "no limit streams the whole table");
    assert_eq!(*total.lock().unwrap(), 1000);
  }

  #[tokio::test]
  async fn integration_mysql_apply_changes_roundtrip_and_rollback() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let sql = connection.sql().unwrap();
    let changes = |updates, deletes, inserts| TableChanges {
      schema: "soquel_test".to_string(),
      table: "customers".to_string(),
      updates,
      deletes,
      inserts,
    };
    let cell = |column: &str, value: Option<&str>| crate::connectors::CellValue {
      column: column.to_string(),
      value: value.map(str::to_string),
    };

    let applied = sql
      .apply_changes(&changes(
        vec![],
        vec![],
        vec![crate::connectors::RowInsert {
          values: vec![
            cell("name", Some("Temp Row")),
            cell("email", Some("temp@example.com")),
          ],
        }],
      ))
      .await
      .unwrap();
    assert_eq!(applied.inserted, 1);

    let updated = sql
      .apply_changes(&changes(
        vec![crate::connectors::RowUpdate {
          key: vec![cell("email", Some("temp@example.com"))],
          set: vec![cell("name", Some("Temp Renamed")), cell("meta", None)],
        }],
        vec![],
        vec![],
      ))
      .await
      .unwrap();
    assert_eq!(updated.updated, 1);

    // Second update matches nothing: the whole batch must roll back.
    let result = sql
      .apply_changes(&changes(
        vec![
          crate::connectors::RowUpdate {
            key: vec![cell("email", Some("temp@example.com"))],
            set: vec![cell("name", Some("Should Not Stick"))],
          },
          crate::connectors::RowUpdate {
            key: vec![cell("email", Some("ghost@example.com"))],
            set: vec![cell("name", Some("x"))],
          },
        ],
        vec![],
        vec![],
      ))
      .await;
    let Err(Error::Database { message }) = result else {
      panic!("expected the batch to fail");
    };
    assert!(message.contains("changed or deleted"), "{message}");
    let check = sql
      .table_rows(&rows_request(
        "customers",
        Some(1),
        vec![text_filter(
          "email",
          crate::connectors::FilterOp::Eq,
          "temp@example.com",
        )],
        None,
      ))
      .await
      .unwrap();
    assert_eq!(
      check.statements[0].rows[0][1].as_deref(),
      Some("Temp Renamed")
    );

    // A key matching several rows trips the exactly-one guard too.
    let multi = sql
      .apply_changes(&TableChanges {
        schema: "soquel_test".to_string(),
        table: "events".to_string(),
        updates: vec![crate::connectors::RowUpdate {
          key: vec![cell("kind", Some("click"))],
          set: vec![cell("n", Some("0"))],
        }],
        deletes: vec![],
        inserts: vec![],
      })
      .await;
    let Err(Error::Database { message }) = multi else {
      panic!("expected the multi-row update to fail");
    };
    assert!(message.contains("instead of exactly 1"), "{message}");

    let deleted = sql
      .apply_changes(&changes(
        vec![],
        vec![crate::connectors::RowDelete {
          key: vec![cell("email", Some("temp@example.com"))],
        }],
        vec![],
      ))
      .await
      .unwrap();
    assert_eq!(deleted.deleted, 1);
  }

  #[tokio::test]
  async fn integration_mysql_schema_snapshot() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let snapshot = connection
      .introspect()
      .unwrap()
      .schema_snapshot()
      .await
      .unwrap();
    let schema = snapshot
      .schemas
      .iter()
      .find(|s| s.name == "soquel_test")
      .expect("seeded database present");

    let customers = schema
      .tables
      .iter()
      .find(|t| t.name == "customers")
      .unwrap();
    assert_eq!(customers.kind, TableKind::Table);
    assert_eq!(customers.primary_key, vec!["id"]);
    let id = customers.columns.iter().find(|c| c.name == "id").unwrap();
    assert_eq!(id.data_type, "int");
    assert_eq!(id.default.as_deref(), Some("auto_increment"));
    let email = customers
      .columns
      .iter()
      .find(|c| c.name == "email")
      .unwrap();
    assert!(email.nullable);
    assert_eq!(email.data_type, "varchar(255)");
    assert!(
      customers
        .indexes
        .iter()
        .any(|i| i.unique && i.definition.contains("email")),
      "{:?}",
      customers.indexes
    );

    let orders = schema.tables.iter().find(|t| t.name == "orders").unwrap();
    let fk = &orders.foreign_keys[0];
    assert_eq!(fk.columns, vec!["customer_id"]);
    assert_eq!(fk.referenced_schema, "soquel_test");
    assert_eq!(fk.referenced_table, "customers");
    assert_eq!(fk.referenced_columns, vec!["id"]);
    // The FK's auto-created index shows up as non-unique.
    assert!(
      orders
        .indexes
        .iter()
        .any(|i| !i.unique && i.definition.contains("customer_id")),
      "{:?}",
      orders.indexes
    );

    // Composite FK: both column pairs, in key order.
    let subscriptions = schema
      .tables
      .iter()
      .find(|t| t.name == "subscriptions")
      .unwrap();
    let composite = &subscriptions.foreign_keys[0];
    assert_eq!(composite.columns, vec!["org_id", "plan_code"]);
    assert_eq!(composite.referenced_table, "plans");
    assert_eq!(composite.referenced_columns, vec!["org_id", "code"]);

    // Databases group as schemas, alphabetical input order preserved.
    let other = snapshot
      .schemas
      .iter()
      .find(|s| s.name == "soquel_other")
      .expect("granted second database visible");
    assert!(other.tables.iter().any(|t| t.name == "notes"));

    let view = schema
      .tables
      .iter()
      .find(|t| t.name == "recent_orders")
      .unwrap();
    assert_eq!(view.kind, TableKind::View);
    assert!(!view.columns.is_empty());
  }

  #[tokio::test]
  async fn integration_mysql_table_ddl_via_show_create() {
    let Some(connection) = test_connection_from_env().await else {
      return;
    };
    let introspect = connection.introspect().unwrap();

    let ddl = introspect.table_ddl("soquel_test", "orders").await.unwrap();
    assert!(ddl.contains("CREATE TABLE `orders`"), "{ddl}");
    assert!(ddl.contains("PRIMARY KEY (`id`)"), "{ddl}");
    assert!(
      ddl.contains("FOREIGN KEY (`customer_id`) REFERENCES `customers` (`id`)"),
      "{ddl}"
    );

    let view = introspect
      .table_ddl("soquel_test", "recent_orders")
      .await
      .unwrap();
    assert!(view.contains("VIEW `recent_orders`"), "{view}");

    assert!(matches!(
      introspect.table_ddl("soquel_test", "nope").await,
      Err(Error::NotFound { .. })
    ));
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
