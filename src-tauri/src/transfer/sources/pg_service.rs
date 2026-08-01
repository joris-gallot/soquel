//! libpq's connection service file: `[name]` sections of `key=value`.

use crate::error::Error;
use crate::profiles::{ConnectorParams, CredentialSource, Env, SqlServerParams, SslMode};
use crate::transfer::{ImportBundle, IncomingConnection};

#[derive(Default)]
struct Service {
  name: String,
  host: Option<String>,
  port: Option<String>,
  database: Option<String>,
  user: Option<String>,
}

impl Service {
  fn into_connection(self) -> IncomingConnection {
    let port = self
      .port
      .as_deref()
      .and_then(|port| port.parse().ok())
      .unwrap_or(5432);
    // libpq falls back to the user for the database, and to the login name for
    // the user; only the host has no sane default, so a service without one
    // arrives with an empty host and the engine names the problem.
    let user = self.user.unwrap_or_default();
    let database = self.database.unwrap_or_else(|| user.clone());
    IncomingConnection {
      name: self.name,
      env: Env::Dev,
      group: None,
      credential: CredentialSource::Keychain,
      params: ConnectorParams::Postgres(SqlServerParams {
        host: self.host.unwrap_or_default(),
        port,
        database,
        user,
        ssl_mode: SslMode::Prefer,
        ssl_root_cert: None,
        tunnel_id: None,
      }),
      tunnel_ref: None,
      // The service file holds no password: that is what pgpass is for.
      secret: None,
      problem: None,
    }
  }
}

pub fn read(path: &std::path::Path) -> Result<ImportBundle, Error> {
  let raw = std::fs::read_to_string(path)?;
  let mut services: Vec<Service> = Vec::new();
  for line in raw.lines() {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
      continue;
    }
    if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
      services.push(Service {
        name: name.trim().to_string(),
        ..Default::default()
      });
      continue;
    }
    // A key outside any section belongs to no service: libpq ignores it too.
    let Some(service) = services.last_mut() else {
      continue;
    };
    let Some((key, value)) = line.split_once('=') else {
      continue;
    };
    let value = value.trim().to_string();
    match key.trim() {
      "host" | "hostaddr" => service.host = Some(value),
      "port" => service.port = Some(value),
      "dbname" => service.database = Some(value),
      "user" => service.user = Some(value),
      // Everything else (sslmode, application_name, ...) is not ours to map yet.
      _ => {}
    }
  }
  Ok(ImportBundle {
    connections: services.into_iter().map(Service::into_connection).collect(),
    tunnels: Vec::new(),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn bundle(contents: &str) -> ImportBundle {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".pg_service.conf");
    std::fs::write(&path, contents).unwrap();
    read(&path).unwrap()
  }

  #[test]
  fn each_section_becomes_a_connection_named_after_the_service() {
    let bundle = bundle(
      "# comment\n\
       [prod]\n\
       host=db.prod\n\
       port=5433\n\
       dbname=shop\n\
       user=app\n\
       sslmode=require\n\
       \n\
       [staging]\n\
       host=db.staging\n\
       user=app\n",
    );
    assert_eq!(bundle.connections.len(), 2);

    let prod = bundle.connections[0].params.sql_server().unwrap();
    assert_eq!(bundle.connections[0].name, "prod");
    assert_eq!(prod.host, "db.prod");
    assert_eq!(prod.port, 5433);
    assert_eq!(prod.database, "shop");

    // libpq defaults: 5432, and the database follows the user.
    let staging = bundle.connections[1].params.sql_server().unwrap();
    assert_eq!(staging.port, 5432);
    assert_eq!(staging.database, "app");
  }

  #[test]
  fn a_service_without_a_host_is_left_for_the_engine_to_refuse() {
    let bundle = bundle("[nowhere]\nuser=app\n");
    let params = bundle.connections[0].params.sql_server().unwrap();
    assert_eq!(params.host, "");
    assert_eq!(bundle.connections[0].problem, None);
  }

  #[test]
  fn keys_outside_a_section_belong_to_nothing() {
    let bundle = bundle("host=orphan\n[prod]\nhost=db.prod\n");
    assert_eq!(bundle.connections.len(), 1);
    assert_eq!(
      bundle.connections[0].params.sql_server().unwrap().host,
      "db.prod"
    );
  }
}
