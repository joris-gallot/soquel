use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Env {
  Dev,
  Staging,
  Prod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectorKind {
  Postgres,
  Mysql,
  Sqlite,
  Redis,
  Mongo,
}

/// libpq semantics: `require` encrypts without verifying the certificate,
/// only `verify-full` checks the chain and hostname.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
  Disable,
  #[default]
  Prefer,
  Require,
  VerifyFull,
}

/// Shared shape for TCP SQL servers (postgres, mysql).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlServerParams {
  pub host: String,
  pub port: u16,
  pub database: String,
  pub user: String,
  #[serde(default)]
  pub ssl_mode: SslMode,
  /// CA bundle path for verify-full (libpq sslrootcert); None = platform store.
  #[serde(default)]
  pub ssl_root_cert: Option<String>,
  #[serde(default)]
  pub tunnel_id: Option<String>,
}

/// Redis speaks AUTH, not SQL: username is optional (ACL), the database is a
/// numeric index, and TLS is a plain toggle (rediss://).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RedisParams {
  pub host: String,
  pub port: u16,
  #[serde(default)]
  pub db: u32,
  #[serde(default)]
  pub username: Option<String>,
  #[serde(default)]
  pub tls: bool,
  #[serde(default)]
  pub tunnel_id: Option<String>,
}

/// MongoDB single node (v1); srv/replica-set discovery later. `database` is
/// the default db the UI opens; credentials validate against `auth_source`,
/// falling back to `database`, then the driver's "admin" (URI semantics).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MongoParams {
  pub host: String,
  pub port: u16,
  #[serde(default)]
  pub database: Option<String>,
  #[serde(default)]
  pub username: Option<String>,
  #[serde(default)]
  pub auth_source: Option<String>,
  #[serde(default)]
  pub tls: bool,
  #[serde(default)]
  pub tunnel_id: Option<String>,
}

/// Per-kind connection parameters; future kinds bring their own shapes.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ConnectorParams {
  Postgres(SqlServerParams),
  Mysql(SqlServerParams),
  #[serde(rename_all = "camelCase")]
  Sqlite {
    path: String,
  },
  Redis(RedisParams),
  Mongo(MongoParams),
}

/// TCP endpoint a tunnel can forward to; kind-agnostic.
pub struct RemoteEndpoint<'a> {
  pub host: &'a str,
  pub port: u16,
  pub tunnel_id: Option<&'a str>,
}

impl ConnectorParams {
  pub fn kind(&self) -> ConnectorKind {
    match self {
      Self::Postgres(_) => ConnectorKind::Postgres,
      Self::Mysql(_) => ConnectorKind::Mysql,
      Self::Sqlite { .. } => ConnectorKind::Sqlite,
      Self::Redis(_) => ConnectorKind::Redis,
      Self::Mongo(_) => ConnectorKind::Mongo,
    }
  }

  /// TCP SQL server shape; None for other kinds.
  pub fn sql_server(&self) -> Option<&SqlServerParams> {
    match self {
      Self::Postgres(params) | Self::Mysql(params) => Some(params),
      Self::Sqlite { .. } | Self::Redis(_) | Self::Mongo(_) => None,
    }
  }

  /// Where TCP goes (tunnel included); None for file-backed kinds.
  pub fn remote(&self) -> Option<RemoteEndpoint<'_>> {
    match self {
      Self::Postgres(params) | Self::Mysql(params) => Some(RemoteEndpoint {
        host: &params.host,
        port: params.port,
        tunnel_id: params.tunnel_id.as_deref(),
      }),
      Self::Redis(params) => Some(RemoteEndpoint {
        host: &params.host,
        port: params.port,
        tunnel_id: params.tunnel_id.as_deref(),
      }),
      Self::Mongo(params) => Some(RemoteEndpoint {
        host: &params.host,
        port: params.port,
        tunnel_id: params.tunnel_id.as_deref(),
      }),
      Self::Sqlite { .. } => None,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
  pub id: String,
  pub name: String,
  pub env: Env,
  #[serde(default)]
  pub group: Option<String>,
  pub params: ConnectorParams,
}

/// Secrets ride in on the input but are stored in the OS keychain, never in the profile.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
  pub name: String,
  pub env: Env,
  #[serde(default)]
  pub group: Option<String>,
  pub params: ConnectorParams,
  pub password: Option<String>,
}

pub struct ProfileStore {
  path: PathBuf,
  profiles: Vec<ConnectionProfile>,
}

impl ProfileStore {
  pub fn load(path: PathBuf) -> Result<Self, Error> {
    let profiles = match fs::read_to_string(&path) {
      // A pre-params-redesign (or corrupted) file must not block startup:
      // move it aside and start fresh.
      Ok(raw) => match serde_json::from_str(&raw) {
        Ok(profiles) => profiles,
        Err(err) => {
          log::warn!("connections.json unreadable ({err}); moving it to connections.json.bak");
          let _ = fs::rename(&path, path.with_extension("json.bak"));
          Vec::new()
        }
      },
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
      Err(err) => return Err(err.into()),
    };
    Ok(Self { path, profiles })
  }

  fn save(&self) -> Result<(), Error> {
    if let Some(dir) = self.path.parent() {
      fs::create_dir_all(dir)?;
    }
    fs::write(&self.path, serde_json::to_string_pretty(&self.profiles)?)?;
    Ok(())
  }

  pub fn list(&self) -> Vec<ConnectionProfile> {
    self.profiles.clone()
  }

