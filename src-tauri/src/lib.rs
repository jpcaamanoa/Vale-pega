mod backup;
mod calendar;
mod commands;
mod db;
mod geo;
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
    .plugin(tauri_plugin_dialog::init())
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

      // Antes de que `VaultSession` lea el estado del disco (línea
      // siguiente): recuperar de una posible interrupción a mitad de un
      // `restore_backup` (Fase 10) — ver
      // `backup::service::run_startup_recovery`. No hace nada en el caso
      // normal.
      backup::service::run_startup_recovery(&vault_dir);

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
      commands::get_geographic_statistics,
      commands::create_appointment,
      commands::get_appointment,
      commands::list_appointments,
      commands::list_archived_appointments,
      commands::check_overlap,
      commands::update_appointment,
      commands::cancel_appointment,
      commands::archive_appointment,
      commands::restore_appointment,
      commands::google_connection_status,
      commands::save_google_credentials,
      commands::begin_google_auth,
      commands::list_google_calendars,
      commands::select_google_calendar,
      commands::disconnect_google_calendar,
      commands::retry_appointment_sync,
      commands::create_session,
      commands::get_session,
      commands::get_session_for_appointment,
      commands::list_sessions,
      commands::list_archived_sessions,
      commands::update_session_metadata,
      commands::archive_session,
      commands::restore_session,
      commands::get_current_note,
      commands::list_note_history,
      commands::autosave_note_draft,
      commands::close_current_note,
      commands::create_new_note_version,
      commands::get_sessions_this_month_count,
      commands::create_goal,
      commands::get_goal,
      commands::list_goals,
      commands::list_archived_goals,
      commands::update_goal,
      commands::archive_goal,
      commands::restore_goal,
      commands::list_goal_indicators,
      commands::create_goal_indicator,
      commands::update_goal_indicator,
      commands::delete_goal_indicator,
      commands::link_session_goal,
      commands::unlink_session_goal,
      commands::update_session_goal_progress,
      commands::list_goals_for_session,
      commands::list_sessions_for_goal,
      commands::list_available_goals_for_session,
      commands::get_clinical_profile,
      commands::create_clinical_profile,
      commands::update_clinical_profile,
      commands::create_payment,
      commands::get_payment,
      commands::list_payments,
      commands::list_archived_payments,
      commands::update_payment,
      commands::archive_payment,
      commands::restore_payment,
      commands::get_payment_dashboard_summary,
      commands::create_prep_note,
      commands::get_prep_note,
      commands::list_prep_notes,
      commands::list_pending_prep_notes,
      commands::update_prep_note,
      commands::set_prep_note_status,
      commands::create_therapy_task,
      commands::get_therapy_task,
      commands::list_therapy_tasks,
      commands::list_archived_therapy_tasks,
      commands::list_pending_therapy_tasks,
      commands::update_therapy_task,
      commands::review_therapy_task,
      commands::archive_therapy_task,
      commands::restore_therapy_task,
      commands::get_pending_therapy_task_count,
      commands::create_treatment_episode,
      commands::get_treatment_episode,
      commands::list_treatment_episodes,
      commands::list_archived_treatment_episodes,
      commands::set_treatment_episode_status,
      commands::archive_treatment_episode,
      commands::restore_treatment_episode,
      commands::get_episode_clinical_profile,
      commands::create_episode_clinical_profile,
      commands::update_episode_clinical_profile,
      commands::close_treatment_episode,
      commands::revert_episode_closure,
      commands::get_active_episode_closure,
      commands::list_episode_closure_history,
      commands::list_upcoming_episode_sessions,
      commands::list_episode_sessions,
      commands::list_episode_goals,
      commands::list_pending_or_partial_therapy_tasks,
      commands::create_backup,
      commands::inspect_backup,
      commands::restore_backup,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
