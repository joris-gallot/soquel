//! Where connections come in from. A source only has to find its file and
//! produce an [`ImportBundle`]; the engine does the rest.

pub mod pg_service;
pub mod pgpass;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::Error;
use crate::transfer::{file, ImportBundle};

/// A source pinned to a file. The path travels with it: the UI shows what it
/// would read, and a test points somewhere other than the real home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
  tag = "kind",
  rename_all = "kebab-case",
  rename_all_fields = "camelCase"
)]
pub enum ImportSource {
  SoquelFile { path: String },
  Pgpass { path: String },
  PgService { path: String },
}

/// A source's own kind, without a path: what `scan` reports on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ImportSourceKind {
  Pgpass,
  PgService,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportSourceSummary {
  pub kind: ImportSourceKind,
  pub source: ImportSource,
  pub path: String,
  /// How many entries the file holds, `None` when it could not be read.
  pub entries: Option<u32>,
  /// Why it could not be read; absent when there is nothing to say.
  pub problem: Option<String>,
}

impl ImportSource {
  pub fn path(&self) -> &str {
    match self {
      Self::SoquelFile { path } | Self::Pgpass { path } | Self::PgService { path } => path,
    }
  }

  /// A soquel file may be encrypted, so reading it can legitimately come back
  /// without a bundle: the caller asks again with the passphrase.
  pub fn read(&self, passphrase: Option<&str>) -> Result<file::ReadFile, Error> {
    let path = std::path::Path::new(self.path());
    match self {
      Self::SoquelFile { .. } => file::read(path, passphrase),
      Self::Pgpass { .. } => Ok(plain(pgpass::read(path)?)),
      Self::PgService { .. } => Ok(plain(pg_service::read(path)?)),
    }
  }
}

fn plain(bundle: ImportBundle) -> file::ReadFile {
  file::ReadFile {
    encrypted: false,
    bundle: Some(bundle),
  }
}

/// libpq's own overrides first, then the documented default. `SOQUEL_IMPORT_HOME`
/// stands in for the home directory so a test never reads the real one.
fn home() -> std::path::PathBuf {
  if let Ok(home) = std::env::var("SOQUEL_IMPORT_HOME") {
    return std::path::PathBuf::from(home);
  }
  #[cfg(unix)]
  let key = "HOME";
  #[cfg(windows)]
  let key = "APPDATA";
  std::env::var(key).map(Into::into).unwrap_or_default()
}

fn default_path(kind: ImportSourceKind) -> std::path::PathBuf {
  let (env_var, file) = match kind {
    ImportSourceKind::Pgpass => ("PGPASSFILE", ".pgpass"),
    ImportSourceKind::PgService => ("PGSERVICEFILE", ".pg_service.conf"),
  };
  std::env::var(env_var)
    .map(std::path::PathBuf::from)
    .unwrap_or_else(|_| home().join(file))
}

