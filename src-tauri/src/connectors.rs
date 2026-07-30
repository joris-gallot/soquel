use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::Error;
use crate::postgres::PostgresConnector;
use crate::profiles::{ConnectionProfile, ConnectorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
  SqlQuery,
  Introspection,
  // Constructed once the Redis connector lands.
  #[allow(dead_code)]
  KvBrowse,
}

/// Coarse type family for UI decisions (alignment, editors, viewers);
/// `data_type` keeps the exact postgres name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ColumnKind {
  Bool,
  Number,
  Text,
  Json,
  Bytes,
  DateTime,
  Uuid,
  Array,
  #[default]
  Other,
}

// Deserialize: the export commands take columns back from the webview.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueryColumn {
  pub name: String,
  /// None when type metadata is unavailable (multi-statement scripts).
  pub data_type: Option<String>,
  pub kind: ColumnKind,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerNotice {
  pub severity: String,
  pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StatementResult {
  pub columns: Vec<QueryColumn>,
  pub rows: Vec<Vec<Option<String>>>,
  pub rows_affected: f64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
  pub statements: Vec<StatementResult>,
  pub notices: Vec<ServerNotice>,
  pub duration_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum SortDirection {
  Asc,
  Desc,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
  pub column: String,
  pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum FilterOp {
  Eq,
  Neq,
  Lt,
  Lte,
  Gt,
  Gte,
  Contains,
  StartsWith,
  IsNull,
  IsNotNull,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColumnFilter {
  pub column: String,
  pub op: FilterOp,
  /// Absent for the null operators.
  pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableRowsRequest {
  pub schema: String,
  pub table: String,
  /// None streams the full result set (export).
  pub limit: Option<u32>,
  pub offset: u32,
  pub sort: Option<SortSpec>,
  #[serde(default)]
  pub filters: Vec<ColumnFilter>,
  /// ctid-keyed editing for tables without a primary key.
  #[serde(default)]
  pub include_ctid: bool,
  /// Optimistic-lock guard for editing: any concurrent write bumps xmin.
  #[serde(default)]
  pub include_xmin: bool,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CellValue {
  pub column: String,
  /// None writes NULL.
  pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RowUpdate {
  pub key: Vec<CellValue>,
  pub set: Vec<CellValue>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RowInsert {
  /// Omitted columns take their DEFAULT.
  pub values: Vec<CellValue>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RowDelete {
  pub key: Vec<CellValue>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableChanges {
  pub schema: String,
  pub table: String,
  pub updates: Vec<RowUpdate>,
  pub inserts: Vec<RowInsert>,
  pub deletes: Vec<RowDelete>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RowsChunk {
  /// Present on the first chunk only.
  pub columns: Option<Vec<QueryColumn>>,
  pub rows: Vec<Vec<Option<String>>>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamSummary {
  pub rows: f64,
  pub duration_ms: f64,
  pub notices: Vec<ServerNotice>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
  pub updated: u32,
  pub inserted: u32,
  pub deleted: u32,
  pub duration_ms: f64,
}

/// SQL capability surface; only connections whose connector declares
/// `Capability::SqlQuery` expose it.
#[async_trait::async_trait]
pub trait SqlQuery: Send + Sync {
  async fn run_query(&self, sql: &str) -> Result<QueryResult, Error>;
  async fn cancel(&self) -> Result<(), Error>;
  async fn table_rows(&self, request: &TableRowsRequest) -> Result<QueryResult, Error>;
  /// Chunked delivery; `on_chunk` returning false aborts (receiver gone).
  async fn stream_rows(
    &self,
    request: &TableRowsRequest,
    on_chunk: Box<dyn Fn(RowsChunk) -> bool + Send>,
  ) -> Result<StreamSummary, Error>;
  async fn apply_changes(&self, changes: &TableChanges) -> Result<ApplyResult, Error>;
  async fn open_session(&self) -> Result<Box<dyn SqlSession>, Error>;
}

/// A dedicated client outside the pool: session state (SET, transactions)
/// sticks, and cancel targets only this session.
#[async_trait::async_trait]
pub trait SqlSession: Send + Sync {
  async fn run_query(&self, sql: &str) -> Result<QueryResult, Error>;
  async fn cancel(&self) -> Result<(), Error>;
  async fn close(&self) -> Result<(), Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum TableKind {
  Table,
  View,
  MaterializedView,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
  pub name: String,
  pub data_type: String,
  pub nullable: bool,
  pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
  pub name: String,
  pub definition: String,
  pub unique: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyInfo {
  pub name: String,
  pub columns: Vec<String>,
  pub referenced_schema: String,
  pub referenced_table: String,
  pub referenced_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
  pub name: String,
  pub kind: TableKind,
  /// Planner estimate (pg reltuples): -1 when never analyzed.
  pub estimated_rows: f64,
  pub columns: Vec<ColumnInfo>,
  pub primary_key: Vec<String>,
  pub indexes: Vec<IndexInfo>,
  pub foreign_keys: Vec<ForeignKeyInfo>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SchemaInfo {
  pub name: String,
  pub tables: Vec<TableInfo>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSnapshot {
  pub schemas: Vec<SchemaInfo>,
}

/// Introspection capability surface, mirroring `Capability::Introspection`.
#[async_trait::async_trait]
pub trait Introspect: Send + Sync {
  async fn schema_snapshot(&self) -> Result<SchemaSnapshot, Error>;
  async fn table_ddl(&self, schema: &str, table: &str) -> Result<String, Error>;
}

/// A live connection to a database, produced by a `Connector`.
#[async_trait::async_trait]
pub trait Connection: Send + Sync {
  async fn health(&self) -> Result<(), Error>;
  async fn close(&self) -> Result<(), Error>;
  fn sql(&self) -> Option<&dyn SqlQuery> {
    None
  }
  fn introspect(&self) -> Option<&dyn Introspect> {
    None
  }
}

/// Local TCP endpoint of an SSH forward. TCP dials 127.0.0.1:{port} while the
/// profile keeps the logical host, so TLS verification still targets it.
#[derive(Debug, Clone, Copy)]
pub struct LocalForward {
  pub port: u16,
}

/// A database kind the app knows how to talk to. Capabilities drive the UI:
/// no capability may assume SQL (Redis browses keys, not tables).
#[async_trait::async_trait]
pub trait Connector: Send + Sync {
  fn capabilities(&self) -> &'static [Capability];
  async fn connect(
    &self,
    profile: &ConnectionProfile,
    secret: Option<&str>,
    forward: Option<LocalForward>,
  ) -> Result<Box<dyn Connection>, Error>;
}

// Exhaustive match: adding a ConnectorKind refuses to compile until it gets a connector.
pub fn connector_for(kind: ConnectorKind) -> &'static dyn Connector {
  match kind {
    ConnectorKind::Postgres => &PostgresConnector,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn postgres_declares_sql_capabilities() {
    let caps = connector_for(ConnectorKind::Postgres).capabilities();
    assert!(caps.contains(&Capability::SqlQuery));
    assert!(caps.contains(&Capability::Introspection));
    assert!(!caps.contains(&Capability::KvBrowse));
  }
}
