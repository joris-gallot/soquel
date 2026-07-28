use crate::error::Error;

const SERVICE: &str = "dev.soquel.app";

pub trait SecretStore: Send + Sync {
  fn set(&self, id: &str, secret: &str) -> Result<(), Error>;
  fn get(&self, id: &str) -> Result<Option<String>, Error>;
  fn delete(&self, id: &str) -> Result<(), Error>;
}

/// OS keychain: Keychain (macOS), Credential Manager (Windows), Secret Service (Linux).
pub struct KeyringStore;

impl KeyringStore {
  fn entry(id: &str) -> Result<keyring::Entry, Error> {
    Ok(keyring::Entry::new(SERVICE, &format!("connection:{id}"))?)
  }
}

impl SecretStore for KeyringStore {
  fn set(&self, id: &str, secret: &str) -> Result<(), Error> {
    Self::entry(id)?.set_password(secret)?;
    Ok(())
  }

  fn get(&self, id: &str) -> Result<Option<String>, Error> {
    match Self::entry(id)?.get_password() {
      Ok(secret) => Ok(Some(secret)),
      Err(keyring::Error::NoEntry) => Ok(None),
      Err(err) => Err(err.into()),
    }
  }

  fn delete(&self, id: &str) -> Result<(), Error> {
    match Self::entry(id)?.delete_credential() {
      Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
      Err(err) => Err(err.into()),
    }
  }
}

/// Plaintext secrets on disk. Explicit opt-in for keychain-less dev machines
/// (WSL) only: never the default, never shipped as one.
pub struct FileStore {
  path: std::path::PathBuf,
  secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl FileStore {
  pub fn load(path: std::path::PathBuf) -> Result<Self, Error> {
    let secrets = match std::fs::read_to_string(&path) {
      Ok(raw) => serde_json::from_str(&raw)?,
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => Default::default(),
      Err(err) => return Err(err.into()),
    };
    Ok(Self {
      path,
      secrets: std::sync::Mutex::new(secrets),
    })
  }

  fn save(&self, secrets: &std::collections::HashMap<String, String>) -> Result<(), Error> {
    if let Some(dir) = self.path.parent() {
      std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&self.path, serde_json::to_string(secrets)?)?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
  }
}

impl SecretStore for FileStore {
  fn set(&self, id: &str, secret: &str) -> Result<(), Error> {
    let mut secrets = self.secrets.lock().unwrap();
    secrets.insert(id.to_string(), secret.to_string());
    self.save(&secrets)
  }

  fn get(&self, id: &str) -> Result<Option<String>, Error> {
    Ok(self.secrets.lock().unwrap().get(id).cloned())
  }

  fn delete(&self, id: &str) -> Result<(), Error> {
    let mut secrets = self.secrets.lock().unwrap();
    secrets.remove(id);
    self.save(&secrets)
  }
}

/// Ephemeral store for e2e/CI environments without an OS keychain.
#[derive(Default)]
pub struct InMemoryStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl SecretStore for InMemoryStore {
  fn set(&self, id: &str, secret: &str) -> Result<(), Error> {
    self
      .0
      .lock()
      .unwrap()
      .insert(id.to_string(), secret.to_string());
    Ok(())
  }

  fn get(&self, id: &str) -> Result<Option<String>, Error> {
    Ok(self.0.lock().unwrap().get(id).cloned())
  }

  fn delete(&self, id: &str) -> Result<(), Error> {
    self.0.lock().unwrap().remove(id);
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn set_get_delete_roundtrip() {
    let store = InMemoryStore::default();
    assert_eq!(store.get("a").unwrap(), None);
    store.set("a", "s3cret").unwrap();
    assert_eq!(store.get("a").unwrap(), Some("s3cret".to_string()));
    store.delete("a").unwrap();
    assert_eq!(store.get("a").unwrap(), None);
  }

  #[test]
  fn file_store_survives_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");

    let store = FileStore::load(path.clone()).unwrap();
    store.set("a", "s3cret").unwrap();

    let reloaded = FileStore::load(path.clone()).unwrap();
    assert_eq!(reloaded.get("a").unwrap(), Some("s3cret".to_string()));

    reloaded.delete("a").unwrap();
    let emptied = FileStore::load(path).unwrap();
    assert_eq!(emptied.get("a").unwrap(), None);
  }
}
