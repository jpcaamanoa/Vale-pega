//! Comandos Tauri de objetivos terapéuticos, sus indicadores, y su vínculo
//! con sesiones. Capa fina: sin SQL, sin reglas de negocio — cada uno
//! obtiene la conexión vía `VaultSession::with_connection` y delega en
//! `services::goals`.
//!
//! Ningún objetivo terapéutico se sincroniza jamás con Google Calendar —
//! este archivo no importa nada de `calendar::*`, y así se queda.

use std::sync::Arc;

use tauri::State;

use crate::repositories::goal_indicators::GoalIndicator;
use crate::repositories::goals::{Goal, GoalListItem};
use crate::repositories::session_goals::{GoalSessionRow, SessionGoalRow};
use crate::security::VaultSession;
use crate::services::goals::{self, GoalIndicatorInput, GoalInput, GoalUpdateInput, SessionGoalLinkInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn create_goal(input: GoalInput, state: State<'_, SharedVaultSession>) -> Result<Goal, String> {
    state.with_connection(|conn| goals::create_goal(conn, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_goal(id: String, state: State<'_, SharedVaultSession>) -> Result<Goal, String> {
    state.with_connection(|conn| goals::get_goal(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_goals(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<GoalListItem>, String> {
    state.with_connection(|conn| goals::list_goals(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Papelera: objetivos con soft delete aplicado. Separado de `list_goals`
/// a propósito, mismo criterio que en Pacientes, Agenda y Sesiones.
#[tauri::command]
pub fn list_archived_goals(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<GoalListItem>, String> {
    state.with_connection(|conn| goals::list_archived_goals(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_goal(id: String, input: GoalUpdateInput, state: State<'_, SharedVaultSession>) -> Result<Goal, String> {
    state.with_connection(|conn| goals::update_goal(conn, &id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Soft delete ("archivar"). No existe un comando de borrado físico. Nunca
/// toca indicadores ni vínculos con sesiones.
#[tauri::command]
pub fn archive_goal(id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| goals::archive_goal(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_goal(id: String, state: State<'_, SharedVaultSession>) -> Result<Goal, String> {
    state.with_connection(|conn| goals::restore_goal(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_goal_indicators(goal_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<GoalIndicator>, String> {
    state.with_connection(|conn| goals::list_indicators(conn, &goal_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_goal_indicator(goal_id: String, input: GoalIndicatorInput, state: State<'_, SharedVaultSession>) -> Result<GoalIndicator, String> {
    state.with_connection(|conn| goals::create_indicator(conn, &goal_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_goal_indicator(id: String, input: GoalIndicatorInput, state: State<'_, SharedVaultSession>) -> Result<GoalIndicator, String> {
    state.with_connection(|conn| goals::update_indicator(conn, &id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_goal_indicator(id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| goals::delete_indicator(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn link_session_goal(input: SessionGoalLinkInput, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| goals::link_session_goal(conn, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn unlink_session_goal(session_id: String, goal_id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state
        .with_connection(|conn| goals::unlink_session_goal(conn, &session_id, &goal_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_session_goal_progress(
    session_id: String,
    goal_id: String,
    progress_note: Option<String>,
    state: State<'_, SharedVaultSession>,
) -> Result<(), String> {
    state
        .with_connection(|conn| goals::update_link_progress_note(conn, &session_id, &goal_id, progress_note))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Objetivos trabajados en una sesión — usado por la sección "Objetivos
/// trabajados en esta sesión" de `SessionDetailScreen`.
#[tauri::command]
pub fn list_goals_for_session(session_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<SessionGoalRow>, String> {
    state.with_connection(|conn| goals::list_goals_for_session(conn, &session_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Sesiones donde se trabajó un objetivo — usado por la sección "Sesiones
/// relacionadas" de `GoalDetailScreen`.
#[tauri::command]
pub fn list_sessions_for_goal(goal_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<GoalSessionRow>, String> {
    state.with_connection(|conn| goals::list_sessions_for_goal(conn, &goal_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Objetivos activos del paciente de esa sesión que todavía no están
/// vinculados a ella — lo que ofrece el selector "Agregar objetivo".
/// Calculado enteramente en el backend para que el frontend nunca decida
/// por su cuenta qué objetivos son elegibles.
#[tauri::command]
pub fn list_available_goals_for_session(session_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<GoalListItem>, String> {
    state
        .with_connection(|conn| goals::list_available_goals_for_session(conn, &session_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}
