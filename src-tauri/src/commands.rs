use crate::error::Error;

#[tauri::command]
#[specta::specta]
pub fn ping() -> Result<String, Error> {
    Ok("pong".to_string())
}
