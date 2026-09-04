//! Comandos Tauri de "preparación para la próxima sesión" (Fase 8). Capa
//! fina: sin SQL, sin reglas de negocio — cada uno obtiene la conexión vía
//! `VaultSession::with_connection` y delega en `services::patient_prep_notes`.

use std::sync::Arc;

use tauri::State;

use crate::repositories::patient_prep_notes::PatientPrepNote;
use crate::security::VaultSession;
use crate::services::patient_prep_notes::{self, PrepNoteInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn create_prep_note(input: PrepNoteInput, state: State<'_, SharedVaultSession>) -> Result<PatientPrepNote, String> {
    state.with_connection(|conn| patient_prep_notes::create_prep_note(conn, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_prep_note(id: String, state: State<'_, SharedVaultSession>) -> Result<PatientPrepNote, String> {
    state.with_connection(|conn| patient_prep_notes::get_prep_note(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_prep_notes(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<PatientPrepNote>, String> {
    state.with_connection(|conn| patient_prep_notes::list_prep_notes(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Lo que se muestra al abrir una sesión nueva y en el panel de continuidad
/// de la ficha del paciente — únicamente las que siguen `pendiente`.
#[tauri::command]
pub fn list_pending_prep_notes(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<PatientPrepNote>, String> {
    state.with_connection(|conn| patient_prep_notes::list_pending_prep_notes(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_prep_note(id: String, content: String, state: State<'_, SharedVaultSession>) -> Result<PatientPrepNote, String> {
    state.with_connection(|conn| patient_prep_notes::update_prep_note(conn, &id, content)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Cambia el estado a `pendiente`/`abordado`/`descartado` — la acción
/// explícita de resolución desde la sesión, o de reabrir si se reconsidera.
#[tauri::command]
pub fn set_prep_note_status(id: String, status: String, state: State<'_, SharedVaultSession>) -> Result<PatientPrepNote, String> {
    state.with_connection(|conn| patient_prep_notes::set_prep_note_status(conn, &id, status)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
