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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
  pub id: String,
  pub name: String,
  pub env: Env,
  pub kind: ConnectorKind,
  pub host: String,
  pub port: u16,
  pub database: String,
  pub user: String,
  #[serde(default)]
  pub ssl_mode: SslMode,
  #[serde(default)]
  pub tunnel_id: Option<String>,
}

/// Secrets ride in on the input but are stored in the OS keychain, never in the profile.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
  pub name: String,
  pub env: Env,
  pub kind: ConnectorKind,
  pub host: String,
  pub port: u16,
  pub database: String,
  pub user: String,
  #[serde(default)]
  pub ssl_mode: SslMode,
  #[serde(default)]
  pub tunnel_id: Option<String>,
  pub password: Option<String>,
}

pub struct ProfileStore {
  path: PathBuf,
  profiles: Vec<ConnectionProfile>,
}

impl ProfileStore {
  pub fn load(path: PathBuf) -> Result<Self, Error> {
    let profiles = match fs::read_to_string(&path) {
      Ok(raw) => serde_json::from_str(&raw)?,
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
      kind: input.kind,
      host: input.host.clone(),
      port: input.port,
      database: input.database.clone(),
      user: input.user.clone(),
      ssl_mode: input.ssl_mode,
      tunnel_id: input.tunnel_id.clone(),
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
    profile.kind = input.kind;
    profile.host = input.host.clone();
    profile.port = input.port;
    profile.database = input.database.clone();
    profile.user = input.user.clone();
    profile.ssl_mode = input.ssl_mode;
    profile.tunnel_id = input.tunnel_id.clone();
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
      kind: ConnectorKind::Postgres,
      host: "localhost".to_string(),
      port: 5432,
      database: "app".to_string(),
      user: "postgres".to_string(),
      ssl_mode: SslMode::Prefer,
      tunnel_id: None,
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
    changed.port = 5433;
    let updated = store.update(&created.id, &changed).unwrap();
    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.port, 5433);

    // Reload from disk: state survives.
    let reloaded = ProfileStore::load(dir.path().join("connections.json")).unwrap();
    assert_eq!(reloaded.get(&created.id).unwrap().name, "renamed");

    store.delete(&created.id).unwrap();
    assert!(store.list().is_empty());
  }

  #[test]
  fn profiles_without_ssl_mode_default_to_prefer() {
    let raw = r#"{"id":"1","name":"old","env":"dev","kind":"postgres",
      "host":"localhost","port":5432,"database":"app","user":"postgres"}"#;
    let profile: ConnectionProfile = serde_json::from_str(raw).unwrap();
    assert_eq!(profile.ssl_mode, SslMode::Prefer);
    assert_eq!(profile.tunnel_id, None);
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
