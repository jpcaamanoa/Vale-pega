//! Comandos Tauri del Plan de Seguridad Clínico Versionado (Fase 12). Capa
//! fina: sin SQL, sin reglas de negocio — cada uno obtiene la conexión vía
//! `VaultSession::with_connection` y delega en `services::safety_plans`.
//!
//! Ningún plan de seguridad se sincroniza jamás con Google Calendar — este
//! archivo no importa nada de `calendar::*`.

use std::sync::Arc;

use tauri::State;

use crate::repositories::safety_plans::{SafetyPlan, SafetyPlanContact, SafetyPlanSummary};
use crate::security::VaultSession;
use crate::services::safety_plans::{self, SafetyPlanContactInput, SafetyPlanInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

/// `None` si el paciente todavía no tiene un plan de seguridad vigente —
/// nunca es un error ni bloquea ningún otro flujo.
#[tauri::command]
pub fn get_current_safety_plan(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Option<SafetyPlan>, String> {
    state.with_connection(|conn| safety_plans::get_current_plan(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_safety_plan_draft(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Option<SafetyPlan>, String> {
    state.with_connection(|conn| safety_plans::get_draft(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Historial minimizado (§33/§49): nunca lleva contenido narrativo ni
/// contactos. Abrir una versión concreta usa `get_safety_plan_by_id`.
#[tauri::command]
pub fn list_safety_plan_history(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<SafetyPlanSummary>, String> {
    state.with_connection(|conn| safety_plans::list_history_summaries(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// El contenido completo de una versión concreta del historial — solo se
/// invoca cuando la usuaria la abre explícitamente.
#[tauri::command]
pub fn get_safety_plan_by_id(plan_id: String, state: State<'_, SharedVaultSession>) -> Result<SafetyPlan, String> {
    state.with_connection(|conn| safety_plans::get_plan_by_id(conn, &plan_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_safety_plan_draft(patient_id: String, input: SafetyPlanInput, state: State<'_, SharedVaultSession>) -> Result<SafetyPlan, String> {
    state.with_connection(|conn| safety_plans::create_draft(conn, &patient_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_safety_plan_draft(plan_id: String, input: SafetyPlanInput, state: State<'_, SharedVaultSession>) -> Result<SafetyPlan, String> {
    state.with_connection(|conn| safety_plans::update_draft(conn, &plan_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn discard_safety_plan_draft(plan_id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| safety_plans::discard_draft(conn, &plan_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn confirm_safety_plan_draft(plan_id: String, state: State<'_, SharedVaultSession>) -> Result<SafetyPlan, String> {
    state.with_connection(|conn| safety_plans::confirm_draft(conn, &plan_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_safety_plan_contacts(plan_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<SafetyPlanContact>, String> {
    state.with_connection(|conn| safety_plans::list_contacts(conn, &plan_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_safety_plan_contact(plan_id: String, input: SafetyPlanContactInput, state: State<'_, SharedVaultSession>) -> Result<SafetyPlanContact, String> {
    state.with_connection(|conn| safety_plans::add_contact(conn, &plan_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_safety_plan_contact(contact_id: String, input: SafetyPlanContactInput, state: State<'_, SharedVaultSession>) -> Result<SafetyPlanContact, String> {
    state.with_connection(|conn| safety_plans::update_contact(conn, &contact_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_safety_plan_contact(contact_id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| safety_plans::delete_contact(conn, &contact_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
