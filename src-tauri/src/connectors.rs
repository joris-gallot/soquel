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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
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

#[derive(Debug, Clone, Serialize, Type)]
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

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableRowsRequest {
  pub schema: String,
  pub table: String,
  pub limit: u32,
  pub offset: u32,
  pub sort: Option<SortSpec>,
}

/// SQL capability surface; only connections whose connector declares
/// `Capability::SqlQuery` expose it.
#[async_trait::async_trait]
pub trait SqlQuery: Send + Sync {
  async fn run_query(&self, sql: &str) -> Result<QueryResult, Error>;
  async fn cancel(&self) -> Result<(), Error>;
  async fn table_rows(&self, request: &TableRowsRequest) -> Result<QueryResult, Error>;
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

/// A database kind the app knows how to talk to. Capabilities drive the UI:
/// no capability may assume SQL (Redis browses keys, not tables).
#[async_trait::async_trait]
pub trait Connector: Send + Sync {
  fn capabilities(&self) -> &'static [Capability];
  async fn connect(
    &self,
    profile: &ConnectionProfile,
    secret: Option<&str>,
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
