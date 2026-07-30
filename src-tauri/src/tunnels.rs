use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
  tag = "method",
  rename_all = "kebab-case",
  rename_all_fields = "camelCase"
)]
pub enum SshAuth {
  Agent,
  KeyFile {
    path: String,
  },
  Password,
  /// No credential: the server authorizes the connection on its own.
  None,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelProfile {
  pub id: String,
  pub name: String,
  pub host: String,
  pub port: u16,
  pub user: String,
  pub auth: SshAuth,
}

/// The secret (key passphrase or password) rides in on the input but is
/// stored in the SecretStore, never in the tunnel profile.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInput {
  pub name: String,
  pub host: String,
  pub port: u16,
  pub user: String,
  pub auth: SshAuth,
  pub secret: Option<String>,
}

pub struct TunnelStore {
  path: PathBuf,
  tunnels: Vec<TunnelProfile>,
}

impl TunnelStore {
  pub fn load(path: PathBuf) -> Result<Self, Error> {
    let tunnels = match fs::read_to_string(&path) {
      Ok(raw) => serde_json::from_str(&raw)?,
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
      Err(err) => return Err(err.into()),
    };
    Ok(Self { path, tunnels })
  }

  fn save(&self) -> Result<(), Error> {
    if let Some(dir) = self.path.parent() {
      fs::create_dir_all(dir)?;
    }
    fs::write(&self.path, serde_json::to_string_pretty(&self.tunnels)?)?;
    Ok(())
  }

  pub fn list(&self) -> Vec<TunnelProfile> {
    self.tunnels.clone()
  }

  pub fn get(&self, id: &str) -> Result<TunnelProfile, Error> {
    self
      .tunnels
      .iter()
      .find(|t| t.id == id)
      .cloned()
      .ok_or_else(|| Error::NotFound {
        message: format!("tunnel {id} not found"),
      })
  }

  pub fn create(&mut self, input: &TunnelInput) -> Result<TunnelProfile, Error> {
    let tunnel = TunnelProfile {
      id: uuid::Uuid::new_v4().to_string(),
      name: input.name.clone(),
      host: input.host.clone(),
      port: input.port,
      user: input.user.clone(),
      auth: input.auth.clone(),
    };
    self.tunnels.push(tunnel.clone());
    self.save()?;
    Ok(tunnel)
  }

  pub fn update(&mut self, id: &str, input: &TunnelInput) -> Result<TunnelProfile, Error> {
    let tunnel = self
      .tunnels
      .iter_mut()
      .find(|t| t.id == id)
      .ok_or_else(|| Error::NotFound {
        message: format!("tunnel {id} not found"),
      })?;
    tunnel.name = input.name.clone();
    tunnel.host = input.host.clone();
    tunnel.port = input.port;
    tunnel.user = input.user.clone();
    tunnel.auth = input.auth.clone();
    let updated = tunnel.clone();
    self.save()?;
    Ok(updated)
  }

  pub fn delete(&mut self, id: &str) -> Result<(), Error> {
    let before = self.tunnels.len();
    self.tunnels.retain(|t| t.id != id);
    if self.tunnels.len() == before {
      return Err(Error::NotFound {
        message: format!("tunnel {id} not found"),
      });
    }
    self.save()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn input(name: &str) -> TunnelInput {
    TunnelInput {
      name: name.to_string(),
      host: "bastion.internal".to_string(),
      port: 22,
      user: "deploy".to_string(),
      auth: SshAuth::KeyFile {
        path: "~/.ssh/id_ed25519".to_string(),
      },
      secret: None,
    }
  }

  #[test]
  fn crud_roundtrip_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tunnels.json");
    let mut store = TunnelStore::load(path.clone()).unwrap();

    let created = store.create(&input("bastion")).unwrap();
    assert_eq!(store.list().len(), 1);

    let mut changed = input("renamed");
    changed.auth = SshAuth::Agent;
    let updated = store.update(&created.id, &changed).unwrap();
    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.auth, SshAuth::Agent);

    let reloaded = TunnelStore::load(path).unwrap();
    assert_eq!(reloaded.get(&created.id).unwrap().name, "renamed");

    store.delete(&created.id).unwrap();
    assert!(store.list().is_empty());
    assert!(matches!(store.get("nope"), Err(Error::NotFound { .. })));
  }
}
