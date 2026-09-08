//! Comandos Tauri de evaluaciones clínicas/psicométricas (Fase 13). Capa
//! fina: sin SQL, sin reglas de negocio — cada uno obtiene la conexión vía
//! `VaultSession::with_connection` y delega en `services::assessments`.
//!
//! Ningún comando de este archivo importa nada de `calendar::*` — las
//! evaluaciones nunca se sincronizan con Google Calendar.

use std::sync::Arc;

use tauri::State;

use crate::repositories::assessments::{AssessmentAdministration, AssessmentAdministrationSummary, AssessmentInstrument};
use crate::security::VaultSession;
use crate::services::assessments::{self, AdministrationInput, AdministrationUpdateInput, InstrumentInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn create_assessment_instrument(input: InstrumentInput, state: State<'_, SharedVaultSession>) -> Result<AssessmentInstrument, String> {
    state.with_connection(|conn| assessments::create_instrument(conn, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_assessment_instrument(id: String, state: State<'_, SharedVaultSession>) -> Result<AssessmentInstrument, String> {
    state.with_connection(|conn| assessments::get_instrument(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// El catálogo completo — nunca filtrado por paciente, es compartido entre
/// todos.
#[tauri::command]
pub fn list_assessment_instruments(state: State<'_, SharedVaultSession>) -> Result<Vec<AssessmentInstrument>, String> {
    state.with_connection(assessments::list_instruments).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_assessment_instrument(id: String, input: InstrumentInput, state: State<'_, SharedVaultSession>) -> Result<AssessmentInstrument, String> {
    state.with_connection(|conn| assessments::update_instrument(conn, &id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_assessment_administration(input: AdministrationInput, state: State<'_, SharedVaultSession>) -> Result<AssessmentAdministration, String> {
    state.with_connection(|conn| assessments::create_administration(conn, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Contenido completo de una administración concreta — solo se invoca al
/// abrirla explícitamente (mismo criterio de minimización de IPC que
/// `get_safety_plan_by_id`).
#[tauri::command]
pub fn get_assessment_administration(id: String, state: State<'_, SharedVaultSession>) -> Result<AssessmentAdministration, String> {
    state.with_connection(|conn| assessments::get_administration(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_assessment_administrations(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<AssessmentAdministrationSummary>, String> {
    state.with_connection(|conn| assessments::list_administrations(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Papelera: administraciones con soft delete aplicado. Mismo criterio que
/// el resto del dominio (Pacientes, Agenda, Sesiones, Objetivos, Pagos,
/// Tareas).
#[tauri::command]
pub fn list_archived_assessment_administrations(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<AssessmentAdministrationSummary>, String> {
    state.with_connection(|conn| assessments::list_archived_administrations(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Evolución longitudinal de un mismo instrumento para un paciente — nunca
/// mezcla instrumentos distintos.
#[tauri::command]
pub fn list_assessment_administrations_for_instrument(patient_id: String, instrument_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<AssessmentAdministrationSummary>, String> {
    state.with_connection(|conn| assessments::list_administrations_for_instrument(conn, &patient_id, &instrument_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_assessment_administration(id: String, input: AdministrationUpdateInput, state: State<'_, SharedVaultSession>) -> Result<AssessmentAdministration, String> {
    state.with_connection(|conn| assessments::update_administration(conn, &id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Soft delete ("archivar"). No existe un comando de borrado físico.
#[tauri::command]
pub fn archive_assessment_administration(id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| assessments::archive_administration(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_assessment_administration(id: String, state: State<'_, SharedVaultSession>) -> Result<AssessmentAdministration, String> {
    state.with_connection(|conn| assessments::restore_administration(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
