use std::sync::Arc;

use tauri::State;

use crate::connectors::{
  connector_for, ApplyResult, Capability, Connection, DocBrowse, DocCollection, DocCount,
  DocDatabase, DocDetail, DocFindRequest, DocPage, DocQueryResult, IndexInfo, KeyDetail,
  KeyScanPage, KvBrowse, KvDatabases, LocalForward, QueryColumn, QueryResult, RowsChunk,
  SchemaSnapshot, SqlQuery, SqlSession, StreamSummary, TableChanges, TableRowsRequest,
};
use crate::error::Error;
use crate::export::{quote_ident, ExportFormat, ExportWriter};
use crate::profiles::{ConnectionInput, ConnectionProfile, ConnectorKind, ConnectorParams};
use crate::ssh::{self, SshTunnel, TunnelTarget};
use crate::tunnels::{TunnelInput, TunnelProfile};
use crate::{ActiveConnection, AppState, SessionEntry};

fn tunnel_secret_id(tunnel_id: &str) -> String {
  format!("tunnel:{tunnel_id}")
}

/// Resolve a profile's tunnel (if any); the returned forward tells the
/// connector where TCP actually goes while the profile keeps the logical host.
async fn open_tunnel(
  state: &AppState,
  profile: &ConnectionProfile,
) -> Result<Option<(SshTunnel, LocalForward)>, Error> {
  let Some(remote) = profile.params.remote() else {
    return Ok(None);
  };
  let Some(tunnel_id) = remote.tunnel_id else {
    return Ok(None);
  };
  let tunnel = state.tunnels.lock().unwrap().get(tunnel_id)?;
  let secret = state.secrets.get(&tunnel_secret_id(tunnel_id))?;
  let known_key = state
    .known_hosts
    .lock()
    .unwrap()
    .get(&tunnel.host, tunnel.port)
    .map(|raw| ssh::parse_public_key(&raw))
    .transpose()?;
  let opened = SshTunnel::open(
    &tunnel,
    secret.as_deref(),
    known_key,
    TunnelTarget {
      host: remote.host.to_string(),
      port: remote.port,
    },
  )
  .await?;
  let forward = LocalForward {
    port: opened.local_port,
  };
  Ok(Some((opened, forward)))
}

#[tauri::command]
#[specta::specta]
pub fn ping() -> Result<String, Error> {
  Ok("pong".to_string())
}

#[tauri::command]
#[specta::specta]
pub fn connector_capabilities(kind: ConnectorKind) -> Result<Vec<Capability>, Error> {
  Ok(connector_for(kind).capabilities().to_vec())
}

/// Ephemeral connect + health check; never touches the active connections.
#[tauri::command]
#[specta::specta]
pub async fn test_connection(
  state: State<'_, AppState>,
  input: ConnectionInput,
  existing_id: Option<String>,
) -> Result<(), Error> {
  let secret = match &input.password {
    Some(password) => Some(password.clone()),
    None => match &existing_id {
      Some(id) => state.secrets.get(id)?,
      None => None,
    },
  };
  let profile = ConnectionProfile {
    id: String::new(),
    name: input.name.clone(),
    env: input.env,
    group: input.group.clone(),
    agent_access: input.agent_access,
    params: input.params.clone(),
  };
  let opened = open_tunnel(&state, &profile).await?;
  let connection = connector_for(profile.params.kind())
    .connect(
      &profile,
      secret.as_deref(),
      opened.as_ref().map(|(_, f)| *f),
    )
    .await?;
  connection.health().await?;
  connection.close().await
}

#[tauri::command]
#[specta::specta]
pub async fn connect(state: State<'_, AppState>, id: String) -> Result<(), Error> {
  connect_impl(state.inner(), id).await
}

