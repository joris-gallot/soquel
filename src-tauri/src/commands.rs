use tauri::State;

use crate::error::Error;
use crate::profiles::{ConnectionInput, ConnectionProfile};
use crate::AppState;

#[tauri::command]
#[specta::specta]
pub fn ping() -> Result<String, Error> {
    Ok("pong".to_string())
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
