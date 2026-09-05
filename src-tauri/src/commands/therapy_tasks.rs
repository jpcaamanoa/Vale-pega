//! Comandos Tauri de tareas terapéuticas entre sesiones (Fase 8). Capa
//! fina: sin SQL, sin reglas de negocio — cada uno obtiene la conexión vía
//! `VaultSession::with_connection` y delega en `services::therapy_tasks`.

use std::sync::Arc;

use tauri::State;

use crate::repositories::therapy_tasks::{TherapyTask, TherapyTaskListItem};
use crate::security::VaultSession;
use crate::services::therapy_tasks::{self, TherapyTaskInput, TherapyTaskReviewInput, TherapyTaskUpdateInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn create_therapy_task(input: TherapyTaskInput, state: State<'_, SharedVaultSession>) -> Result<TherapyTask, String> {
    state.with_connection(|conn| therapy_tasks::create_task(conn, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_therapy_task(id: String, state: State<'_, SharedVaultSession>) -> Result<TherapyTask, String> {
    state.with_connection(|conn| therapy_tasks::get_task(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_therapy_tasks(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<TherapyTaskListItem>, String> {
    state.with_connection(|conn| therapy_tasks::list_tasks(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Papelera: tareas con soft delete aplicado. Separado de
/// `list_therapy_tasks` a propósito, mismo criterio que en Pacientes,
/// Agenda, Sesiones, Objetivos y Pagos.
#[tauri::command]
pub fn list_archived_therapy_tasks(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<TherapyTaskListItem>, String> {
    state.with_connection(|conn| therapy_tasks::list_archived_tasks(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Lo que se muestra al abrir una sesión nueva y en el panel de continuidad
/// de la ficha del paciente — únicamente las que siguen `pendiente`.
#[tauri::command]
pub fn list_pending_therapy_tasks(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<TherapyTaskListItem>, String> {
    state.with_connection(|conn| therapy_tasks::list_pending_tasks(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// `'pendiente'` + `'parcial'` del paciente — usada exclusivamente por la
/// advertencia del flujo de cierre de un proceso (Fase 11).
#[tauri::command]
pub fn list_pending_or_partial_therapy_tasks(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<TherapyTaskListItem>, String> {
    state.with_connection(|conn| therapy_tasks::list_pending_or_partial_tasks(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_therapy_task(id: String, input: TherapyTaskUpdateInput, state: State<'_, SharedVaultSession>) -> Result<TherapyTask, String> {
    state.with_connection(|conn| therapy_tasks::update_task(conn, &id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// La acción de resolución: cambia el estado y, opcionalmente, registra en
/// qué sesión se revisó.
#[tauri::command]
pub fn review_therapy_task(id: String, input: TherapyTaskReviewInput, state: State<'_, SharedVaultSession>) -> Result<TherapyTask, String> {
    state.with_connection(|conn| therapy_tasks::review_task(conn, &id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Soft delete ("archivar"). No existe un comando de borrado físico.
#[tauri::command]
pub fn archive_therapy_task(id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| therapy_tasks::archive_task(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_therapy_task(id: String, state: State<'_, SharedVaultSession>) -> Result<TherapyTask, String> {
    state.with_connection(|conn| therapy_tasks::restore_task(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Conteo global de tareas pendientes — para el bloque "Pendientes" del
/// Dashboard. Nunca una lista de tareas individuales.
#[tauri::command]
pub fn get_pending_therapy_task_count(state: State<'_, SharedVaultSession>) -> Result<i64, String> {
    state.with_connection(therapy_tasks::pending_task_count).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
