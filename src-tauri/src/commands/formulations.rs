//! Comandos Tauri de Formulación Clínica Textual Versionada (Fase 15).
//! Capa fina: sin SQL, sin reglas de negocio — cada uno obtiene la
//! conexión vía `VaultSession::with_connection` y delega en
//! `services::formulations`.
//!
//! Ningún comando de este archivo importa nada de `calendar::*` — la
//! formulación nunca se sincroniza con Google Calendar.

use std::sync::Arc;

use tauri::State;

use crate::repositories::formulations::{CaseFormulation, FormulationSummary, FormulationVersion};
use crate::security::VaultSession;
use crate::services::formulations::{self, FormulationInput, NewVersionInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn create_formulation(input: FormulationInput, state: State<'_, SharedVaultSession>) -> Result<(CaseFormulation, FormulationVersion), String> {
    state.with_connection(|conn| formulations::create_formulation(conn, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_formulation(id: String, state: State<'_, SharedVaultSession>) -> Result<CaseFormulation, String> {
    state.with_connection(|conn| formulations::get_formulation(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// `None` si el proceso todavía no tiene una formulación principal — no es
/// un error.
#[tauri::command]
pub fn get_formulation_by_episode(episode_id: String, state: State<'_, SharedVaultSession>) -> Result<Option<CaseFormulation>, String> {
    state.with_connection(|conn| formulations::get_formulation_by_episode(conn, &episode_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Todas las formulaciones del paciente, de cualquiera de sus procesos —
/// minimizado por IPC, nunca lleva el contenido completo de una versión.
#[tauri::command]
pub fn list_formulations(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<FormulationSummary>, String> {
    state.with_connection(|conn| formulations::list_formulations(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_current_formulation_version(formulation_id: String, state: State<'_, SharedVaultSession>) -> Result<FormulationVersion, String> {
    state.with_connection(|conn| formulations::get_current_version(conn, &formulation_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// El contenido completo de una versión concreta — solo se invoca al
/// abrirla explícitamente desde el historial (misma minimización de IPC
/// que `get_safety_plan_by_id`/`get_assessment_administration`).
#[tauri::command]
pub fn get_formulation_version(version_id: String, state: State<'_, SharedVaultSession>) -> Result<FormulationVersion, String> {
    state.with_connection(|conn| formulations::get_version(conn, &version_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_formulation_versions(formulation_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<FormulationVersion>, String> {
    state.with_connection(|conn| formulations::list_versions(conn, &formulation_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// "Actualizar formulación": crea una versión nueva, nunca sobrescribe la
/// anterior.
#[tauri::command]
pub fn create_formulation_version(formulation_id: String, input: NewVersionInput, state: State<'_, SharedVaultSession>) -> Result<(CaseFormulation, FormulationVersion), String> {
    state.with_connection(|conn| formulations::create_new_version(conn, &formulation_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
