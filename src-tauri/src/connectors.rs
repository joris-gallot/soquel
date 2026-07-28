use serde::Serialize;
use specta::Type;

use crate::error::Error;
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

/// A live connection to a database, produced by a `Connector`.
// Callers land with the postgres driver; drop the allow then.
#[allow(dead_code)]
#[async_trait::async_trait]
pub trait Connection: Send + Sync {
    async fn health(&self) -> Result<(), Error>;
    async fn close(&self) -> Result<(), Error>;
}

/// A database kind the app knows how to talk to. Capabilities drive the UI:
/// no capability may assume SQL (Redis browses keys, not tables).
#[async_trait::async_trait]
pub trait Connector: Send + Sync {
    fn capabilities(&self) -> &'static [Capability];
    #[allow(dead_code)]
    async fn connect(
        &self,
        profile: &ConnectionProfile,
        secret: Option<&str>,
    ) -> Result<Box<dyn Connection>, Error>;
}

pub struct PostgresConnector;

#[async_trait::async_trait]
impl Connector for PostgresConnector {
    fn capabilities(&self) -> &'static [Capability] {
        &[Capability::SqlQuery, Capability::Introspection]
    }

    async fn connect(
        &self,
        _profile: &ConnectionProfile,
        _secret: Option<&str>,
    ) -> Result<Box<dyn Connection>, Error> {
        Err(Error::Unsupported {
            message: "postgres driver not implemented yet".to_string(),
        })
    }
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
    use crate::profiles::Env;

    fn profile() -> ConnectionProfile {
        ConnectionProfile {
            id: "test".to_string(),
            name: "local".to_string(),
            env: Env::Dev,
            kind: ConnectorKind::Postgres,
            host: "localhost".to_string(),
            port: 5432,
            database: "app".to_string(),
            user: "postgres".to_string(),
        }
    }

    #[test]
    fn postgres_declares_sql_capabilities() {
        let caps = connector_for(ConnectorKind::Postgres).capabilities();
        assert!(caps.contains(&Capability::SqlQuery));
        assert!(caps.contains(&Capability::Introspection));
        assert!(!caps.contains(&Capability::KvBrowse));
    }

    #[tokio::test]
    async fn postgres_connect_is_unsupported_for_now() {
        let result = connector_for(ConnectorKind::Postgres)
            .connect(&profile(), None)
            .await;
        assert!(matches!(result, Err(Error::Unsupported { .. })));
    }
}
