use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deadpool::managed::{self, Metrics, Object, Pool, RecycleError, RecycleResult};
use tokio_postgres::types::{Kind as PgKind, Type as PgType};
use tokio_postgres::{AsyncMessage, CancelToken, Client, Config, SimpleQueryMessage};

use crate::connectors::{
  ApplyResult, Capability, CellValue, ColumnFilter, ColumnKind, Connection, Connector, FilterOp,
  Introspect, LocalForward, QueryColumn, QueryResult, RowsChunk, ServerNotice, SortDirection,
  SqlQuery, SqlSession, StatementResult, StreamSummary, TableChanges, TableRowsRequest,
};
use crate::error::Error;
use crate::profiles::{ConnectionProfile, SqlServerParams, SslMode};

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
    forward: Option<LocalForward>,
  ) -> Result<Box<dyn Connection>, Error> {
    let params = profile.params.sql_server();
    let mut config = build_config(params, forward);
    if let Some(secret) = secret {
      config.password(secret);
    }
    let connection =
      PostgresConnection::new(config, params.ssl_mode, params.ssl_root_cert.clone())?;
    // Surface auth/reachability/TLS errors now, not on the first query.
    drop(connection.checkout().await?);
    Ok(Box::new(connection))
  }
}

/// Through a tunnel, TCP dials the local forward (`hostaddr`) while `host`
/// stays the logical hostname: TLS SNI and verify-full target the real server,
/// not 127.0.0.1.
fn build_config(params: &SqlServerParams, forward: Option<LocalForward>) -> Config {
  let mut config = Config::new();
  config.host(&params.host);
  match forward {
    Some(forward) => {
      config.hostaddr(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
      config.port(forward.port);
    }
    None => {
      config.port(params.port);
    }
  }
  config
    .dbname(&params.database)
    .user(&params.user)
    .application_name("soquel")
    .ssl_mode(tls::config_ssl_mode(params.ssl_mode))
    .connect_timeout(Duration::from_secs(10));
  config
}

pub(super) struct PooledPg {
  pub(super) client: Client,
  notices: Arc<Mutex<Vec<ServerNotice>>>,
  server_version: Option<String>,
}

pub(super) struct PgManager {
  config: Config,
  ssl_mode: SslMode,
  ssl_root_cert: Option<String>,
  server_version: Arc<std::sync::OnceLock<String>>,
}

async fn connect_pg(
  config: &Config,
  ssl_mode: SslMode,
  ssl_root_cert: Option<&str>,
) -> Result<PooledPg, Error> {
  let tls = tls::connector(ssl_mode, ssl_root_cert)?;
  let (client, mut connection) = config.connect(tls).await?;
  let server_version = connection.parameter("server_version").map(str::to_string);
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
  Ok(PooledPg {
    client,
    notices,
    server_version,
  })
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
    let pg = connect_pg(&self.config, self.ssl_mode, self.ssl_root_cert.as_deref()).await?;
    if let Some(version) = &pg.server_version {
      let _ = self.server_version.set(version.clone());
    }
    Ok(pg)
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
  ssl_root_cert: Option<String>,
  server_version: Arc<std::sync::OnceLock<String>>,
  cancels: Mutex<HashMap<u64, CancelToken>>,
  next_cancel_id: AtomicU64,
}

impl PostgresConnection {
  fn new(config: Config, ssl_mode: SslMode, ssl_root_cert: Option<String>) -> Result<Self, Error> {
    let server_version = Arc::new(std::sync::OnceLock::new());
    let pool = Pool::builder(PgManager {
      config,
      ssl_mode,
      ssl_root_cert: ssl_root_cert.clone(),
      server_version: server_version.clone(),
    })
    .max_size(POOL_MAX_SIZE)
    .build()
    .map_err(|err| Error::Database {
      message: format!("connection pool: {err}"),
    })?;
    Ok(Self {
      pool,
      ssl_mode,
      ssl_root_cert,
      server_version,
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
  ssl_root_cert: Option<String>,
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
      .cancel_query(tls::connector(
        self.ssl_mode,
        self.ssl_root_cert.as_deref(),
      )?)
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
impl SqlQuery for PostgresConnection {
  async fn run_query(&self, sql: &str) -> Result<QueryResult, Error> {
    self.execute_script(sql).await
  }

  async fn cancel(&self) -> Result<(), Error> {
    let tokens: Vec<CancelToken> = self.cancels.lock().unwrap().values().cloned().collect();
    for token in tokens {
      token
        .cancel_query(tls::connector(
          self.ssl_mode,
          self.ssl_root_cert.as_deref(),
        )?)
        .await?;
    }
    Ok(())
  }

  async fn table_rows(&self, request: &TableRowsRequest) -> Result<QueryResult, Error> {
    let pg = self.checkout().await?;
    let _guard = self.register_cancel(pg.client.cancel_token());
    pg.notices.lock().unwrap().clear();
    let start = Instant::now();

    let plan = plan_select(&pg, request).await?;
    let bind = bind_text(&plan.params);
    let rows = pg.client.query_typed(&plan.sql, &bind).await?;

    let statement = StatementResult {
      columns: plan.columns,
      rows_affected: rows.len() as f64,
      rows: rows
        .iter()
        .map(|row| (0..row.len()).map(|i| row.get(i)).collect())
        .collect(),
    };
    let notices = std::mem::take(&mut *pg.notices.lock().unwrap());
    Ok(QueryResult {
      statements: vec![statement],
      notices,
      duration_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
  }

  async fn stream_rows(
    &self,
    request: &TableRowsRequest,
    on_chunk: Box<dyn Fn(RowsChunk) -> bool + Send>,
  ) -> Result<StreamSummary, Error> {
    use futures_util::TryStreamExt;

    let pg = self.checkout().await?;
    let _guard = self.register_cancel(pg.client.cancel_token());
    pg.notices.lock().unwrap().clear();
    let start = Instant::now();

    let plan = plan_select(&pg, request).await?;
    let params = plan.params.iter().map(|value| (value, PgType::TEXT));
    let stream = pg.client.query_typed_raw(&plan.sql, params).await?;
    futures_util::pin_mut!(stream);

    let mut columns = Some(plan.columns);
    let mut total = 0u64;
    let mut chunk: Vec<Vec<Option<String>>> = Vec::with_capacity(CHUNK_ROWS);
    while let Some(row) = stream.try_next().await? {
      chunk.push((0..row.len()).map(|i| row.get(i)).collect());
      total += 1;
      if chunk.len() == CHUNK_ROWS
        && !on_chunk(RowsChunk {
          columns: columns.take(),
          rows: std::mem::take(&mut chunk),
        })
      {
        // Receiver gone: stop reading; dropping the stream discards the rest.
        break;
      }
    }
    if columns.is_some() || !chunk.is_empty() {
      on_chunk(RowsChunk {
        columns: columns.take(),
        rows: std::mem::take(&mut chunk),
      });
    }

    let notices = std::mem::take(&mut *pg.notices.lock().unwrap());
    Ok(StreamSummary {
      rows: total as f64,
      duration_ms: start.elapsed().as_secs_f64() * 1000.0,
      notices,
    })
  }

  async fn apply_changes(&self, changes: &TableChanges) -> Result<ApplyResult, Error> {
    let pg = self.checkout().await?;
    let base = format!(
      "SELECT * FROM {}.{}",
      quote_ident(&changes.schema),
      quote_ident(&changes.table)
    );
    let prepared = pg.client.prepare(&base).await?;
    let columns: Vec<(String, PgType)> = prepared
      .columns()
      .iter()
      .map(|c| (c.name().to_string(), c.type_().clone()))
      .collect();
    let statements = build_change_statements(&changes.schema, &changes.table, &columns, changes)?;
    if statements.is_empty() {
      return Err(Error::Unsupported {
        message: "no changes to apply".to_string(),
      });
    }

    let start = Instant::now();
    pg.client.batch_execute("BEGIN").await?;
    match run_change_statements(&pg, &statements).await {
      Ok((updated, inserted, deleted)) => {
        pg.client.batch_execute("COMMIT").await?;
        Ok(ApplyResult {
          updated,
          inserted,
          deleted,
          duration_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
      }
      Err(err) => {
        // The pooled client must go back clean.
        let _ = pg.client.batch_execute("ROLLBACK").await;
        Err(err)
      }
    }
  }

  async fn open_session(&self) -> Result<Box<dyn SqlSession>, Error> {
    let manager = self.pool.manager();
    let pg = connect_pg(
      &manager.config,
      manager.ssl_mode,
      manager.ssl_root_cert.as_deref(),
    )
    .await?;
    Ok(Box::new(PostgresSession {
      cancel: pg.client.cancel_token(),
      ssl_mode: manager.ssl_mode,
      ssl_root_cert: manager.ssl_root_cert.clone(),
      pg,
    }))
  }
}

// Identifiers come from the UI: quoting is the injection boundary.
fn quote_ident(ident: &str) -> String {
  format!("\"{}\"", ident.replace('"', "\"\""))
}

const CHUNK_ROWS: usize = 200;
const MAX_FETCH_ROWS: u32 = 5000;

struct SelectPlan {
  sql: String,
  params: Vec<String>,
  columns: Vec<QueryColumn>,
}

/// Shared by the collected and streamed paths. The prepared column list is the
/// only source of column identity: filters and sort must name one of these, so
/// no frontend string reaches SQL unquoted.
async fn plan_select(pg: &PooledPg, request: &TableRowsRequest) -> Result<SelectPlan, Error> {
  let base = format!(
    "SELECT * FROM {}.{}",
    quote_ident(&request.schema),
    quote_ident(&request.table)
  );
  let prepared = pg.client.prepare(&base).await?;
  let columns: Vec<(String, PgType)> = prepared
    .columns()
    .iter()
    .map(|c| (c.name().to_string(), c.type_().clone()))
    .collect();

  let (where_clause, params) = build_where(&columns, &request.filters)?;

  // ::text keeps the server's canonical formatting (arrays, timestamps, bytea)
  // while the extended protocol carries the filter parameters.
  let mut projection = columns
    .iter()
    .map(|(name, _)| {
      let ident = quote_ident(name);
      format!("{ident}::text AS {ident}")
    })
    .collect::<Vec<_>>()
    .join(", ");
  if request.include_xmin {
    projection = format!("\"xmin\"::text AS \"xmin\", {projection}");
  }
  if request.include_ctid {
    projection = format!("\"ctid\"::text AS \"ctid\", {projection}");
  }
  let mut sql = format!(
    "SELECT {projection} FROM {}.{}{where_clause}",
    quote_ident(&request.schema),
    quote_ident(&request.table)
  );
  if let Some(sort) = &request.sort {
    if !columns.iter().any(|(name, _)| name == &sort.column) {
      return Err(Error::Unsupported {
        message: format!("unknown column {}", sort.column),
      });
    }
    let direction = match sort.direction {
      SortDirection::Asc => "ASC",
      SortDirection::Desc => "DESC",
    };
    // Qualified: a bare name would resolve to the ::text output alias and sort
    // lexicographically.
    sql.push_str(&format!(
      " ORDER BY {}.{}.{} {direction}",
      quote_ident(&request.schema),
      quote_ident(&request.table),
      quote_ident(&sort.column)
    ));
  }
  if let Some(limit) = request.limit {
    sql.push_str(&format!(" LIMIT {}", limit.min(MAX_FETCH_ROWS)));
  }
  sql.push_str(&format!(" OFFSET {}", request.offset));

  let mut result_columns: Vec<QueryColumn> = columns
    .iter()
    .map(|(name, ty)| QueryColumn {
      name: name.clone(),
      data_type: Some(type_name(ty)),
      kind: column_kind(ty),
    })
    .collect();
  if request.include_ctid {
    result_columns.insert(
      0,
      QueryColumn {
        name: "ctid".to_string(),
        data_type: Some("tid".to_string()),
        kind: ColumnKind::Other,
      },
    );
  }
  if request.include_xmin {
    let position = usize::from(request.include_ctid);
    result_columns.insert(
      position,
      QueryColumn {
        name: "xmin".to_string(),
        data_type: Some("xid".to_string()),
        kind: ColumnKind::Other,
      },
    );
  }
  Ok(SelectPlan {
    sql,
    params,
    columns: result_columns,
  })
}

// Every parameter travels as text; build_where casts it to the column's type.
fn bind_text(params: &[String]) -> Vec<(&(dyn tokio_postgres::types::ToSql + Sync), PgType)> {
  params
    .iter()
    .map(|value| {
      (
        value as &(dyn tokio_postgres::types::ToSql + Sync),
        PgType::TEXT,
      )
    })
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeKind {
  Update,
  Insert,
  Delete,
}

#[derive(Debug)]
struct ChangeStatement {
  sql: String,
  params: Vec<Option<String>>,
  kind: ChangeKind,
}

/// Fixed order: updates, then deletes, then inserts.
fn build_change_statements(
  schema: &str,
  table: &str,
  columns: &[(String, PgType)],
  changes: &TableChanges,
) -> Result<Vec<ChangeStatement>, Error> {
  let target = format!("{}.{}", quote_ident(schema), quote_ident(table));
  let mut statements = Vec::new();

  for update in &changes.updates {
    if update.set.is_empty() || update.key.is_empty() {
      return Err(Error::Unsupported {
        message: "an update needs at least one changed cell and a key".to_string(),
      });
    }
    let mut params = Vec::new();
    let mut sets = Vec::new();
    for cell in &update.set {
      let cast = change_cast(columns, &cell.column)?;
      params.push(cell.value.clone());
      sets.push(format!(
        "{} = ${}::{cast}",
        quote_ident(&cell.column),
        params.len()
      ));
    }
    let key = key_clause(columns, &update.key, &mut params)?;
    statements.push(ChangeStatement {
      sql: format!("UPDATE {target} SET {} WHERE {key}", sets.join(", ")),
      params,
      kind: ChangeKind::Update,
    });
  }

  for delete in &changes.deletes {
    if delete.key.is_empty() {
      return Err(Error::Unsupported {
        message: "a delete needs a key".to_string(),
      });
    }
    let mut params = Vec::new();
    let key = key_clause(columns, &delete.key, &mut params)?;
    statements.push(ChangeStatement {
      sql: format!("DELETE FROM {target} WHERE {key}"),
      params,
      kind: ChangeKind::Delete,
    });
  }

  for insert in &changes.inserts {
    let mut params = Vec::new();
    let statement = if insert.values.is_empty() {
      format!("INSERT INTO {target} DEFAULT VALUES")
    } else {
      let mut names = Vec::new();
      let mut values = Vec::new();
      for cell in &insert.values {
        let cast = change_cast(columns, &cell.column)?;
        params.push(cell.value.clone());
        names.push(quote_ident(&cell.column));
        values.push(format!("${}::{cast}", params.len()));
      }
      format!(
        "INSERT INTO {target} ({}) VALUES ({})",
        names.join(", "),
        values.join(", ")
      )
    };
    statements.push(ChangeStatement {
      sql: statement,
      params,
      kind: ChangeKind::Insert,
    });
  }

  Ok(statements)
}

// System columns are absent from the prepared list but valid as keys:
// ctid for PK-less tables, xmin as the optimistic-lock guard.
fn change_cast(columns: &[(String, PgType)], column: &str) -> Result<String, Error> {
  if column == "ctid" {
    return Ok("tid".to_string());
  }
  if column == "xmin" {
    return Ok("xid".to_string());
  }
  columns
    .iter()
    .find(|(name, _)| name == column)
    .map(|(_, ty)| type_name(ty))
    .ok_or_else(|| Error::Unsupported {
      message: format!("unknown column {column}"),
    })
}

/// NULL-safe key comparison: PK values are never NULL, but ctid-less tables
/// may key on nullable columns.
fn key_clause(
  columns: &[(String, PgType)],
  key: &[CellValue],
  params: &mut Vec<Option<String>>,
) -> Result<String, Error> {
  let mut clauses = Vec::new();
  for cell in key {
    let cast = change_cast(columns, &cell.column)?;
    params.push(cell.value.clone());
    clauses.push(format!(
      "{} IS NOT DISTINCT FROM ${}::{cast}",
      quote_ident(&cell.column),
      params.len()
    ));
  }
  Ok(clauses.join(" AND "))
}

async fn run_change_statements(
  pg: &PooledPg,
  statements: &[ChangeStatement],
) -> Result<(u32, u32, u32), Error> {
  use futures_util::TryStreamExt;

  let (mut updated, mut inserted, mut deleted) = (0u32, 0u32, 0u32);
  for statement in statements {
    let params = statement.params.iter().map(|value| (value, PgType::TEXT));
    let stream = pg.client.query_typed_raw(&statement.sql, params).await?;
    futures_util::pin_mut!(stream);
    while stream.try_next().await?.is_some() {}
    let affected = stream.rows_affected().unwrap_or(0);
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

/// AND-ed WHERE clause + bound parameter values, `$1`-numbered in order.
fn build_where(
  columns: &[(String, PgType)],
  filters: &[ColumnFilter],
) -> Result<(String, Vec<String>), Error> {
  let mut clauses = Vec::new();
  let mut params: Vec<String> = Vec::new();
  for filter in filters {
    if !columns.iter().any(|(name, _)| name == &filter.column) {
      return Err(Error::Unsupported {
        message: format!("unknown column {}", filter.column),
      });
    }
    let ident = quote_ident(&filter.column);
    // Parameters are declared text on the wire: comparisons cast them to the
    // column's type so postgres compares values, not strings.
    let cast = columns
      .iter()
      .find(|(name, _)| name == &filter.column)
      .map(|(_, ty)| type_name(ty))
      .unwrap_or_default();
    let clause = match filter.op {
      FilterOp::IsNull => format!("{ident} IS NULL"),
      FilterOp::IsNotNull => format!("{ident} IS NOT NULL"),
      op => {
        let value = filter.value.clone().ok_or_else(|| Error::Unsupported {
          message: format!("filter on {} requires a value", filter.column),
        })?;
        let n = params.len() + 1;
        match op {
          FilterOp::Contains => {
            params.push(format!("%{}%", escape_like(&value)));
            format!("{ident}::text ILIKE ${n}")
          }
          FilterOp::StartsWith => {
            params.push(format!("{}%", escape_like(&value)));
            format!("{ident}::text ILIKE ${n}")
          }
          _ => {
            params.push(value);
            format!("{ident} {} ${n}::{cast}", comparison_operator(op))
          }
        }
      }
    };
    clauses.push(clause);
  }
  let clause = if clauses.is_empty() {
    String::new()
  } else {
    format!(" WHERE {}", clauses.join(" AND "))
  };
  Ok((clause, params))
}

fn comparison_operator(op: FilterOp) -> &'static str {
  match op {
    FilterOp::Eq => "=",
    FilterOp::Neq => "<>",
    FilterOp::Lt => "<",
    FilterOp::Lte => "<=",
    FilterOp::Gt => ">",
    FilterOp::Gte => ">=",
    FilterOp::Contains | FilterOp::StartsWith | FilterOp::IsNull | FilterOp::IsNotNull => {
      unreachable!("handled before reaching the comparison branch")
    }
  }
}

// User values are literals: LIKE metacharacters must not act as wildcards.
fn escape_like(value: &str) -> String {
  value
    .replace('\\', "\\\\")
    .replace('%', "\\%")
    .replace('_', "\\_")
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

  // Convention: integration_* tests run via `pnpm test:integration` (needs `pnpm db:test`),
  // each gated by its connector's env var, and skip silently otherwise.
  #[test]
  fn quote_ident_escapes_quotes() {
    assert_eq!(quote_ident("customers"), "\"customers\"");
    assert_eq!(quote_ident("MiXeD"), "\"MiXeD\"");
    assert_eq!(quote_ident("evil\"; DROP--"), "\"evil\"\"; DROP--\"");
  }

  fn text_columns(names: &[&str]) -> Vec<(String, PgType)> {
    names
      .iter()
      .map(|name| (name.to_string(), PgType::TEXT))
      .collect()
  }

  fn filter(column: &str, op: FilterOp, value: Option<&str>) -> ColumnFilter {
    ColumnFilter {
      column: column.to_string(),
      op,
      value: value.map(str::to_string),
    }
  }

  #[test]
  fn build_where_covers_every_operator() {
    let columns = text_columns(&["name", "amount"]);
    let cases: [(FilterOp, Option<&str>, &str, Option<&str>); 10] = [
      (
        FilterOp::Eq,
        Some("x"),
        r#" WHERE "name" = $1::text"#,
        Some("x"),
      ),
      (
        FilterOp::Neq,
        Some("x"),
        r#" WHERE "name" <> $1::text"#,
        Some("x"),
      ),
      (
        FilterOp::Lt,
        Some("5"),
        r#" WHERE "name" < $1::text"#,
        Some("5"),
      ),
      (
        FilterOp::Lte,
        Some("5"),
        r#" WHERE "name" <= $1::text"#,
        Some("5"),
      ),
      (
        FilterOp::Gt,
        Some("5"),
        r#" WHERE "name" > $1::text"#,
        Some("5"),
      ),
      (
        FilterOp::Gte,
        Some("5"),
        r#" WHERE "name" >= $1::text"#,
        Some("5"),
      ),
      (
        FilterOp::Contains,
        Some("ada"),
        r#" WHERE "name"::text ILIKE $1"#,
        Some("%ada%"),
      ),
      (
        FilterOp::StartsWith,
        Some("ada"),
        r#" WHERE "name"::text ILIKE $1"#,
        Some("ada%"),
      ),
      (FilterOp::IsNull, None, r#" WHERE "name" IS NULL"#, None),
      (
        FilterOp::IsNotNull,
        None,
        r#" WHERE "name" IS NOT NULL"#,
        None,
      ),
    ];
    for (op, value, clause, param) in cases {
      let (built, params) = build_where(&columns, &[filter("name", op, value)]).unwrap();
      assert_eq!(built, clause, "{op:?}");
      assert_eq!(
        params,
        param.map(str::to_string).into_iter().collect::<Vec<_>>()
      );
    }
  }

  #[test]
  fn build_where_numbers_params_and_ands_clauses() {
    let columns = text_columns(&["name", "email", "amount"]);
    let (clause, params) = build_where(
      &columns,
      &[
        filter("name", FilterOp::Contains, Some("a")),
        filter("email", FilterOp::IsNotNull, None),
        filter("amount", FilterOp::Gt, Some("10")),
      ],
    )
    .unwrap();
    assert_eq!(
      clause,
      r#" WHERE "name"::text ILIKE $1 AND "email" IS NOT NULL AND "amount" > $2::text"#
    );
    assert_eq!(params, vec!["%a%".to_string(), "10".to_string()]);
  }

  #[test]
  fn build_where_rejects_unknown_columns_and_missing_values() {
    let columns = text_columns(&["name"]);
    assert!(matches!(
      build_where(&columns, &[filter("nope", FilterOp::Eq, Some("x"))]),
      Err(Error::Unsupported { .. })
    ));
    assert!(matches!(
      build_where(&columns, &[filter("name", FilterOp::Eq, None)]),
      Err(Error::Unsupported { .. })
    ));
  }

  #[test]
  fn build_where_quotes_hostile_idents_and_escapes_like() {
    let columns = vec![("evil\"; DROP--".to_string(), PgType::TEXT)];
    let (clause, params) = build_where(
      &columns,
      &[filter("evil\"; DROP--", FilterOp::Contains, Some("50%_\\"))],
    )
    .unwrap();
    assert_eq!(clause, r#" WHERE "evil""; DROP--"::text ILIKE $1"#);
    assert_eq!(params, vec!["%50\\%\\_\\\\%".to_string()]);
  }

  fn cell(column: &str, value: Option<&str>) -> CellValue {
    CellValue {
      column: column.to_string(),
      value: value.map(str::to_string),
    }
  }

  fn no_changes() -> TableChanges {
    TableChanges {
      schema: "app".to_string(),
      table: "customers".to_string(),
      updates: vec![],
      inserts: vec![],
      deletes: vec![],
    }
  }

  #[test]
  fn change_statements_cover_update_delete_insert() {
    let columns = vec![
      ("id".to_string(), PgType::INT4),
      ("name".to_string(), PgType::TEXT),
      ("meta".to_string(), PgType::JSONB),
    ];
    let changes = TableChanges {
      updates: vec![crate::connectors::RowUpdate {
        key: vec![cell("id", Some("1"))],
        set: vec![cell("name", Some("Ada")), cell("meta", None)],
      }],
      deletes: vec![crate::connectors::RowDelete {
        key: vec![cell("id", Some("2"))],
      }],
      inserts: vec![
        crate::connectors::RowInsert {
          values: vec![cell("name", Some("New"))],
        },
        crate::connectors::RowInsert { values: vec![] },
      ],
      ..no_changes()
    };
    let statements = build_change_statements("app", "customers", &columns, &changes).unwrap();
    assert_eq!(statements.len(), 4);

    assert_eq!(
      statements[0].sql,
      r#"UPDATE "app"."customers" SET "name" = $1::text, "meta" = $2::jsonb WHERE "id" IS NOT DISTINCT FROM $3::int4"#
    );
    assert_eq!(
      statements[0].params,
      vec![Some("Ada".to_string()), None, Some("1".to_string())]
    );
    assert_eq!(statements[0].kind, ChangeKind::Update);

    assert_eq!(
      statements[1].sql,
      r#"DELETE FROM "app"."customers" WHERE "id" IS NOT DISTINCT FROM $1::int4"#
    );
    assert_eq!(statements[1].kind, ChangeKind::Delete);

    assert_eq!(
      statements[2].sql,
      r#"INSERT INTO "app"."customers" ("name") VALUES ($1::text)"#
    );
    assert_eq!(
      statements[3].sql,
      r#"INSERT INTO "app"."customers" DEFAULT VALUES"#
    );
  }

  #[test]
  fn change_statements_reject_unknown_columns_and_empty_shapes() {
    let columns = vec![("id".to_string(), PgType::INT4)];
    let unknown = TableChanges {
      updates: vec![crate::connectors::RowUpdate {
        key: vec![cell("id", Some("1"))],
        set: vec![cell("nope", Some("x"))],
      }],
      ..no_changes()
    };
    assert!(matches!(
      build_change_statements("app", "customers", &columns, &unknown),
      Err(Error::Unsupported { .. })
    ));

    let empty_key = TableChanges {
      deletes: vec![crate::connectors::RowDelete { key: vec![] }],
      ..no_changes()
    };
    assert!(matches!(
      build_change_statements("app", "customers", &columns, &empty_key),
      Err(Error::Unsupported { .. })
    ));
  }

  #[test]
  fn change_statements_allow_ctid_keys_and_quote_hostile_idents() {
    let columns = vec![("evil\"; DROP--".to_string(), PgType::TEXT)];
    let changes = TableChanges {
      updates: vec![crate::connectors::RowUpdate {
        key: vec![cell("ctid", Some("(0,1)"))],
        set: vec![cell("evil\"; DROP--", Some("x"))],
      }],
      ..no_changes()
    };
    let statements = build_change_statements("app", "t", &columns, &changes).unwrap();
    assert_eq!(
      statements[0].sql,
      r#"UPDATE "app"."t" SET "evil""; DROP--" = $1::text WHERE "ctid" IS NOT DISTINCT FROM $2::tid"#
    );
  }

  #[test]
  fn change_statements_key_on_xmin_as_xid() {
    let columns = vec![
      ("id".to_string(), PgType::INT4),
      ("name".to_string(), PgType::TEXT),
    ];
    let changes = TableChanges {
      updates: vec![crate::connectors::RowUpdate {
        key: vec![cell("id", Some("1")), cell("xmin", Some("12345"))],
        set: vec![cell("name", Some("Ada"))],
      }],
      ..no_changes()
    };
    let statements = build_change_statements("app", "customers", &columns, &changes).unwrap();
    assert_eq!(
      statements[0].sql,
      r#"UPDATE "app"."customers" SET "name" = $1::text WHERE "id" IS NOT DISTINCT FROM $2::int4 AND "xmin" IS NOT DISTINCT FROM $3::xid"#
    );
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

  #[test]
  fn build_config_keeps_logical_host_behind_a_forward() {
    use tokio_postgres::config::Host;

    let params = SqlServerParams {
      host: "db.internal".to_string(),
      port: 5432,
      database: "app".to_string(),
      user: "u".to_string(),
      ssl_mode: SslMode::VerifyFull,
      ssl_root_cert: None,
      tunnel_id: None,
    };

    // Tunneled: TCP goes to the forward, TLS still targets db.internal.
    let config = build_config(&params, Some(LocalForward { port: 6000 }));
    assert_eq!(config.get_hosts(), &[Host::Tcp("db.internal".to_string())]);
    assert_eq!(
      config.get_hostaddrs(),
      &[std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)]
    );
    assert_eq!(config.get_ports(), &[6000]);

    let direct = build_config(&params, None);
    assert!(direct.get_hostaddrs().is_empty());
    assert_eq!(direct.get_ports(), &[5432]);
  }

  fn connection_from_url(url: &str, ssl_mode: SslMode) -> PostgresConnection {
    let mut config: Config = url.parse().unwrap();
    config.ssl_mode(tls::config_ssl_mode(ssl_mode));
    PostgresConnection::new(config, ssl_mode, None).unwrap()
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
  async fn integration_postgres_tls_require_accepts_self_signed() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG_TLS") else {
      return;
    };
    // require encrypts without verifying: the throwaway cert must pass.
    let pg = connection_from_url(&url, SslMode::Require);
    let result = pg
      .run_query("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
      .await
      .unwrap();
    assert_eq!(
      result.statements[0].rows[0][0].as_deref(),
      Some("t"),
      "session must actually be TLS"
    );
  }

  #[tokio::test]
  async fn integration_postgres_tls_verify_full_rejects_self_signed() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG_TLS") else {
      return;
    };
    let pg = connection_from_url(&url, SslMode::VerifyFull);
    let Err(Error::Database { message }) = pg.health().await else {
      panic!("expected verify-full to reject an untrusted certificate");
    };
    assert!(!message.is_empty());
  }

  // Throwaway CA that signed the server cert (SAN localhost/127.0.0.1).
  pub(crate) const TEST_ROOT_CERT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/test-tls/ca.crt");

  #[tokio::test]
  async fn integration_postgres_tls_verify_full_passes_with_root_cert() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG_TLS") else {
      return;
    };
    let mut config: Config = url.parse().unwrap();
    config.ssl_mode(tls::config_ssl_mode(SslMode::VerifyFull));
    // The compose URL points at localhost, which the cert SAN carries.
    let pg = PostgresConnection::new(
      config,
      SslMode::VerifyFull,
      Some(TEST_ROOT_CERT.to_string()),
    )
    .unwrap();
    pg.health().await.unwrap();
  }

  #[tokio::test]
  async fn integration_postgres_server_version_is_captured() {
    use crate::connectors::Connection;

    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    // Captured lazily with the first pooled connection.
    pg.health().await.unwrap();
    let version = pg.server_version().expect("version after first checkout");
    assert!(
      version.starts_with(|c: char| c.is_ascii_digit()),
      "{version}"
    );
  }

  #[tokio::test]
  async fn integration_postgres_pool_recycles_terminated_connection() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let result = pg.run_query("SELECT pg_backend_pid()").await.unwrap();
    let pid = result.statements[0].rows[0][0].clone().unwrap();

    // Kill the idle pooled backend from a second, out-of-pool connection.
    let killer = test_connection_from_env().await.unwrap();
    let killed = killer
      .run_query(&format!("SELECT pg_terminate_backend({pid})"))
      .await
      .unwrap();
    assert_eq!(killed.statements[0].rows[0][0].as_deref(), Some("t"));

    // The client task needs a moment to observe the EOF before is_closed
    // trips; recycle must then cull the dead object and hand out a fresh one.
    let mut fresh_pid = None;
    for _ in 0..50 {
      tokio::time::sleep(Duration::from_millis(100)).await;
      if let Ok(result) = pg.run_query("SELECT pg_backend_pid()").await {
        fresh_pid = Some(result.statements[0].rows[0][0].clone().unwrap());
        break;
      }
    }
    let fresh_pid = fresh_pid.expect("pool never recovered from a terminated backend");
    assert_ne!(fresh_pid, pid);
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
        limit: Some(2),
        offset: 0,
        sort: Some(crate::connectors::SortSpec {
          column: "name".to_string(),
          direction: SortDirection::Desc,
        }),
        filters: vec![],
        include_ctid: false,
        include_xmin: false,
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
        limit: Some(2),
        offset: 2,
        sort: Some(crate::connectors::SortSpec {
          column: "name".to_string(),
          direction: SortDirection::Desc,
        }),
        filters: vec![],
        include_ctid: false,
        include_xmin: false,
      })
      .await
      .unwrap();
    assert_eq!(next.statements[0].rows.len(), 1);
    assert_eq!(
      next.statements[0].rows[0][1],
      Some("Ada Lovelace".to_string())
    );
  }

  async fn filtered_rows(
    pg: &PostgresConnection,
    table: &str,
    filters: Vec<ColumnFilter>,
  ) -> StatementResult {
    pg.table_rows(&TableRowsRequest {
      schema: "app".to_string(),
      table: table.to_string(),
      limit: Some(100),
      offset: 0,
      sort: None,
      filters,
      include_ctid: false,
      include_xmin: false,
    })
    .await
    .unwrap()
    .statements
    .remove(0)
  }

  #[tokio::test]
  async fn integration_postgres_filters_compare_typed_columns() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };

    // contains on text.
    let by_name = filtered_rows(
      &pg,
      "customers",
      vec![filter("name", FilterOp::Contains, Some("ada"))],
    )
    .await;
    assert_eq!(by_name.rows.len(), 1);
    assert_eq!(by_name.rows[0][1].as_deref(), Some("Ada Lovelace"));

    // gt on numeric and on timestamptz: UNKNOWN params coerce to the column type.
    let expensive = filtered_rows(
      &pg,
      "orders",
      vec![filter("amount", FilterOp::Gt, Some("100"))],
    )
    .await;
    assert_eq!(expensive.rows.len(), 2);
    let recent = filtered_rows(
      &pg,
      "orders",
      vec![filter("placed_at", FilterOp::Gt, Some("2000-01-01"))],
    )
    .await;
    assert_eq!(recent.rows.len(), 3);

    // is-null, and two filters AND-ed.
    let no_email = filtered_rows(
      &pg,
      "customers",
      vec![filter("email", FilterOp::IsNull, None)],
    )
    .await;
    assert_eq!(no_email.rows.len(), 1);
    assert_eq!(no_email.rows[0][1].as_deref(), Some("Grace Hopper"));
    let both = filtered_rows(
      &pg,
      "orders",
      vec![
        filter("amount", FilterOp::Gt, Some("100")),
        filter("note", FilterOp::IsNotNull, None),
      ],
    )
    .await;
    assert_eq!(both.rows.len(), 2);
  }

  #[tokio::test]
  async fn integration_postgres_filters_keep_server_text_values() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let rows = filtered_rows(
      &pg,
      "customers",
      vec![filter("name", FilterOp::Eq, Some("Ada Lovelace"))],
    )
    .await;
    let tags = rows.columns.iter().position(|c| c.name == "tags").unwrap();
    let meta = rows.columns.iter().position(|c| c.name == "meta").unwrap();
    assert_eq!(rows.rows[0][tags].as_deref(), Some("{vip,eu}"));
    assert_eq!(
      rows.rows[0][meta].as_deref(),
      Some(r#"{"plan": "pro", "seats": 3}"#)
    );
    assert_eq!(rows.columns[tags].kind, ColumnKind::Array);

    let receipts = filtered_rows(
      &pg,
      "orders",
      vec![filter("receipt", FilterOp::IsNotNull, None)],
    )
    .await;
    let receipt = receipts
      .columns
      .iter()
      .position(|c| c.name == "receipt")
      .unwrap();
    assert_eq!(receipts.rows[0][receipt].as_deref(), Some("\\xdeadbeef"));
  }

  fn collecting_chunks() -> (
    Arc<Mutex<Vec<RowsChunk>>>,
    Box<dyn Fn(RowsChunk) -> bool + Send>,
  ) {
    let chunks: Arc<Mutex<Vec<RowsChunk>>> = Arc::default();
    let sink = chunks.clone();
    (
      chunks,
      Box::new(move |chunk| {
        sink.lock().unwrap().push(chunk);
        true
      }),
    )
  }

  #[tokio::test]
  async fn integration_postgres_stream_rows_chunks_in_order() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let (chunks, on_chunk) = collecting_chunks();
    let summary = pg
      .stream_rows(
        &TableRowsRequest {
          schema: "app".to_string(),
          table: "events".to_string(),
          limit: Some(1000),
          offset: 0,
          sort: Some(crate::connectors::SortSpec {
            column: "id".to_string(),
            direction: SortDirection::Asc,
          }),
          filters: vec![],
          include_ctid: false,
          include_xmin: false,
        },
        on_chunk,
      )
      .await
      .unwrap();
    assert_eq!(summary.rows, 1000.0);

    let chunks = chunks.lock().unwrap();
    assert_eq!(chunks.len(), 5, "1000 rows in 200-row chunks");
    assert!(chunks[0].columns.is_some(), "first chunk carries columns");
    assert!(chunks[1..].iter().all(|c| c.columns.is_none()));
    assert_eq!(chunks[0].rows[0][0].as_deref(), Some("1"));
    let last = chunks.last().unwrap();
    assert_eq!(last.rows.last().unwrap()[0].as_deref(), Some("1000"));
  }

  #[tokio::test]
  async fn integration_postgres_stream_rows_applies_filters() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let (chunks, on_chunk) = collecting_chunks();
    let summary = pg
      .stream_rows(
        &TableRowsRequest {
          schema: "app".to_string(),
          table: "events".to_string(),
          limit: Some(5000),
          offset: 0,
          sort: None,
          filters: vec![filter("kind", FilterOp::Eq, Some("purchase"))],
          include_ctid: false,
          include_xmin: false,
        },
        on_chunk,
      )
      .await
      .unwrap();
    // n % 3 == 2 over 1..=10000.
    assert_eq!(summary.rows, 3333.0);
    let total: usize = chunks.lock().unwrap().iter().map(|c| c.rows.len()).sum();
    assert_eq!(total, 3333);
  }

  // Export path: no limit streams the full table, past MAX_FETCH_ROWS.
  #[tokio::test]
  async fn integration_postgres_stream_rows_unlimited_exports_csv() {
    use crate::export::{ChunkSink, ExportFormat};

    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let out = Arc::new(Mutex::new(ChunkSink::new(
      Vec::<u8>::new(),
      ExportFormat::Csv,
      String::new(),
    )));
    let sink = out.clone();
    let summary = pg
      .stream_rows(
        &TableRowsRequest {
          schema: "app".to_string(),
          table: "events".to_string(),
          limit: None,
          offset: 0,
          sort: None,
          filters: vec![],
          include_ctid: false,
          include_xmin: false,
        },
        Box::new(move |chunk| sink.lock().unwrap().push(chunk)),
      )
      .await
      .unwrap();
    assert_eq!(summary.rows, 10000.0);

    let mut sink = Arc::into_inner(out).unwrap().into_inner().unwrap();
    assert!(sink.error.take().is_none());
    let csv = String::from_utf8(sink.finish().unwrap()).unwrap();
    assert_eq!(csv.lines().count(), 10001, "header + every row");
  }

  #[tokio::test]
  async fn integration_postgres_stream_abort_leaves_connection_usable() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let delivered: Arc<Mutex<usize>> = Arc::default();
    let sink = delivered.clone();
    let summary = pg
      .stream_rows(
        &TableRowsRequest {
          schema: "app".to_string(),
          table: "events".to_string(),
          limit: Some(5000),
          offset: 0,
          sort: None,
          filters: vec![],
          include_ctid: false,
          include_xmin: false,
        },
        Box::new(move |_chunk| {
          let mut count = sink.lock().unwrap();
          *count += 1;
          // Pretend the receiver disappeared after the first chunk.
          *count < 1
        }),
      )
      .await
      .unwrap();
    assert!(summary.rows < 5000.0, "the stream must stop early");
    assert_eq!(*delivered.lock().unwrap(), 1);
    // The pooled client survives an aborted stream.
    pg.run_query("SELECT 1").await.unwrap();
  }

  #[tokio::test]
  async fn integration_postgres_apply_changes_roundtrip_in_one_transaction() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    // Work on rows this test owns so parallel tests stay unaffected.
    let applied = pg
      .apply_changes(&TableChanges {
        inserts: vec![crate::connectors::RowInsert {
          values: vec![
            cell("name", Some("Temp Row")),
            cell("email", Some("temp@example.com")),
          ],
        }],
        ..no_changes()
      })
      .await
      .unwrap();
    assert_eq!(applied.inserted, 1);

    let update_null = pg
      .apply_changes(&TableChanges {
        updates: vec![crate::connectors::RowUpdate {
          key: vec![cell("email", Some("temp@example.com"))],
          set: vec![cell("name", Some("Temp Renamed")), cell("meta", None)],
        }],
        ..no_changes()
      })
      .await
      .unwrap();
    assert_eq!(update_null.updated, 1);

    let rows = filtered_rows(
      &pg,
      "customers",
      vec![filter("email", FilterOp::Eq, Some("temp@example.com"))],
    )
    .await;
    assert_eq!(rows.rows[0][1].as_deref(), Some("Temp Renamed"));

    let deleted = pg
      .apply_changes(&TableChanges {
        deletes: vec![crate::connectors::RowDelete {
          key: vec![cell("email", Some("temp@example.com"))],
        }],
        ..no_changes()
      })
      .await
      .unwrap();
    assert_eq!(deleted.deleted, 1);
  }

  #[tokio::test]
  async fn integration_postgres_apply_changes_rolls_back_entirely() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    // First update is valid, second matches nothing: NOTHING must stick.
    let result = pg
      .apply_changes(&TableChanges {
        updates: vec![
          crate::connectors::RowUpdate {
            key: vec![cell("id", Some("1"))],
            set: vec![cell("name", Some("Should Not Stick"))],
          },
          crate::connectors::RowUpdate {
            key: vec![cell("id", Some("999999"))],
            set: vec![cell("name", Some("x"))],
          },
        ],
        ..no_changes()
      })
      .await;
    let Err(Error::Database { message }) = result else {
      panic!("expected the batch to fail");
    };
    assert!(message.contains("matched 0 rows"), "{message}");

    let rows = filtered_rows(
      &pg,
      "customers",
      vec![filter("id", FilterOp::Eq, Some("1"))],
    )
    .await;
    assert_eq!(rows.rows[0][1].as_deref(), Some("Ada Lovelace"));
    // The pooled client came back clean (no open transaction).
    pg.run_query("SELECT 1").await.unwrap();
  }

  #[tokio::test]
  async fn integration_postgres_xmin_guard_detects_concurrent_writes() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    pg.apply_changes(&TableChanges {
      inserts: vec![crate::connectors::RowInsert {
        values: vec![
          cell("name", Some("Xmin Guard")),
          cell("email", Some("xmin@example.com")),
        ],
      }],
      ..no_changes()
    })
    .await
    .unwrap();

    let fetched = pg
      .table_rows(&TableRowsRequest {
        schema: "app".to_string(),
        table: "customers".to_string(),
        limit: Some(1),
        offset: 0,
        sort: None,
        filters: vec![filter("email", FilterOp::Eq, Some("xmin@example.com"))],
        include_ctid: false,
        include_xmin: true,
      })
      .await
      .unwrap();
    let statement = &fetched.statements[0];
    assert_eq!(statement.columns[0].name, "xmin");
    let stale_xmin = statement.rows[0][0].clone();

    // A concurrent write bumps xmin: the stale guard must match nothing.
    pg.run_query(
      "UPDATE app.customers SET name = 'Moved Underneath' WHERE email = 'xmin@example.com'",
    )
    .await
    .unwrap();
    let result = pg
      .apply_changes(&TableChanges {
        updates: vec![crate::connectors::RowUpdate {
          key: vec![
            cell("email", Some("xmin@example.com")),
            crate::connectors::CellValue {
              column: "xmin".to_string(),
              value: stale_xmin,
            },
          ],
          set: vec![cell("name", Some("Should Conflict"))],
        }],
        ..no_changes()
      })
      .await;
    let Err(Error::Database { message }) = result else {
      panic!("expected the stale xmin to conflict");
    };
    assert!(message.contains("changed or deleted"), "{message}");

    // With the fresh xmin the same update goes through.
    let fresh = pg
      .table_rows(&TableRowsRequest {
        schema: "app".to_string(),
        table: "customers".to_string(),
        limit: Some(1),
        offset: 0,
        sort: None,
        filters: vec![filter("email", FilterOp::Eq, Some("xmin@example.com"))],
        include_ctid: false,
        include_xmin: true,
      })
      .await
      .unwrap();
    let fresh_xmin = fresh.statements[0].rows[0][0].clone();
    let applied = pg
      .apply_changes(&TableChanges {
        updates: vec![crate::connectors::RowUpdate {
          key: vec![
            cell("email", Some("xmin@example.com")),
            crate::connectors::CellValue {
              column: "xmin".to_string(),
              value: fresh_xmin,
            },
          ],
          set: vec![cell("name", Some("Guard Passed"))],
        }],
        ..no_changes()
      })
      .await
      .unwrap();
    assert_eq!(applied.updated, 1);

    pg.apply_changes(&TableChanges {
      deletes: vec![crate::connectors::RowDelete {
        key: vec![cell("email", Some("xmin@example.com"))],
      }],
      ..no_changes()
    })
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn integration_postgres_update_matching_several_rows_rolls_back() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    // Both audit_log seed rows share the same `at` (inserted in one statement):
    // keying on it matches 2 rows and must trip the exactly-one guard.
    let shared_at = pg
      .run_query("SELECT at::text FROM public.audit_log GROUP BY at HAVING count(*) > 1 LIMIT 1")
      .await
      .unwrap()
      .statements[0]
      .rows[0][0]
      .clone()
      .unwrap();
    let result = pg
      .apply_changes(&TableChanges {
        schema: "public".to_string(),
        table: "audit_log".to_string(),
        updates: vec![crate::connectors::RowUpdate {
          key: vec![cell("at", Some(&shared_at))],
          set: vec![cell("message", Some("clobbered"))],
        }],
        ..no_changes()
      })
      .await;
    let Err(Error::Database { message }) = result else {
      panic!("expected the multi-match update to fail");
    };
    assert!(message.contains("matched 2 rows"), "{message}");

    let clobbered = pg
      .run_query("SELECT count(*) FROM public.audit_log WHERE message = 'clobbered'")
      .await
      .unwrap();
    assert_eq!(clobbered.statements[0].rows[0][0].as_deref(), Some("0"));
  }

  #[tokio::test]
  async fn integration_postgres_write_values_cannot_inject_sql() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let hostile = "'; DROP TABLE app.customers; --";
    let email = "hostile@example.com";
    pg.apply_changes(&TableChanges {
      inserts: vec![crate::connectors::RowInsert {
        values: vec![cell("name", Some(hostile)), cell("email", Some(email))],
      }],
      ..no_changes()
    })
    .await
    .unwrap();

    // Stored literally, executed never.
    let stored = filtered_rows(
      &pg,
      "customers",
      vec![filter("email", FilterOp::Eq, Some(email))],
    )
    .await;
    assert_eq!(stored.rows[0][1].as_deref(), Some(hostile));

    pg.apply_changes(&TableChanges {
      deletes: vec![crate::connectors::RowDelete {
        key: vec![cell("email", Some(email))],
      }],
      ..no_changes()
    })
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn integration_postgres_invalid_cast_applies_nothing() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let result = pg
      .apply_changes(&TableChanges {
        schema: "app".to_string(),
        table: "orders".to_string(),
        updates: vec![crate::connectors::RowUpdate {
          key: vec![cell("id", Some("1"))],
          set: vec![cell("amount", Some("not-a-number"))],
        }],
        ..no_changes()
      })
      .await;
    let Err(Error::Database { message }) = result else {
      panic!("expected the cast to fail");
    };
    assert!(message.contains("invalid input syntax"), "{message}");

    let intact = filtered_rows(&pg, "orders", vec![filter("id", FilterOp::Eq, Some("1"))]).await;
    assert_eq!(intact.rows[0][2].as_deref(), Some("129.90"));
  }

  #[tokio::test]
  async fn integration_postgres_ctid_editing_on_pkless_table() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let inserted = pg
      .apply_changes(&TableChanges {
        schema: "public".to_string(),
        table: "audit_log".to_string(),
        inserts: vec![crate::connectors::RowInsert {
          values: vec![cell("message", Some("ctid test row"))],
        }],
        ..no_changes()
      })
      .await
      .unwrap();
    assert_eq!(inserted.inserted, 1);

    let fetched = pg
      .table_rows(&TableRowsRequest {
        schema: "public".to_string(),
        table: "audit_log".to_string(),
        limit: Some(100),
        offset: 0,
        sort: None,
        filters: vec![filter("message", FilterOp::Eq, Some("ctid test row"))],
        include_ctid: true,
        include_xmin: false,
      })
      .await
      .unwrap();
    let statement = &fetched.statements[0];
    assert_eq!(statement.columns[0].name, "ctid");
    let ctid = statement.rows[0][0].clone().unwrap();

    let deleted = pg
      .apply_changes(&TableChanges {
        schema: "public".to_string(),
        table: "audit_log".to_string(),
        deletes: vec![crate::connectors::RowDelete {
          key: vec![cell("ctid", Some(&ctid))],
        }],
        ..no_changes()
      })
      .await
      .unwrap();
    assert_eq!(deleted.deleted, 1);
  }

  #[tokio::test]
  async fn integration_postgres_filter_values_cannot_inject_sql() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    // Values are bound parameters: hostile input is compared, never executed.
    for hostile in [
      "'; DROP TABLE app.customers; --",
      "1 OR 1=1",
      "Ada' OR name <> '",
    ] {
      let rows = filtered_rows(
        &pg,
        "customers",
        vec![filter("name", FilterOp::Eq, Some(hostile))],
      )
      .await;
      assert!(rows.rows.is_empty(), "{hostile:?} must match nothing");
      let contains = filtered_rows(
        &pg,
        "customers",
        vec![filter("name", FilterOp::Contains, Some(hostile))],
      )
      .await;
      assert!(contains.rows.is_empty(), "{hostile:?} must match nothing");
    }
    // The table survived every attempt. (No global count: the write tests
    // running in parallel insert their own temporary rows.)
    let intact = filtered_rows(
      &pg,
      "customers",
      vec![filter("name", FilterOp::Eq, Some("Ada Lovelace"))],
    )
    .await;
    assert_eq!(intact.rows.len(), 1);
  }

  #[tokio::test]
  async fn integration_postgres_filters_combine_with_sort_and_offset() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let result = pg
      .table_rows(&TableRowsRequest {
        schema: "app".to_string(),
        table: "orders".to_string(),
        limit: Some(1),
        offset: 1,
        sort: Some(crate::connectors::SortSpec {
          column: "amount".to_string(),
          direction: SortDirection::Desc,
        }),
        filters: vec![filter("amount", FilterOp::Gt, Some("40"))],
        include_ctid: false,
        include_xmin: false,
      })
      .await
      .unwrap();
    // amounts > 40 sorted desc: 999.99, 129.90, 49.00 -> offset 1 = 129.90.
    assert_eq!(result.statements[0].rows[0][2].as_deref(), Some("129.90"));
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
      group: None,
      params: crate::profiles::ConnectorParams::Postgres(SqlServerParams {
        host: host.clone(),
        port: config.get_ports()[0],
        database: config.get_dbname().unwrap().to_string(),
        user: config.get_user().unwrap().to_string(),
        ssl_mode: SslMode::Prefer,
        ssl_root_cert: None,
        tunnel_id: None,
      }),
    }
  }

  #[tokio::test]
  async fn integration_postgres_auth_failure_maps_to_database_error() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG") else {
      return;
    };
    let profile = profile_from_env_url(&url);
    let result = PostgresConnector
      .connect(&profile, Some("definitely-wrong"), None)
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
    match &mut profile.params {
      crate::profiles::ConnectorParams::Postgres(params) => params.port = 59999,
      _ => unreachable!(),
    }
    let result = PostgresConnector.connect(&profile, None, None).await;
    let Err(Error::Database { message }) = result.map(|_| ()) else {
      panic!("expected a database error");
    };
    assert!(!message.is_empty());
  }
}
