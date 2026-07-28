use serde::Serialize;
use specta::Type;

/// Normalized error shape crossing the IPC boundary.
#[derive(Debug, thiserror::Error, Serialize, Type)]
#[serde(
  tag = "kind",
  rename_all = "kebab-case",
  rename_all_fields = "camelCase"
)]
pub enum Error {
  #[error("{message}")]
  NotFound { message: String },
  #[error("{message}")]
  Storage { message: String },
  #[error("{message}")]
  Secret { message: String },
  #[error("{message}")]
  Unsupported { message: String },
  #[error("{message}")]
  Database { message: String },
}

impl From<tokio_postgres::Error> for Error {
  fn from(err: tokio_postgres::Error) -> Self {
    // db_error carries the useful message; err.to_string() alone is often just "db error".
    let message = err
      .as_db_error()
      .map(|db| db.message().to_string())
      .unwrap_or_else(|| err.to_string());
    Error::Database { message }
  }
}

impl From<std::io::Error> for Error {
  fn from(err: std::io::Error) -> Self {
    Error::Storage {
      message: err.to_string(),
    }
  }
}

impl From<serde_json::Error> for Error {
  fn from(err: serde_json::Error) -> Self {
    Error::Storage {
      message: err.to_string(),
    }
  }
}

impl From<keyring::Error> for Error {
  fn from(err: keyring::Error) -> Self {
    Error::Secret {
      message: err.to_string(),
    }
  }
}
