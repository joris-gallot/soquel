use tauri::State;

use crate::connectors::{connector_for, Capability};
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
