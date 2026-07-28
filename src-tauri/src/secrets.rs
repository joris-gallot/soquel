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
}