  pub fn get(&self, id: &str) -> Result<ConnectionProfile, Error> {
    self
      .profiles
      .iter()
      .find(|p| p.id == id)
      .cloned()
      .ok_or_else(|| Error::NotFound {
        message: format!("connection {id} not found"),
      })
  }

  pub fn create(&mut self, input: &ConnectionInput) -> Result<ConnectionProfile, Error> {
    let profile = ConnectionProfile {
      id: uuid::Uuid::new_v4().to_string(),
      name: input.name.clone(),
      env: input.env,
      group: input.group.clone(),
      params: input.params.clone(),
    };
    self.profiles.push(profile.clone());
    self.save()?;
    Ok(profile)
  }

  pub fn update(&mut self, id: &str, input: &ConnectionInput) -> Result<ConnectionProfile, Error> {
    let profile = self
      .profiles
      .iter_mut()
      .find(|p| p.id == id)
      .ok_or_else(|| Error::NotFound {
        message: format!("connection {id} not found"),
      })?;
    profile.name = input.name.clone();
    profile.env = input.env;
    profile.group = input.group.clone();
    profile.params = input.params.clone();
    let updated = profile.clone();
    self.save()?;
    Ok(updated)
  }

  pub fn delete(&mut self, id: &str) -> Result<(), Error> {
    let before = self.profiles.len();
    self.profiles.retain(|p| p.id != id);
    if self.profiles.len() == before {
      return Err(Error::NotFound {
        message: format!("connection {id} not found"),
      });
    }
    self.save()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn input(name: &str) -> ConnectionInput {
    ConnectionInput {
      name: name.to_string(),
      env: Env::Dev,
      group: None,
      params: ConnectorParams::Postgres(SqlServerParams {
        host: "localhost".to_string(),
        port: 5432,
        database: "app".to_string(),
        user: "postgres".to_string(),
        ssl_mode: SslMode::Prefer,
        ssl_root_cert: None,
        tunnel_id: None,
      }),
      password: None,
    }
  }

  fn store() -> (tempfile::TempDir, ProfileStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = ProfileStore::load(dir.path().join("connections.json")).unwrap();
    (dir, store)
  }

  #[test]
  fn crud_roundtrip_persists() {
    let (dir, mut store) = store();
    let created = store.create(&input("local")).unwrap();
    assert_eq!(store.list().len(), 1);

    let mut changed = input("renamed");
    changed.params = ConnectorParams::Mysql(SqlServerParams {
      host: "db.internal".to_string(),
      port: 3306,
      database: "app".to_string(),
      user: "soquel".to_string(),
      ssl_mode: SslMode::VerifyFull,
      ssl_root_cert: Some("/etc/ca.pem".to_string()),
      tunnel_id: None,
    });
    let updated = store.update(&created.id, &changed).unwrap();
    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.params.kind(), ConnectorKind::Mysql);

    // Reload from disk: the tagged params survive, kind included.
    let reloaded = ProfileStore::load(dir.path().join("connections.json")).unwrap();
    let profile = reloaded.get(&created.id).unwrap();
    assert_eq!(profile.name, "renamed");
    assert_eq!(profile.params.kind(), ConnectorKind::Mysql);
    assert_eq!(
      profile
        .params
        .sql_server()
        .unwrap()
        .ssl_root_cert
        .as_deref(),
      Some("/etc/ca.pem")
    );

    store.delete(&created.id).unwrap();
    assert!(store.list().is_empty());
  }

  #[test]
  fn group_survives_a_reload_and_can_be_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.json");
    let mut store = ProfileStore::load(path.clone()).unwrap();

    let mut grouped = input("local");
    grouped.group = Some("clients".to_string());
    let created = store.create(&grouped).unwrap();
    assert_eq!(
      ProfileStore::load(path.clone())
        .unwrap()
        .get(&created.id)
        .unwrap()
        .group
        .as_deref(),
      Some("clients")
    );

    // Clearing must write null, not keep the old value.
    let updated = store.update(&created.id, &input("local")).unwrap();
    assert_eq!(updated.group, None);
    let reloaded = ProfileStore::load(path).unwrap();
    assert_eq!(reloaded.get(&created.id).unwrap().group, None);
  }

  #[test]
  fn params_serialize_with_an_inline_kind_tag() {
    let (_dir, mut store) = store();
    store.create(&input("local")).unwrap();
    let raw = fs::read_to_string(store.path.clone()).unwrap();
    // The tag lives inside params: the discriminated union the frontend sees.
    assert!(raw.contains(r#""kind": "postgres""#), "{raw}");
    assert!(!raw.contains(r#""params": null"#), "{raw}");
  }

  #[test]
  fn unreadable_store_is_moved_aside_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.json");
    // The pre-redesign flat shape: no params object.
    fs::write(
      &path,
      r#"[{"id":"1","name":"old","env":"dev","kind":"postgres","host":"h","port":5432,"database":"db","user":"u"}]"#,
    )
    .unwrap();

    let store = ProfileStore::load(path.clone()).unwrap();
    assert!(store.list().is_empty());
    assert!(!path.exists());
    assert!(path.with_extension("json.bak").exists());
  }

  #[test]
  fn missing_id_is_not_found() {
    let (_dir, mut store) = store();
    assert!(matches!(store.get("nope"), Err(Error::NotFound { .. })));
    assert!(matches!(
      store.update("nope", &input("x")),
      Err(Error::NotFound { .. })
    ));
    assert!(matches!(store.delete("nope"), Err(Error::NotFound { .. })));
  }
}
