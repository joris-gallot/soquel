//! libpq's password file: one `host:port:database:user:password` per line.

use crate::error::Error;
use crate::profiles::{ConnectorParams, CredentialSource, Env, SqlServerParams, SslMode};
use crate::transfer::{ImportBundle, IncomingConnection};

const WILDCARD: &str = "*";

/// A line of the file, fields already unescaped.
struct Entry {
  host: String,
  port: String,
  database: String,
  user: String,
  password: String,
}

/// Splits on unescaped colons: a password may hold `\:` and `\\`.
fn split_fields(line: &str) -> Vec<String> {
  let mut fields = vec![String::new()];
  let mut escaped = false;
  for character in line.chars() {
    let current = fields.last_mut().expect("the vec starts with one field");
    match character {
      _ if escaped => {
        current.push(character);
        escaped = false;
      }
      '\\' => escaped = true,
      ':' => fields.push(String::new()),
      _ => current.push(character),
    }
  }
  fields
}

fn parse_line(line: &str) -> Option<Entry> {
  let line = line.trim();
  if line.is_empty() || line.starts_with('#') {
    return None;
  }
  let fields = split_fields(line);
  // Five fields exactly: a short line is not a rule libpq would honour either.
  let [host, port, database, user, password] = <[String; 5]>::try_from(fields).ok()?;
  Some(Entry {
    host,
    port,
    database,
    user,
    password,
  })
}

/// What keeps a line from describing a reachable server. A wildcard is a
/// matching rule, not an address: inventing one would connect somewhere the
/// file never named.
fn problem(entry: &Entry) -> Option<&'static str> {
  if entry.host == WILDCARD {
    return Some("the host is a wildcard");
  }
  if entry.port == WILDCARD {
    return Some("the port is a wildcard");
  }
  if entry.database == WILDCARD {
    return Some("the database is a wildcard");
  }
  if entry.port.parse::<u16>().is_err() {
    return Some("the port is not a number");
  }
  if entry.user == WILDCARD || entry.user.is_empty() {
    return Some("the user is a wildcard");
  }
  None
}

/// Wildcard lines ride along as unusable entries: the preview names them and
/// the engine refuses the batch, rather than the file half-importing in silence.
fn unusable(entry: &Entry, reason: &str) -> IncomingConnection {
  IncomingConnection {
    name: format!("{}:{}:{}", entry.host, entry.port, entry.database),
    env: Env::Dev,
    group: None,
    credential: CredentialSource::Keychain,
    // Empty host: `validate` in the engine reads it back as the same problem.
    params: ConnectorParams::Postgres(SqlServerParams {
      host: String::new(),
      port: 0,
      database: entry.database.clone(),
      user: entry.user.clone(),
      ssl_mode: SslMode::Prefer,
      ssl_root_cert: None,
      tunnel_id: None,
    }),
    tunnel_ref: None,
    secret: None,
    problem: Some(reason.to_string()),
  }
}

fn connection(entry: Entry) -> IncomingConnection {
  let port = entry.port.parse().expect("checked by problem()");
  IncomingConnection {
    name: format!("{}@{}:{}/{}", entry.user, entry.host, port, entry.database),
    env: Env::Dev,
    group: None,
    credential: CredentialSource::Keychain,
    params: ConnectorParams::Postgres(SqlServerParams {
      host: entry.host,
      port,
      database: entry.database,
      user: entry.user,
      ssl_mode: SslMode::Prefer,
      ssl_root_cert: None,
      tunnel_id: None,
    }),
    tunnel_ref: None,
    secret: (!entry.password.is_empty()).then_some(entry.password),
    problem: None,
  }
}

pub fn read(path: &std::path::Path) -> Result<ImportBundle, Error> {
  let raw = std::fs::read_to_string(path)?;
  Ok(ImportBundle {
    connections: raw
      .lines()
      .filter_map(parse_line)
      .map(|entry| match problem(&entry) {
        Some(reason) => unusable(&entry, reason),
        None => connection(entry),
      })
      .collect(),
    tunnels: Vec::new(),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn bundle(contents: &str) -> ImportBundle {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".pgpass");
    std::fs::write(&path, contents).unwrap();
    read(&path).unwrap()
  }

  #[test]
  fn a_concrete_line_becomes_a_named_connection() {
    let bundle = bundle("db.prod:5432:shop:app:s3cret\n");
    let [connection] = <[_; 1]>::try_from(bundle.connections).unwrap();

    assert_eq!(connection.name, "app@db.prod:5432/shop");
    assert_eq!(connection.secret.as_deref(), Some("s3cret"));
    assert_eq!(connection.problem, None);
    let params = connection.params.sql_server().unwrap();
    assert_eq!(params.host, "db.prod");
    assert_eq!(params.port, 5432);
    assert_eq!(params.database, "shop");
    assert_eq!(params.user, "app");
  }

  #[test]
  fn comments_blank_lines_and_short_lines_are_not_entries() {
    let bundle = bundle("# a comment\n\n  \nhost:5432:db\ndb.prod:5432:shop:app:s3cret\n");
    assert_eq!(bundle.connections.len(), 1);
  }

  #[test]
  fn a_colon_or_a_backslash_can_live_inside_a_password() {
    let bundle = bundle(
      r"db.prod:5432:shop:app:pass\:with\\colon"
        .to_string()
        .as_str(),
    );
    let [connection] = <[_; 1]>::try_from(bundle.connections).unwrap();
    assert_eq!(connection.secret.as_deref(), Some(r"pass:with\colon"));
  }

  #[test]
  fn a_wildcard_line_is_carried_as_unusable_with_its_reason() {
    for (line, reason) in [
      ("*:*:*:postgres:secret", "the host is a wildcard"),
      ("db.prod:*:shop:app:secret", "the port is a wildcard"),
      ("db.prod:5432:*:app:secret", "the database is a wildcard"),
      ("db.prod:5432:shop:*:secret", "the user is a wildcard"),
      ("db.prod:nope:shop:app:secret", "the port is not a number"),
    ] {
      let bundle = bundle(&format!("{line}\n"));
      let [connection] = <[_; 1]>::try_from(bundle.connections).unwrap();
      assert_eq!(connection.problem.as_deref(), Some(reason), "{line}");
      // Nothing usable, so nothing to leak either.
      assert_eq!(connection.secret, None, "{line}");
    }
  }

  #[test]
  fn a_line_without_a_password_still_describes_a_connection() {
    let bundle = bundle("db.prod:5432:shop:app:\n");
    let [connection] = <[_; 1]>::try_from(bundle.connections).unwrap();
    assert_eq!(connection.secret, None);
    assert_eq!(connection.problem, None);
  }
}
