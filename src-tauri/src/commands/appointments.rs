//! Comandos Tauri de citas. Cada comando mutador sigue siempre la misma
//! forma: (1) un `with_connection` corto para aplicar la mutación local —que
//! ya persiste y ya es el resultado final desde el punto de vista de los
//! datos clínicos—, (2) la reconciliación con Google (best-effort, nunca
//! puede deshacer el paso 1), y (3) un segundo `with_connection` corto para
//! releer la cita con lo que haya cambiado (típicamente `google_event_id`).
//!
//! Ningún comando de este archivo recibe ni ejecuta SQL arbitrario, y
//! ninguno le da a Google Calendar autoridad sobre el estado local — ver
//! `calendar::sync` para el contrato exacto de cada operación.

use std::sync::Arc;

use tauri::State;

use crate::calendar::sync::{self, SyncOutcome};
use crate::repositories::appointments::Appointment;
use crate::security::VaultSession;
use crate::services::appointments::{self, AppointmentInput, OverlapWarning};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

/// Lo que devuelve cada comando mutador: la cita tal como quedó después de
/// la reconciliación con Google, más el resultado de esa reconciliación —
/// nunca un error duro solo porque Google no respondió, ya que la mutación
/// local siempre se completó antes de intentar sincronizar.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppointmentWithSync {
    #[serde(flatten)]
    pub appointment: Appointment,
    pub sync_outcome: SyncOutcome,
}

/// Los tres pasos de `calendar::sync` (preparar → reconciliar → aplicar),
/// orquestados desde el único lugar que tiene tanto acceso al vault
/// (`VaultSession`) como al runtime async: los comandos. `calendar::sync` en
/// sí mismo no conoce `VaultSession` ni Tauri.
pub(crate) async fn sync_after_mutation(state: &State<'_, SharedVaultSession>, appointment_id: &str) -> SyncOutcome {
    let prepared = match state.with_connection(|conn| sync::prepare_reconcile(conn, appointment_id)) {
        Ok(Ok(input)) => input,
        Ok(Err(outcome)) => return outcome,
        Err(_) => return SyncOutcome::Failed { message: LOCKED_MESSAGE.to_string() },
    };

    let result = sync::reconcile(prepared).await;

    let _ = state.with_connection(|conn| sync::apply_reconcile_effect(conn, appointment_id, result.effect));
    result.outcome
}

/// Sincroniza y vuelve a leer la cita — el paso final compartido por todos
/// los comandos mutadores.
pub(crate) async fn finish(state: &State<'_, SharedVaultSession>, id: &str) -> Result<AppointmentWithSync, String> {
    let sync_outcome = sync_after_mutation(state, id).await;
    let appointment = state
        .with_connection(|conn| appointments::get_appointment(conn, id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(AppointmentWithSync { appointment, sync_outcome })
}

#[tauri::command]
pub async fn create_appointment(
    input: AppointmentInput,
    state: State<'_, SharedVaultSession>,
) -> Result<AppointmentWithSync, String> {
    let created = state
        .with_connection(|conn| appointments::create_appointment(conn, input))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())?;
    finish(&state, &created.id).await
}

#[tauri::command]
pub fn get_appointment(id: String, state: State<'_, SharedVaultSession>) -> Result<Appointment, String> {
    state
        .with_connection(|conn| appointments::get_appointment(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_appointments(
    from: Option<String>,
    to: Option<String>,
    state: State<'_, SharedVaultSession>,
) -> Result<Vec<Appointment>, String> {
    state
        .with_connection(|conn| appointments::list_appointments(conn, from, to))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Papelera: citas con soft delete aplicado. Separado de `list_appointments`
/// a propósito, mismo criterio que en pacientes — nunca se mezclan activas y
/// archivadas en la misma respuesta.
#[tauri::command]
pub fn list_archived_appointments(
    from: Option<String>,
    to: Option<String>,
    state: State<'_, SharedVaultSession>,
) -> Result<Vec<Appointment>, String> {
    state
        .with_connection(|conn| appointments::list_archived_appointments(conn, from, to))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Advertencia de solapamiento — nunca bloquea el guardado, solo informa.
#[tauri::command]
pub fn check_overlap(
    starts_at: String,
    ends_at: String,
    exclude_id: Option<String>,
    state: State<'_, SharedVaultSession>,
) -> Result<Vec<OverlapWarning>, String> {
    state
        .with_connection(|conn| appointments::check_overlap(conn, &starts_at, &ends_at, exclude_id.as_deref()))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_appointment(
    id: String,
    input: AppointmentInput,
    state: State<'_, SharedVaultSession>,
) -> Result<AppointmentWithSync, String> {
    state
        .with_connection(|conn| appointments::update_appointment(conn, &id, input))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())?;
    finish(&state, &id).await
}

/// Marca la cita como cancelada (distinto de archivar) y, si tenía evento
/// espejo en Google, lo elimina — ver `calendar::sync::reconcile`.
#[tauri::command]
pub async fn cancel_appointment(id: String, state: State<'_, SharedVaultSession>) -> Result<AppointmentWithSync, String> {
    state
        .with_connection(|conn| appointments::cancel_appointment(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())?;
    finish(&state, &id).await
}

/// Soft delete. Igual que cancelar, elimina el evento espejo en Google si
/// existía.
#[tauri::command]
pub async fn archive_appointment(id: String, state: State<'_, SharedVaultSession>) -> Result<AppointmentWithSync, String> {
    state
        .with_connection(|conn| appointments::archive_appointment(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())?;
    finish(&state, &id).await
}

#[tauri::command]
pub async fn restore_appointment(id: String, state: State<'_, SharedVaultSession>) -> Result<AppointmentWithSync, String> {
    state
        .with_connection(|conn| appointments::restore_appointment(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())?;
    finish(&state, &id).await
}
