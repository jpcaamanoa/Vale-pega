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
    // MULTI-1 (hardening pre-RC, Fase 17): debe ser el PRIMER plugin registrado. Los plugins se
    // inicializan síncronamente en el orden de registro, dentro de `Builder::build()` — es decir,
    // antes de que corra el `.setup(...)` de más abajo, que abre `VaultSession`, ejecuta
    // `run_startup_recovery` y `sweep_stale_temp_files` (ver
    // `tauri::App::manager::initialize_plugins`, llamado desde `Builder::build` antes de que
    // exista siquiera la oportunidad de ejecutar el `.setup()` del usuario). Si el proceso que
    // arranca es una segunda instancia, el plugin reenvía sus argumentos a la instancia ya viva y
    // termina el proceso actual (`std::process::exit`) dentro de su propia inicialización — por
    // lo tanto ese `.setup(...)` nunca llega a ejecutarse para la segunda instancia: el vault
    // activo de la primera instancia nunca se reinicializa, nunca se vuelve a correr el barrido
    // de arranque, y nunca se abre una segunda conexión a `vault.db`. Cuaderno Clínico pasa así a
    // ser explícitamente single-instance; el callback aquí solo enfoca la ventana ya existente.
    .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
      if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
      }
    }))
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

      // Fase 18 (validación pre-RC): `run_startup_recovery` decide qué hacer con un
      // `vault-rescue` huérfano mirando si `vault_dir` existe en disco — si existe, asume que el
      // `rescue` es basura segura de borrar (el `restore` ya promovió el staging); si NO existe,
      // asume un crash a mitad de camino y restaura el `rescue`. Por eso este `create_dir_all`
      // debe ejecutarse DESPUÉS de `run_startup_recovery`, nunca antes: crear `vault_dir` primero
      // lo haría "existir" artificialmente en el único caso real en que legítimamente no debería
      // existir todavía (crash exactamente entre mover el vault anterior a `rescue` y promover el
      // staging) — la recuperación tomaría la rama equivocada y borraría el `rescue`, perdiendo el
      // vault real en vez de restaurarlo. Descubierto en Fase 18 mediante una validación real de
      // extremo a extremo (el test unitario de `run_startup_recovery` en aislamiento no lo
      // detectaba, porque nunca ejercitaba el orden real de llamadas de `lib.rs`) — ver
      // `restore_startup_recovery_still_restores_the_rescued_vault_through_the_real_app_startup_sequence`.
      backup::service::run_startup_recovery(&vault_dir);

      std::fs::create_dir_all(&vault_dir)?;

      // Fase 16 (Documentos cifrados): barrido de temporales descifrados que
      // un crash de una sesión anterior pudiera haber dejado sin limpiar —
      // ver `services::document_temp::sweep_stale_temp_files`. No hace nada
      // en el caso normal.
      services::document_temp::sweep_stale_temp_files();

      let vault_session: Arc<VaultSession> = Arc::new(VaultSession::new(&vault_dir));
      app.manage(vault_session.clone());

      let document_temp_registry: Arc<services::document_temp::DocumentTempRegistry> = Arc::new(services::document_temp::DocumentTempRegistry::new());
      app.manage(document_temp_registry.clone());

      // Bloqueo automático por inactividad (Fase 1.4). Deliberadamente NO
      // reacciona a que el sistema operativo se suspenda o bloquee la
      // pantalla — eso queda fuera de alcance de esta fase, ver
      // `security::session::VaultSession::tick_auto_lock`.
      //
      // Fase 16: al bloquear por inactividad, también se limpian los
      // temporales descifrados de Documentos que pudieran existir (Bloque 21
      // de la aprobación) — el bloqueo manual hace lo mismo, ver
      // `commands::vault::lock_vault`.
      tauri::async_runtime::spawn(async move {
        loop {
          tokio::time::sleep(AUTO_LOCK_TICK_INTERVAL).await;
          if vault_session.tick_auto_lock() {
            document_temp_registry.cleanup_all();
          }
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
      commands::get_current_safety_plan,
      commands::get_safety_plan_draft,
      commands::list_safety_plan_history,
      commands::get_safety_plan_by_id,
      commands::create_safety_plan_draft,
      commands::create_safety_plan_draft_from_current,
      commands::update_safety_plan_draft,
      commands::discard_safety_plan_draft,
      commands::confirm_safety_plan_draft,
      commands::list_safety_plan_contacts,
      commands::add_safety_plan_contact,
      commands::update_safety_plan_contact,
      commands::delete_safety_plan_contact,
      commands::create_assessment_instrument,
      commands::get_assessment_instrument,
      commands::list_assessment_instruments,
      commands::update_assessment_instrument,
      commands::create_assessment_administration,
      commands::get_assessment_administration,
      commands::list_assessment_administrations,
      commands::list_archived_assessment_administrations,
      commands::list_assessment_administrations_for_instrument,
      commands::update_assessment_administration,
      commands::archive_assessment_administration,
      commands::restore_assessment_administration,
      commands::create_formulation,
      commands::get_formulation,
      commands::get_formulation_by_episode,
      commands::list_formulations,
      commands::get_current_formulation_version,
      commands::get_formulation_version,
      commands::list_formulation_versions,
      commands::create_formulation_version,
      commands::create_document,
      commands::get_document,
      commands::list_documents,
      commands::list_archived_documents,
      commands::update_document_metadata,
      commands::archive_document,
      commands::restore_document,
      commands::get_document_data_url,
      commands::open_document_externally,
      commands::export_document,
      commands::check_document_consistency,
    ])
    .build(tauri::generate_context!())
    .expect("error while building tauri application")
    .run(|app_handle, event| {
      // Fase 16: al cerrar la aplicación, limpiar cualquier temporal
      // descifrado de Documentos que pudiera seguir existiendo (Bloque 21 de
      // la aprobación, "cuando sea posible") — best-effort, nunca bloquea el
      // cierre.
      if let tauri::RunEvent::Exit = event {
        if let Some(registry) = app_handle.try_state::<Arc<services::document_temp::DocumentTempRegistry>>() {
          registry.cleanup_all();
        }
      }
    });
}