/// What this machine has to offer. A source that is missing or unreadable is
/// reported, never fatal: one broken file must not hide the others.
pub fn scan() -> Vec<ImportSourceSummary> {
  [ImportSourceKind::Pgpass, ImportSourceKind::PgService]
    .into_iter()
    .map(|kind| {
      let path = default_path(kind);
      let source = match kind {
        ImportSourceKind::Pgpass => ImportSource::Pgpass {
          path: path.to_string_lossy().into_owned(),
        },
        ImportSourceKind::PgService => ImportSource::PgService {
          path: path.to_string_lossy().into_owned(),
        },
      };
      // exists, not is_file: a directory where the file belongs is a broken
      // setup worth naming, not something to report as absent.
      let (entries, problem) = match path.exists() {
        false => (None, None),
        true => match source.read(None) {
          Ok(read) => (
            read.bundle.map(|bundle| bundle.connections.len() as u32),
            None,
          ),
          Err(err) => (None, Some(err.to_string())),
        },
      };
      ImportSourceSummary {
        kind,
        source,
        path: path.to_string_lossy().into_owned(),
        entries,
        problem,
      }
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The scan reads whatever `SOQUEL_IMPORT_HOME` points at, so a test never
  /// touches the real home. Serialized: env vars are process-wide.
  fn with_home<T>(files: &[(&str, &str)], body: impl FnOnce() -> T) -> T {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let dir = tempfile::tempdir().unwrap();
    for (name, contents) in files {
      std::fs::write(dir.path().join(name), contents).unwrap();
    }
    std::env::set_var("SOQUEL_IMPORT_HOME", dir.path());
    std::env::remove_var("PGPASSFILE");
    std::env::remove_var("PGSERVICEFILE");
    let outcome = body();
    std::env::remove_var("SOQUEL_IMPORT_HOME");
    outcome
  }

  #[test]
  fn the_scan_counts_what_it_finds() {
    let summaries = with_home(
      &[
        (".pgpass", "db.prod:5432:shop:app:s3cret\n*:*:*:app:other\n"),
        (".pg_service.conf", "[prod]\nhost=db.prod\n"),
      ],
      scan,
    );
    let pgpass = &summaries[0];
    assert_eq!(pgpass.kind, ImportSourceKind::Pgpass);
    // Both lines are entries: the wildcard one arrives flagged, not dropped.
    assert_eq!(pgpass.entries, Some(2));
    assert_eq!(pgpass.problem, None);
    assert_eq!(summaries[1].entries, Some(1));
  }

  #[test]
  fn libpq_own_overrides_win_over_the_home() {
    let elsewhere = tempfile::tempdir().unwrap();
    let pgpass = elsewhere.path().join("elsewhere.pgpass");
    std::fs::write(&pgpass, "db.prod:5432:shop:app:s3cret\n").unwrap();

    // The home holds nothing: only PGPASSFILE says where to look.
    let summaries = with_home(&[], || {
      std::env::set_var("PGPASSFILE", &pgpass);
      let summaries = scan();
      std::env::remove_var("PGPASSFILE");
      summaries
    });
    assert_eq!(summaries[0].path, pgpass.to_string_lossy());
    assert_eq!(summaries[0].entries, Some(1));
  }

  #[test]
  fn a_missing_file_is_reported_not_an_error() {
    let summaries = with_home(&[], scan);
    assert!(summaries.iter().all(|summary| summary.entries.is_none()));
    assert!(summaries.iter().all(|summary| summary.problem.is_none()));
    // The path still shows: the UI can say where it looked.
    assert!(summaries[0].path.ends_with(".pgpass"));
  }

  #[test]
  fn one_unreadable_source_does_not_hide_the_others() {
    let summaries = with_home(&[(".pg_service.conf", "[prod]\nhost=db.prod\n")], || {
      // A directory where a file is expected: reading it fails.
      let home = std::env::var("SOQUEL_IMPORT_HOME").unwrap();
      std::fs::create_dir(std::path::Path::new(&home).join(".pgpass")).unwrap();
      scan()
    });
    assert_eq!(summaries[0].entries, None);
    assert!(summaries[0].problem.is_some());
    assert_eq!(summaries[1].entries, Some(1));
  }

  /// pgpass is usually 0600, and one owned by another user is a real case.
  #[cfg(unix)]
  #[test]
  fn a_file_it_may_not_read_says_so() {
    use std::os::unix::fs::PermissionsExt;

    let summaries = with_home(&[(".pgpass", "db.prod:5432:shop:app:s3cret\n")], || {
      let home = std::env::var("SOQUEL_IMPORT_HOME").unwrap();
      let path = std::path::Path::new(&home).join(".pgpass");
      std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
      scan()
    });
    // Running as root reads it anyway: only assert when the mode bites.
    if summaries[0].entries.is_none() {
      assert!(summaries[0].problem.is_some());
    }
  }
}
