use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tauri::Manager;

use crate::connectors::Connection;
use crate::known_hosts::KnownHostsStore;
use crate::profiles::ProfileStore;
use crate::secrets::{FileStore, InMemoryStore, KeyringStore, SecretStore};
use crate::ssh::SshTunnel;
use crate::tunnels::TunnelStore;

mod commands;
mod connectors;
mod error;
mod known_hosts;
mod postgres;
mod profiles;
mod secrets;
mod ssh;
mod tunnels;

/// A connected database plus the tunnel carrying it: dropped together.
pub struct ActiveConnection {
  pub connection: Arc<dyn Connection>,
  pub _tunnel: Option<SshTunnel>,
}

pub struct AppState {
  pub profiles: Mutex<ProfileStore>,
  pub tunnels: Mutex<TunnelStore>,
  pub known_hosts: Mutex<KnownHostsStore>,
  pub secrets: Box<dyn SecretStore>,
  pub connections: tokio::sync::Mutex<HashMap<String, ActiveConnection>>,
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
      commands::test_connection,
      commands::connect,
      commands::disconnect,
      commands::active_connections,
      commands::run_query,
      commands::cancel_query,
      commands::table_rows,
      commands::schema_snapshot,
      commands::list_tunnels,
      commands::get_tunnel,
      commands::create_tunnel,
      commands::update_tunnel,
      commands::delete_tunnel,
      commands::test_tunnel,
      commands::default_ssh_keys,
      commands::trust_host_key,
    ])
    .error_handling(tauri_specta::ErrorHandlingMode::Result)
}

// Anchored to the crate dir: the binary's cwd is wherever the runner spawned it.
#[cfg(debug_assertions)]
const BINDINGS_PATH: &str = concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/../packages/app/src/lib/bindings.ts"
);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let builder = specta_builder();

  #[cfg(debug_assertions)]
  builder
    .export(specta_typescript::Typescript::default(), BINDINGS_PATH)
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
      // SOQUEL_DATA_DIR isolates e2e runs from the real app data.
      let data_dir = match std::env::var("SOQUEL_DATA_DIR") {
        Ok(dir) => std::path::PathBuf::from(dir),
        Err(_) => app.path().app_data_dir()?,
      };
      let store = ProfileStore::load(data_dir.join("connections.json"))?;
      let tunnels = TunnelStore::load(data_dir.join("tunnels.json"))?;
      let known_hosts = KnownHostsStore::load(data_dir.join("known_hosts.json"))?;
      // Keychain-less environments: e2e/CI (ephemeral) and WSL dev (plaintext file, opt-in).
      let secrets: Box<dyn SecretStore> = if std::env::var("SOQUEL_EPHEMERAL_SECRETS").is_ok() {
        Box::new(InMemoryStore::default())
      } else if std::env::var("SOQUEL_INSECURE_FILE_SECRETS").is_ok() {
        Box::new(FileStore::load(data_dir.join("secrets.json"))?)
      } else {
        Box::new(KeyringStore)
      };
      app.manage(AppState {
        profiles: Mutex::new(store),
        tunnels: Mutex::new(tunnels),
        known_hosts: Mutex::new(known_hosts),
        secrets,
        connections: tokio::sync::Mutex::new(HashMap::new()),
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
        super::BINDINGS_PATH,
      )
      .expect("failed to export typescript bindings");
  }
}
