//! Comandos Tauri de sesiones clínicas y notas. Capa fina: sin SQL, sin
//! reglas de negocio — cada uno obtiene la conexión vía
//! `VaultSession::with_connection` y delega en `services::sessions`.
//!
//! Ninguna sesión clínica se sincroniza jamás con Google Calendar — este
//! archivo no importa nada de `calendar::*`, y así se queda.

use std::sync::Arc;

use tauri::State;

use crate::repositories::session_notes::SessionNote;
use crate::repositories::sessions::{Session, SessionListItem};
use crate::security::VaultSession;
use crate::services::sessions::{self, SessionInput, SessionMetadataInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn create_session(input: SessionInput, state: State<'_, SharedVaultSession>) -> Result<Session, String> {
    state
        .with_connection(|conn| sessions::create_session(conn, input))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map(|result| result.session)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_session(id: String, state: State<'_, SharedVaultSession>) -> Result<Session, String> {
    state
        .with_connection(|conn| sessions::get_session(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// La sesión ya asociada a esa cita, si existe — usado por `AppointmentDetailScreen`
/// para decidir entre "Iniciar sesión" y "Ver sesión".
#[tauri::command]
pub fn get_session_for_appointment(
    appointment_id: String,
    state: State<'_, SharedVaultSession>,
) -> Result<Option<Session>, String> {
    state
        .with_connection(|conn| sessions::get_session_for_appointment(conn, &appointment_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_sessions(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<SessionListItem>, String> {
    state
        .with_connection(|conn| sessions::list_sessions(conn, &patient_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Papelera: sesiones con soft delete aplicado. Separado de `list_sessions`
/// a propósito, mismo criterio que en Pacientes y Agenda.
#[tauri::command]
pub fn list_archived_sessions(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<SessionListItem>, String> {
    state
        .with_connection(|conn| sessions::list_archived_sessions(conn, &patient_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_session_metadata(
    id: String,
    input: SessionMetadataInput,
    state: State<'_, SharedVaultSession>,
) -> Result<Session, String> {
    state
        .with_connection(|conn| sessions::update_session_metadata(conn, &id, input))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Soft delete ("archivar"). No existe un comando de borrado físico. Nunca
/// toca las notas de la sesión.
#[tauri::command]
pub fn archive_session(id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state
        .with_connection(|conn| sessions::archive_session(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_session(id: String, state: State<'_, SharedVaultSession>) -> Result<Session, String> {
    state
        .with_connection(|conn| sessions::restore_session(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_current_note(session_id: String, state: State<'_, SharedVaultSession>) -> Result<SessionNote, String> {
    state
        .with_connection(|conn| sessions::get_current_note(conn, &session_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_note_history(session_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<SessionNote>, String> {
    state
        .with_connection(|conn| sessions::list_note_history(conn, &session_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Autoguardado del borrador vigente. Nunca puede tocar una nota cerrada
/// (ver `services::sessions::autosave_note_draft`).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn autosave_note_draft(
    session_id: String,
    content: Option<String>,
    interventions: Option<String>,
    homework_tasks: Option<String>,
    next_focus: Option<String>,
    state: State<'_, SharedVaultSession>,
) -> Result<(), String> {
    state
        .with_connection(|conn| sessions::autosave_note_draft(conn, &session_id, content, interventions, homework_tasks, next_focus))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn close_current_note(session_id: String, state: State<'_, SharedVaultSession>) -> Result<SessionNote, String> {
    state
        .with_connection(|conn| sessions::close_current_note(conn, &session_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// "Editar" una nota cerrada: crea la versión siguiente, nunca sobrescribe.
#[tauri::command]
pub fn create_new_note_version(session_id: String, state: State<'_, SharedVaultSession>) -> Result<SessionNote, String> {
    state
        .with_connection(|conn| sessions::create_new_note_version(conn, &session_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Conteo global de sesiones del mes actual — para el bloque "Resumen" del
/// Dashboard (Fase 8). Nunca una lista de sesiones individuales.
#[tauri::command]
pub fn get_sessions_this_month_count(state: State<'_, SharedVaultSession>) -> Result<i64, String> {
    state.with_connection(sessions::sessions_this_month_count).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
