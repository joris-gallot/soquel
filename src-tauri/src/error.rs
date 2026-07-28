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
    // Unconstructed until the first fallible command lands.
    #[allow(dead_code)]
    #[error("{message}")]
    Internal { message: String },
}
