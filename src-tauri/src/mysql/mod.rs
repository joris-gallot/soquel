//! MySQL connector: mysql_async pool, text values rendered to strings.

use std::sync::OnceLock;
use std::time::Instant;

use mysql_async::consts::ColumnType;
use mysql_async::prelude::Queryable;
use mysql_async::{
  Column, Opts, OptsBuilder, Pool, PoolConstraints, PoolOpts, Row, SslOpts, Value,
};

use crate::connectors::{
  verify_exactly_one, ApplyResult, CancelRegistry, Capability, ColumnKind, Connection, Connector,
  Introspect, LocalForward, QueryColumn, QueryResult, RowsChunk, SqlQuery, SqlSession,
  StatementResult, StreamSummary, TableChanges, TableRowsRequest, CHUNK_ROWS, POOL_MAX_SIZE,
};
use crate::error::Error;
use crate::profiles::{ConnectionProfile, SqlServerParams, SslMode};

mod browse;
mod introspect;

use browse::{build_change_statements, build_select, ChangeKind, ChangeStatement};

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
    let params = profile
      .params
      .sql_server()
      .ok_or_else(|| Error::Unsupported {
        message: "this connector needs a TCP SQL server profile".to_string(),
      })?;
    let opts = build_opts(params, secret, forward, ssl_opts(params, forward));
    let connection = match MysqlConnection::open(opts).await {
      Ok(connection) => connection,
      // libpq-style prefer: retry in plaintext when the TLS attempt fails.
      Err(_) if params.ssl_mode == SslMode::Prefer => {
        let opts = build_opts(params, secret, forward, None);
        MysqlConnection::open(opts).await?
      }
      Err(err) => return Err(err),
    };
    Ok(Box::new(connection))
  }
}

fn build_opts(
  params: &SqlServerParams,
  secret: Option<&str>,
  forward: Option<LocalForward>,
  ssl: Option<SslOpts>,
) -> Opts {
  let (host, port) = match forward {
    Some(forward) => ("127.0.0.1".to_string(), forward.port),
    None => (params.host.clone(), params.port),
  };
  OptsBuilder::default()
    .ip_or_hostname(host)
    .tcp_port(port)
    .db_name(Some(params.database.clone()))
    .user(Some(params.user.clone()))
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
fn ssl_opts(params: &SqlServerParams, forward: Option<LocalForward>) -> Option<SslOpts> {
  match params.ssl_mode {
    SslMode::Disable => None,
    // Encrypt without verifying, like the postgres AcceptAll verifier.
    SslMode::Prefer | SslMode::Require => Some(
      SslOpts::default()
        .with_danger_accept_invalid_certs(true)
        .with_danger_skip_domain_validation(true),
    ),
    SslMode::VerifyFull => {
      let mut ssl = SslOpts::default();
      if let Some(path) = &params.ssl_root_cert {
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
  /// Thread ids of in-flight queries, for KILL QUERY on a side connection.
  active_threads: CancelRegistry<u32>,
}

impl MysqlConnection {
  async fn open(opts: Opts) -> Result<Self, Error> {
    let pool = Pool::new(opts.clone());
    let connection = Self {
      pool,
      opts,
      server_version: OnceLock::new(),
      active_threads: CancelRegistry::default(),
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
    let _guard = self.active_threads.register(conn.id());
    run_script(&mut conn, sql).await
  }

  async fn run_read_only_query(&self, sql: &str) -> Result<QueryResult, Error> {
    // READ ONLY transactions block DML, but DDL implicit-commits right out of
    // them: only read statement heads may pass.
    read_statement_guard(sql)?;
    let mut conn = self.pool.get_conn().await?;
    let _guard = self.active_threads.register(conn.id());
    conn.query_drop("START TRANSACTION READ ONLY").await?;
    let result = run_script(&mut conn, sql).await;
    let rollback = conn.query_drop("ROLLBACK").await;
    let result = result?;
    rollback?;
    Ok(result)
  }

  async fn cancel(&self) -> Result<(), Error> {
    let mut conn = self.pool.get_conn().await?;
    for thread in self.active_threads.tokens() {
      conn.query_drop(format!("KILL QUERY {thread}")).await?;
    }
    Ok(())
  }

  async fn table_rows(&self, request: &TableRowsRequest) -> Result<QueryResult, Error> {
    let mut conn = self.pool.get_conn().await?;
    let _guard = self.active_threads.register(conn.id());
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
    let _guard = self.active_threads.register(conn.id());
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
        verify_exactly_one(
          if statement.kind == ChangeKind::Update {
            "row update"
          } else {
            "row delete"
          },
          affected,
        )?;
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

/// First significant keyword allowlist for the agent read-only surface.
fn read_statement_guard(sql: &str) -> Result<(), Error> {
  const READ_HEADS: [&str; 11] = [
    "SELECT", "WITH", "SHOW", "EXPLAIN", "DESCRIBE", "DESC", "TABLE", "VALUES", "CHECKSUM", "HELP",
    "DO",
  ];
  let head = statement_head(sql);
  if READ_HEADS.iter().any(|h| head.eq_ignore_ascii_case(h)) {
    return Ok(());
  }
  Err(Error::Unsupported {
    message: format!("only read statements are allowed for agents (got `{head}`)"),
  })
}

/// First keyword of a statement, past whitespace, comments and opening parens.
fn statement_head(sql: &str) -> String {
  let mut rest = sql;
  loop {
    rest = rest.trim_start();
    if let Some(stripped) = rest.strip_prefix("--").or_else(|| rest.strip_prefix('#')) {
      rest = stripped.split_once('\n').map_or("", |(_, tail)| tail);
    } else if let Some(stripped) = rest.strip_prefix("/*") {
      rest = stripped.split_once("*/").map_or("", |(_, tail)| tail);
    } else if let Some(stripped) = rest.strip_prefix('(') {
      rest = stripped;
    } else {
      break;
    }
  }
  rest
    .chars()
    .take_while(|c| c.is_ascii_alphabetic())
    .collect()
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
mod tests;

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