pub(crate) async fn connect_impl(state: &AppState, id: String) -> Result<(), Error> {
  let profile = state.profiles.lock().unwrap().get(&id)?;
  let secret = state.secrets.get(&id)?;
  let opened = open_tunnel(state, &profile).await?;
  let forward = opened.as_ref().map(|(_, f)| *f);
  let connection = connector_for(profile.params.kind())
    .connect(&profile, secret.as_deref(), forward)
    .await?;
  state.connections.lock().await.insert(
    id,
    ActiveConnection {
      connection: connection.into(),
      _tunnel: opened.map(|(tunnel, _)| tunnel),
    },
  );
  Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn disconnect(state: State<'_, AppState>, id: String) -> Result<(), Error> {
  let orphaned: Vec<Arc<dyn SqlSession>> = {
    let mut sessions = state.sessions.lock().await;
    let ids: Vec<String> = sessions
      .iter()
      .filter(|(_, entry)| entry.connection_id == id)
      .map(|(session_id, _)| session_id.clone())
      .collect();
    ids
      .iter()
      .filter_map(|session_id| sessions.remove(session_id))
      .map(|entry| entry.session)
      .collect()
  };
  for session in orphaned {
    let _ = session.close().await;
  }
  let active = state.connections.lock().await.remove(&id);
  match active {
    Some(active) => active.connection.close().await,
    None => Ok(()),
  }
}

#[tauri::command]
#[specta::specta]
pub async fn open_sql_session(
  state: State<'_, AppState>,
  connection_id: String,
) -> Result<String, Error> {
  let connection = active(&state, &connection_id).await?;
  let session = sql_surface(&connection)?.open_session().await?;
  let id = uuid::Uuid::new_v4().to_string();
  state.sessions.lock().await.insert(
    id.clone(),
    SessionEntry {
      connection_id,
      session: session.into(),
    },
  );
  Ok(id)
}

#[tauri::command]
#[specta::specta]
pub async fn run_session_query(
  state: State<'_, AppState>,
  id: String,
  sql: String,
) -> Result<QueryResult, Error> {
  session(&state, &id).await?.run_query(&sql).await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_session_query(state: State<'_, AppState>, id: String) -> Result<(), Error> {
  session(&state, &id).await?.cancel().await
}

#[tauri::command]
#[specta::specta]
pub async fn close_sql_session(state: State<'_, AppState>, id: String) -> Result<(), Error> {
  let entry = state.sessions.lock().await.remove(&id);
  match entry {
    Some(entry) => entry.session.close().await,
    None => Ok(()),
  }
}

// Clone the Arc out so queries never hold the map lock.
async fn session(state: &State<'_, AppState>, id: &str) -> Result<Arc<dyn SqlSession>, Error> {
  state
    .sessions
    .lock()
    .await
    .get(id)
    .map(|entry| entry.session.clone())
    .ok_or_else(|| Error::NotFound {
      message: format!("sql session {id} is not open"),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn active_connections(state: State<'_, AppState>) -> Result<Vec<String>, Error> {
  Ok(state.connections.lock().await.keys().cloned().collect())
}

#[tauri::command]
#[specta::specta]
pub async fn server_version(
  state: State<'_, AppState>,
  id: String,
) -> Result<Option<String>, Error> {
  Ok(active(&state, &id).await?.server_version())
}

#[tauri::command]
#[specta::specta]
pub async fn run_query(
  state: State<'_, AppState>,
  id: String,
  sql: String,
) -> Result<QueryResult, Error> {
  let connection = active(&state, &id).await?;
  sql_surface(&connection)?.run_query(&sql).await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_query(state: State<'_, AppState>, id: String) -> Result<(), Error> {
  let connection = active(&state, &id).await?;
  sql_surface(&connection)?.cancel().await
}

#[tauri::command]
#[specta::specta]
pub async fn table_rows(
  state: State<'_, AppState>,
  id: String,
  request: TableRowsRequest,
) -> Result<QueryResult, Error> {
  let connection = active(&state, &id).await?;
  sql_surface(&connection)?.table_rows(&request).await
}

#[tauri::command]
#[specta::specta]
pub async fn stream_table_rows(
  state: State<'_, AppState>,
  id: String,
  request: TableRowsRequest,
  channel: tauri::ipc::Channel<RowsChunk>,
) -> Result<StreamSummary, Error> {
  let connection = active(&state, &id).await?;
  sql_surface(&connection)?
    .stream_rows(&request, Box::new(move |chunk| channel.send(chunk).is_ok()))
    .await
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
  pub rows: f64,
}

/// Streams the full filtered/sorted table to a file; rows never enter the
/// webview. Cancel = `cancel_query` on the same connection; a canceled or
/// failed export removes the partial file.
#[tauri::command]
#[specta::specta]
pub async fn export_table_rows(
  state: State<'_, AppState>,
  id: String,
  request: TableRowsRequest,
  format: ExportFormat,
  path: String,
  channel: tauri::ipc::Channel<ExportProgress>,
) -> Result<StreamSummary, Error> {
  let kind = state.profiles.lock().unwrap().get(&id)?.params.kind();
  let connection = active(&state, &id).await?;
  crate::export::run_export(
    sql_surface(&connection)?,
    &request,
    format,
    kind,
    &path,
    move |rows| {
      let _ = channel.send(ExportProgress { rows: rows as f64 });
    },
  )
  .await
}

/// Materialized results (SQL editor); the grid path is `export_table_rows`.
#[tauri::command]
#[specta::specta]
pub fn export_statement(
  columns: Vec<QueryColumn>,
  rows: Vec<Vec<Option<String>>>,
  format: ExportFormat,
  kind: ConnectorKind,
  table: String,
  path: String,
) -> Result<(), Error> {
  let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
  let mut writer = ExportWriter::new(file, format, kind, columns, quote_ident(kind, &table))?;
  for row in &rows {
    writer.row(row)?;
  }
  writer.finish()?;
  Ok(())
}

/// Clipboard copy: same formats, returned as a string.
#[tauri::command]
#[specta::specta]
pub fn format_statement(
  columns: Vec<QueryColumn>,
  rows: Vec<Vec<Option<String>>>,
  format: ExportFormat,
  kind: ConnectorKind,
  table: String,
) -> Result<String, Error> {
  let mut out = Vec::new();
  let mut writer = ExportWriter::new(&mut out, format, kind, columns, quote_ident(kind, &table))?;
  for row in &rows {
    writer.row(row)?;
  }
  writer.finish()?;
  Ok(String::from_utf8(out).expect("formats emit utf-8"))
}

#[tauri::command]
#[specta::specta]
pub async fn apply_table_changes(
  state: State<'_, AppState>,
  id: String,
  changes: TableChanges,
) -> Result<ApplyResult, Error> {
  let connection = active(&state, &id).await?;
  sql_surface(&connection)?.apply_changes(&changes).await
}

#[tauri::command]
#[specta::specta]
pub async fn schema_snapshot(
  state: State<'_, AppState>,
  id: String,
) -> Result<SchemaSnapshot, Error> {
  let connection = active(&state, &id).await?;
  introspect_surface(&connection)?.schema_snapshot().await
}

#[tauri::command]
#[specta::specta]
pub async fn table_ddl(
  state: State<'_, AppState>,
  id: String,
  schema: String,
  table: String,
) -> Result<String, Error> {
  let connection = active(&state, &id).await?;
  introspect_surface(&connection)?
    .table_ddl(&schema, &table)
    .await
}

fn introspect_surface(
  connection: &Arc<dyn Connection>,
) -> Result<&dyn crate::connectors::Introspect, Error> {
  connection.introspect().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support schema introspection".to_string(),
  })
}

// Clone the Arc out so queries never hold the map lock.
pub(crate) async fn active(state: &AppState, id: &str) -> Result<Arc<dyn Connection>, Error> {
  state
    .connections
    .lock()
    .await
    .get(id)
    .map(|active| active.connection.clone())
    .ok_or_else(|| Error::NotFound {
      message: format!("connection {id} is not active"),
    })
}

fn sql_surface(connection: &Arc<dyn Connection>) -> Result<&dyn SqlQuery, Error> {
  connection.sql().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support sql queries".to_string(),
  })
}

fn kv_surface(connection: &Arc<dyn Connection>) -> Result<&dyn KvBrowse, Error> {
  connection.kv().ok_or_else(|| Error::Unsupported {
    message: "this connection does not browse keys".to_string(),
  })
}

fn doc_surface(connection: &Arc<dyn Connection>) -> Result<&dyn DocBrowse, Error> {
  connection.doc().ok_or_else(|| Error::Unsupported {
    message: "this connection does not browse documents".to_string(),
  })
}

#[tauri::command]
#[specta::specta]
pub async fn scan_keys(
  state: State<'_, AppState>,
  id: String,
  pattern: String,
  cursor: Option<String>,
  count: u32,
) -> Result<KeyScanPage, Error> {
  let connection = active(&state, &id).await?;
  kv_surface(&connection)?
    .scan_keys(&pattern, cursor.as_deref(), count)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn key_detail(
  state: State<'_, AppState>,
  id: String,
  key: String,
) -> Result<KeyDetail, Error> {
  let connection = active(&state, &id).await?;
  kv_surface(&connection)?.key_detail(&key).await
}

#[tauri::command]
#[specta::specta]
pub async fn kv_set_string(
  state: State<'_, AppState>,
  id: String,
  key: String,
  value: String,
) -> Result<(), Error> {
  let connection = active(&state, &id).await?;
  kv_surface(&connection)?.set_string(&key, &value).await
}

#[tauri::command]
#[specta::specta]
pub async fn kv_delete_key(
  state: State<'_, AppState>,
  id: String,
  key: String,
) -> Result<(), Error> {
  let connection = active(&state, &id).await?;
  kv_surface(&connection)?.delete_key(&key).await
}

#[tauri::command]
#[specta::specta]
pub async fn kv_set_ttl(
  state: State<'_, AppState>,
  id: String,
  key: String,
  ttl_ms: Option<f64>,
) -> Result<(), Error> {
  let connection = active(&state, &id).await?;
  kv_surface(&connection)?.set_ttl(&key, ttl_ms).await
}

#[tauri::command]
#[specta::specta]
pub async fn kv_run_command(
  state: State<'_, AppState>,
  id: String,
  command: String,
) -> Result<Vec<String>, Error> {
  let connection = active(&state, &id).await?;
  kv_surface(&connection)?.run_command(&command).await
}

#[tauri::command]
#[specta::specta]
pub async fn kv_databases(state: State<'_, AppState>, id: String) -> Result<KvDatabases, Error> {
  let connection = active(&state, &id).await?;
  kv_surface(&connection)?.databases().await
}

impl AppState {
  /// Reconnect on the target db and swap the active connection: a SELECT on the
  /// multiplexed socket would silently revert on reconnect.
  pub async fn select_kv_db(&self, id: &str, db: u32) -> Result<(), Error> {
    let mut profile = self.profiles.lock().unwrap().get(id)?;
    let ConnectorParams::Redis(params) = &mut profile.params else {
      return Err(Error::Unsupported {
        message: "database selection is a redis feature".to_string(),
      });
    };
    params.db = db;
    let secret = self.secrets.get(id)?;
    let forward = {
      let connections = self.connections.lock().await;
      let entry = connections.get(id).ok_or_else(|| Error::NotFound {
        message: format!("connection {id} is not active"),
      })?;
      entry._tunnel.as_ref().map(|tunnel| LocalForward {
        port: tunnel.local_port,
      })
    };
    let connection = connector_for(profile.params.kind())
      .connect(&profile, secret.as_deref(), forward)
      .await?;
    let mut connections = self.connections.lock().await;
    let Some(entry) = connections.get_mut(id) else {
      let _ = connection.close().await;
      return Err(Error::NotFound {
        message: format!("connection {id} is not active"),
      });
    };
    let old = std::mem::replace(&mut entry.connection, Arc::from(connection));
    drop(connections);
    old.close().await
  }
}

#[tauri::command]
#[specta::specta]
pub async fn kv_select_db(state: State<'_, AppState>, id: String, db: u32) -> Result<(), Error> {
  state.select_kv_db(&id, db).await
}

#[tauri::command]
#[specta::specta]
pub async fn doc_databases(
  state: State<'_, AppState>,
  id: String,
) -> Result<Vec<DocDatabase>, Error> {
  let connection = active(&state, &id).await?;
  doc_surface(&connection)?.databases().await
}

#[tauri::command]
#[specta::specta]
pub async fn doc_collections(
  state: State<'_, AppState>,
  id: String,
  db: String,
) -> Result<Vec<DocCollection>, Error> {
  let connection = active(&state, &id).await?;
  doc_surface(&connection)?.collections(&db).await
}

#[tauri::command]
#[specta::specta]
pub async fn doc_find(
  state: State<'_, AppState>,
  id: String,
  request: DocFindRequest,
) -> Result<DocPage, Error> {
  let connection = active(&state, &id).await?;
  doc_surface(&connection)?.find_docs(&request).await
}

#[tauri::command]
#[specta::specta]
pub async fn doc_detail(
  state: State<'_, AppState>,
  id: String,
  db: String,
  collection: String,
  doc_id: String,
) -> Result<DocDetail, Error> {
  let connection = active(&state, &id).await?;
  doc_surface(&connection)?
    .doc_detail(&db, &collection, &doc_id)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn doc_replace(
  state: State<'_, AppState>,
  id: String,
  db: String,
  collection: String,
  doc_id: String,
  doc: String,
) -> Result<(), Error> {
  let connection = active(&state, &id).await?;
  doc_surface(&connection)?
    .replace_doc(&db, &collection, &doc_id, &doc)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn doc_delete(
  state: State<'_, AppState>,
  id: String,
  db: String,
  collection: String,
  doc_id: String,
) -> Result<(), Error> {
  let connection = active(&state, &id).await?;
  doc_surface(&connection)?
    .delete_doc(&db, &collection, &doc_id)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn doc_indexes(
  state: State<'_, AppState>,
  id: String,
  db: String,
  collection: String,
) -> Result<Vec<IndexInfo>, Error> {
  let connection = active(&state, &id).await?;
  doc_surface(&connection)?.indexes(&db, &collection).await
}

#[tauri::command]
#[specta::specta]
pub async fn doc_count(
  state: State<'_, AppState>,
  id: String,
  db: String,
  collection: String,
  filter: Option<String>,
) -> Result<DocCount, Error> {
  let connection = active(&state, &id).await?;
  doc_surface(&connection)?
    .count_docs(&db, &collection, filter.as_deref())
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn doc_run_query(
  state: State<'_, AppState>,
  id: String,
  db: String,
  collection: String,
  source: String,
) -> Result<DocQueryResult, Error> {
  let connection = active(&state, &id).await?;
  doc_surface(&connection)?
    .run_query(&db, &collection, &source)
    .await
}

#[tauri::command]
#[specta::specta]
pub fn list_connections(state: State<'_, AppState>) -> Result<Vec<ConnectionProfile>, Error> {
  Ok(state.profiles.lock().unwrap().list())
}

#[tauri::command]
#[specta::specta]
pub fn get_connection(state: State<'_, AppState>, id: String) -> Result<ConnectionProfile, Error> {
  state.profiles.lock().unwrap().get(&id)
}

#[tauri::command]
#[specta::specta]
pub fn create_connection(
  state: State<'_, AppState>,
  input: ConnectionInput,
) -> Result<ConnectionProfile, Error> {
  let profile = state.profiles.lock().unwrap().create(&input)?;
  if let Some(password) = &input.password {
    // No orphan profile when the keychain is unavailable.
    if let Err(err) = state.secrets.set(&profile.id, password) {
      let _ = state.profiles.lock().unwrap().delete(&profile.id);
      return Err(err);
    }
  }
  Ok(profile)
}

#[tauri::command]
#[specta::specta]
pub fn update_connection(
  state: State<'_, AppState>,
  id: String,
  input: ConnectionInput,
) -> Result<ConnectionProfile, Error> {
  let profile = state.profiles.lock().unwrap().update(&id, &input)?;
  if let Some(password) = &input.password {
    state.secrets.set(&profile.id, password)?;
  }
  Ok(profile)
}

#[tauri::command]
#[specta::specta]
pub fn delete_connection(state: State<'_, AppState>, id: String) -> Result<(), Error> {
  state.profiles.lock().unwrap().delete(&id)?;
  state.secrets.delete(&id)
}

#[tauri::command]
#[specta::specta]
pub fn list_tunnels(state: State<'_, AppState>) -> Result<Vec<TunnelProfile>, Error> {
  Ok(state.tunnels.lock().unwrap().list())
}

#[tauri::command]
#[specta::specta]
pub fn get_tunnel(state: State<'_, AppState>, id: String) -> Result<TunnelProfile, Error> {
  state.tunnels.lock().unwrap().get(&id)
}

#[tauri::command]
#[specta::specta]
pub fn create_tunnel(
  state: State<'_, AppState>,
  input: TunnelInput,
) -> Result<TunnelProfile, Error> {
  let tunnel = state.tunnels.lock().unwrap().create(&input)?;
  if let Some(secret) = &input.secret {
    // No orphan tunnel when the keychain is unavailable.
    if let Err(err) = state.secrets.set(&tunnel_secret_id(&tunnel.id), secret) {
      let _ = state.tunnels.lock().unwrap().delete(&tunnel.id);
      return Err(err);
    }
  }
  Ok(tunnel)
}

#[tauri::command]
#[specta::specta]
pub fn update_tunnel(
  state: State<'_, AppState>,
  id: String,
  input: TunnelInput,
) -> Result<TunnelProfile, Error> {
  let tunnel = state.tunnels.lock().unwrap().update(&id, &input)?;
  if let Some(secret) = &input.secret {
    state.secrets.set(&tunnel_secret_id(&tunnel.id), secret)?;
  }
  Ok(tunnel)
}

#[tauri::command]
#[specta::specta]
pub fn delete_tunnel(state: State<'_, AppState>, id: String) -> Result<(), Error> {
  let used_by: Vec<String> = state
    .profiles
    .lock()
    .unwrap()
    .list()
    .into_iter()
    .filter(|p| p.params.remote().and_then(|remote| remote.tunnel_id) == Some(id.as_str()))
    .map(|p| p.name)
    .collect();
  if !used_by.is_empty() {
    return Err(Error::Storage {
      message: format!("tunnel is used by {}", used_by.join(", ")),
    });
  }
  state.tunnels.lock().unwrap().delete(&id)?;
  state.secrets.delete(&tunnel_secret_id(&id))
}

/// Ephemeral tunnel bring-up: validates the host key and the credentials
/// without touching a database (no channel is opened until a client connects).
#[tauri::command]
#[specta::specta]
pub async fn test_tunnel(
  state: State<'_, AppState>,
  input: TunnelInput,
  existing_id: Option<String>,
) -> Result<(), Error> {
  let secret = match &input.secret {
    Some(secret) => Some(secret.clone()),
    None => match &existing_id {
      Some(id) => state.secrets.get(&tunnel_secret_id(id))?,
      None => None,
    },
  };
  let tunnel = TunnelProfile {
    id: String::new(),
    name: input.name.clone(),
    host: input.host.clone(),
    port: input.port,
    user: input.user.clone(),
    auth: input.auth.clone(),
  };
  let known_key = state
    .known_hosts
    .lock()
    .unwrap()
    .get(&tunnel.host, tunnel.port)
    .map(|raw| ssh::parse_public_key(&raw))
    .transpose()?;
  SshTunnel::open(
    &tunnel,
    secret.as_deref(),
    known_key,
    TunnelTarget {
      host: "127.0.0.1".to_string(),
      port: 1,
    },
  )
  .await
  .map(|_| ())
}

#[tauri::command]
#[specta::specta]
pub fn default_ssh_keys() -> Result<Vec<String>, Error> {
  Ok(ssh::default_key_paths())
}

#[tauri::command]
#[specta::specta]
pub fn trust_host_key(
  state: State<'_, AppState>,
  host: String,
  port: u16,
  key: String,
) -> Result<(), Error> {
  ssh::parse_public_key(&key)?;
  state.known_hosts.lock().unwrap().trust(&host, port, &key)
}

#[tauri::command]
#[specta::specta]
pub async fn mcp_status(state: State<'_, AppState>) -> Result<crate::mcp::McpStatus, Error> {
  crate::mcp::status(state.inner()).await
}

#[tauri::command]
#[specta::specta]
pub async fn mcp_start(
  app: tauri::AppHandle,
  state: State<'_, AppState>,
  port: Option<u16>,
) -> Result<crate::mcp::McpStatus, Error> {
  crate::mcp::start(app, port.unwrap_or(crate::mcp::DEFAULT_PORT)).await?;
  crate::mcp::status(state.inner()).await
}

#[tauri::command]
#[specta::specta]
pub async fn mcp_stop(state: State<'_, AppState>) -> Result<(), Error> {
  crate::mcp::stop(state.inner()).await
}

#[tauri::command]
#[specta::specta]
pub async fn mcp_regenerate_token(
  state: State<'_, AppState>,
) -> Result<crate::mcp::McpStatus, Error> {
  crate::mcp::regenerate_token(state.inner()).await?;
  crate::mcp::status(state.inner()).await
}

#[tauri::command]
#[specta::specta]
pub async fn mcp_audit_log(
  state: State<'_, AppState>,
  limit: Option<u32>,
) -> Result<Vec<crate::mcp::AuditEntry>, Error> {
  crate::mcp::audit_log(state.inner(), limit.unwrap_or(200) as usize)
}

#[tauri::command]
#[specta::specta]
pub async fn mcp_resolve_approval(
  state: State<'_, AppState>,
  id: String,
  approved: bool,
) -> Result<(), Error> {
  crate::mcp::resolve_approval(state.inner(), &id, approved).await
}
