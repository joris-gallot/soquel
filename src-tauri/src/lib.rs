use std::sync::Mutex;

use tauri::Manager;

use crate::profiles::ProfileStore;
use crate::secrets::{KeyringStore, SecretStore};

mod commands;
mod connectors;
mod error;
mod profiles;
mod secrets;

pub struct AppState {
    pub profiles: Mutex<ProfileStore>,
    pub secrets: Box<dyn SecretStore>,
}

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::new()
        .commands(tauri_specta::collect_commands![
            commands::ping,
            commands::connector_capabilities,
            commands::list_connections,
            commands::get_connection,
            commands::create_connection,
            commands::update_connection,
            commands::delete_connection,
        ])
        .error_handling(tauri_specta::ErrorHandlingMode::Result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    builder
        .export(
            specta_typescript::Typescript::default(),
            "../packages/app/src/lib/bindings.ts",
        )
        .expect("failed to export typescript bindings");

    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            let store = ProfileStore::load(app.path().app_data_dir()?.join("connections.json"))?;
            app.manage(AppState {
                profiles: Mutex::new(store),
                secrets: Box::new(KeyringStore),
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    // Regenerates the bindings without launching the app.
    #[test]
    fn export_typescript_bindings() {
        super::specta_builder()
            .export(
                specta_typescript::Typescript::default(),
                "../packages/app/src/lib/bindings.ts",
            )
            .expect("failed to export typescript bindings");
    }
}
