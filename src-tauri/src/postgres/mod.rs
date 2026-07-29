use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deadpool::managed::{self, Metrics, Object, Pool, RecycleError, RecycleResult};
use tokio_postgres::types::{Kind as PgKind, Type as PgType};
use tokio_postgres::{AsyncMessage, CancelToken, Client, Config, SimpleQueryMessage};

use crate::connectors::{
  Capability, ColumnFilter, ColumnKind, Connection, Connector, FilterOp, Introspect, QueryColumn,
  QueryResult, ServerNotice, SortDirection, SqlQuery, SqlSession, StatementResult,
  TableRowsRequest,
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
    let pg = self.checkout().await?;
    let _guard = self.register_cancel(pg.client.cancel_token());
    pg.notices.lock().unwrap().clear();
    let start = Instant::now();

    // The prepared column list is the only source of column identity: filters
    // and sort must name one of these, so no frontend string reaches SQL unquoted.
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
    let projection = columns
      .iter()
      .map(|(name, _)| {
        let ident = quote_ident(name);
        format!("{ident}::text AS {ident}")
      })
      .collect::<Vec<_>>()
      .join(", ");
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
    sql.push_str(&format!(
      " LIMIT {} OFFSET {}",
      request.limit.min(1000),
      request.offset
    ));

    // Every parameter travels as text; build_where casts it to the column's type.
    let bind: Vec<(&(dyn tokio_postgres::types::ToSql + Sync), PgType)> = params
      .iter()
      .map(|value| (value as &(dyn tokio_postgres::types::ToSql + Sync), PgType::TEXT))
      .collect();
    let rows = pg.client.query_typed(&sql, &bind).await?;

    let statement = StatementResult {
      columns: columns
        .iter()
        .map(|(name, ty)| QueryColumn {
          name: name.clone(),
          data_type: Some(type_name(ty)),
          kind: column_kind(ty),
        })
        .collect(),
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
  use crate::profiles::ConnectorKind;

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
      (FilterOp::Eq, Some("x"), r#" WHERE "name" = $1::text"#, Some("x")),
      (FilterOp::Neq, Some("x"), r#" WHERE "name" <> $1::text"#, Some("x")),
      (FilterOp::Lt, Some("5"), r#" WHERE "name" < $1::text"#, Some("5")),
      (FilterOp::Lte, Some("5"), r#" WHERE "name" <= $1::text"#, Some("5")),
      (FilterOp::Gt, Some("5"), r#" WHERE "name" > $1::text"#, Some("5")),
      (FilterOp::Gte, Some("5"), r#" WHERE "name" >= $1::text"#, Some("5")),
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
      assert_eq!(params, param.map(str::to_string).into_iter().collect::<Vec<_>>());
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
        filters: vec![],
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
        filters: vec![],
      })
      .await
      .unwrap();
    assert_eq!(next.statements[0].rows.len(), 1);
    assert_eq!(next.statements[0].rows[0][1], Some("Ada Lovelace".to_string()));
  }

  async fn filtered_rows(
    pg: &PostgresConnection,
    table: &str,
    filters: Vec<ColumnFilter>,
  ) -> StatementResult {
    pg.table_rows(&TableRowsRequest {
      schema: "app".to_string(),
      table: table.to_string(),
      limit: 100,
      offset: 0,
      sort: None,
      filters,
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
    // The table survived every attempt.
    let intact = filtered_rows(&pg, "customers", vec![]).await;
    assert_eq!(intact.rows.len(), 3);
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
        limit: 1,
        offset: 1,
        sort: Some(crate::connectors::SortSpec {
          column: "amount".to_string(),
          direction: SortDirection::Desc,
        }),
        filters: vec![filter("amount", FilterOp::Gt, Some("40"))],
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
