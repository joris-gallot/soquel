use std::sync::Arc;

use tauri::State;

use crate::connectors::{
  connector_for, Capability, Connection, QueryResult, SchemaSnapshot, SqlQuery,
};
use crate::error::Error;
use crate::profiles::{ConnectionInput, ConnectionProfile, ConnectorKind};
use crate::AppState;

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
    kind: input.kind,
    host: input.host.clone(),
    port: input.port,
    database: input.database.clone(),
    user: input.user.clone(),
  };
  let connection = connector_for(input.kind)
    .connect(&profile, secret.as_deref())
    .await?;
  connection.health().await?;
  connection.close().await
}

#[tauri::command]
#[specta::specta]
pub async fn connect(state: State<'_, AppState>, id: String) -> Result<(), Error> {
  let profile = state.profiles.lock().unwrap().get(&id)?;
  let secret = state.secrets.get(&id)?;
  let connection = connector_for(profile.kind)
    .connect(&profile, secret.as_deref())
    .await?;
  state.connections.lock().await.insert(id, connection.into());
  Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn disconnect(state: State<'_, AppState>, id: String) -> Result<(), Error> {
  let connection = state.connections.lock().await.remove(&id);
  match connection {
    Some(connection) => connection.close().await,
    None => Ok(()),
  }
}

#[tauri::command]
#[specta::specta]
pub async fn active_connections(state: State<'_, AppState>) -> Result<Vec<String>, Error> {
  Ok(state.connections.lock().await.keys().cloned().collect())
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
pub async fn schema_snapshot(
  state: State<'_, AppState>,
  id: String,
) -> Result<SchemaSnapshot, Error> {
  let connection = active(&state, &id).await?;
  let introspect = connection.introspect().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support schema introspection".to_string(),
  })?;
  introspect.schema_snapshot().await
}

// Clone the Arc out so queries never hold the map lock.
async fn active(state: &State<'_, AppState>, id: &str) -> Result<Arc<dyn Connection>, Error> {
  state
    .connections
    .lock()
    .await
    .get(id)
    .cloned()
    .ok_or_else(|| Error::NotFound {
      message: format!("connection {id} is not active"),
    })
}

fn sql_surface(connection: &Arc<dyn Connection>) -> Result<&dyn SqlQuery, Error> {
  connection.sql().ok_or_else(|| Error::Unsupported {
    message: "this connection does not support sql queries".to_string(),
  })
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
    state.secrets.set(&profile.id, password)?;
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
