use std::time::{Duration, Instant};

use tokio_postgres::{CancelToken, Client, Config, NoTls, SimpleQueryMessage};

use crate::connectors::{
  Capability, Connection, Connector, QueryResult, SqlQuery, StatementResult,
};
use crate::error::Error;
use crate::profiles::ConnectionProfile;

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
      .connect_timeout(Duration::from_secs(10));
    if let Some(secret) = secret {
      config.password(secret);
    }
    let (client, connection) = config.connect(NoTls).await?;
    tauri::async_runtime::spawn(async move {
      if let Err(err) = connection.await {
        log::warn!("postgres connection closed: {err}");
      }
    });
    Ok(Box::new(PostgresConnection {
      cancel: client.cancel_token(),
      client,
    }))
  }
}

pub struct PostgresConnection {
  client: Client,
  cancel: CancelToken,
}

#[async_trait::async_trait]
impl Connection for PostgresConnection {
  async fn health(&self) -> Result<(), Error> {
    self.client.simple_query("SELECT 1").await?;
    Ok(())
  }

  // Dropping the client terminates the connection task.
  async fn close(&self) -> Result<(), Error> {
    Ok(())
  }

  fn sql(&self) -> Option<&dyn SqlQuery> {
    Some(self)
  }
}

#[async_trait::async_trait]
impl SqlQuery for PostgresConnection {
  async fn run_query(&self, sql: &str) -> Result<QueryResult, Error> {
    let start = Instant::now();
    let messages = self.client.simple_query(sql).await?;
    Ok(QueryResult {
      statements: collect_statements(messages),
      duration_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
  }

  async fn cancel(&self) -> Result<(), Error> {
    self.cancel.cancel_query(NoTls).await?;
    Ok(())
  }
}

fn collect_statements(messages: Vec<SimpleQueryMessage>) -> Vec<StatementResult> {
  let mut statements = Vec::new();
  let mut current = StatementResult::default();
  for message in messages {
    match message {
      SimpleQueryMessage::RowDescription(columns) => {
        current.columns = columns.iter().map(|c| c.name().to_string()).collect();
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

#[cfg(test)]
mod tests {
  use super::*;

  // Runs only with SOQUEL_TEST_PG=postgres://user:pass@host:port/db set.
  #[tokio::test]
  async fn query_roundtrip_against_real_postgres() {
    let Ok(url) = std::env::var("SOQUEL_TEST_PG") else {
      return;
    };
    let (client, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(connection);
    let pg = PostgresConnection {
      cancel: client.cancel_token(),
      client,
    };

    pg.health().await.unwrap();

    let result = pg
      .run_query("SELECT 1 AS one; SELECT 'a' AS a, NULL AS b")
      .await
      .unwrap();
    assert_eq!(result.statements.len(), 2);
    assert_eq!(result.statements[0].columns, vec!["one"]);
    assert_eq!(result.statements[0].rows_affected, 1.0);
    assert_eq!(
      result.statements[1].rows[0],
      vec![Some("a".to_string()), None]
    );
  }
}
