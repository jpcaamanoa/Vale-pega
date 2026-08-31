mod commands;
mod db;
mod repositories;
mod security;
mod services;

use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;

use security::VaultSession;

/// Cada cuánto se revisa si corresponde bloquear por inactividad. No tiene
/// que ser muy fino: el bloqueo automático se dispara quince minutos
/// (configurable) después de la última actividad, así que revisar cada diez
/// segundos es más que suficiente.
const AUTO_LOCK_TICK_INTERVAL: Duration = Duration::from_secs(10);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      let vault_dir = app.path().app_data_dir()?.join("vault");
      std::fs::create_dir_all(&vault_dir)?;

      let vault_session: Arc<VaultSession> = Arc::new(VaultSession::new(&vault_dir));
      app.manage(vault_session.clone());

      // Bloqueo automático por inactividad (Fase 1.4). Deliberadamente NO
      // reacciona a que el sistema operativo se suspenda o bloquee la
      // pantalla — eso queda fuera de alcance de esta fase, ver
      // `security::session::VaultSession::tick_auto_lock`.
      tauri::async_runtime::spawn(async move {
        loop {
          tokio::time::sleep(AUTO_LOCK_TICK_INTERVAL).await;
          vault_session.tick_auto_lock();
        }
      });

      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      commands::app_info,
      commands::vault_status,
      commands::evaluate_password_strength,
      commands::begin_vault_creation,
      commands::confirm_vault_creation,
      commands::cancel_vault_creation,
      commands::unlock_vault,
      commands::recover_vault_access,
      commands::change_vault_password,
      commands::lock_vault,
      commands::record_vault_activity,
      commands::set_auto_lock_timeout_seconds,
      commands::create_patient,
      commands::get_patient,
      commands::list_patients,
      commands::list_archived_patients,
      commands::update_patient,
      commands::archive_patient,
      commands::restore_patient,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
