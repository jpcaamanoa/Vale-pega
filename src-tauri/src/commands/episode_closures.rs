//! Comandos Tauri del cierre estructurado de un proceso terapéutico (Fase
//! 11). Capa fina: sin SQL, sin reglas de negocio — cada uno obtiene la
//! conexión vía `VaultSession::with_connection` y delega en
//! `services::episode_closures` (o en `services::sessions`/`services::goals`
//! para las consultas de solo lectura asociadas a la vista de un proceso).
//!
//! Ningún cierre se sincroniza jamás con Google Calendar — este archivo no
//! importa nada de `calendar::*`.

use std::sync::Arc;

use tauri::State;

use crate::repositories::episode_closures::EpisodeClosure;
use crate::repositories::goals::GoalListItem;
use crate::repositories::sessions::SessionListItem;
use crate::repositories::treatment_episodes::TreatmentEpisode;
use crate::security::VaultSession;
use crate::services::episode_closures::{self, CloseEpisodeInput, RevertClosureInput};
use crate::services::goals;
use crate::services::sessions;

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn close_treatment_episode(episode_id: String, input: CloseEpisodeInput, state: State<'_, SharedVaultSession>) -> Result<(EpisodeClosure, TreatmentEpisode), String> {
    state.with_connection(|conn| episode_closures::close_episode(conn, &episode_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn revert_episode_closure(closure_id: String, input: RevertClosureInput, state: State<'_, SharedVaultSession>) -> Result<(EpisodeClosure, TreatmentEpisode), String> {
    state.with_connection(|conn| episode_closures::revert_closure(conn, &closure_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// `None` si el proceso no tiene un cierre vigente — no es un error.
#[tauri::command]
pub fn get_active_episode_closure(episode_id: String, state: State<'_, SharedVaultSession>) -> Result<Option<EpisodeClosure>, String> {
    state.with_connection(|conn| episode_closures::get_active_closure(conn, &episode_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_episode_closure_history(episode_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<EpisodeClosure>, String> {
    state.with_connection(|conn| episode_closures::list_closure_history(conn, &episode_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Sesiones futuras agendadas de un proceso, sin resolver todavía — la UI
/// las usa para construir el formulario de resolución obligatoria antes de
/// poder cerrar.
#[tauri::command]
pub fn list_upcoming_episode_sessions(episode_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<SessionListItem>, String> {
    state.with_connection(|conn| sessions::list_upcoming_sessions_by_episode(conn, &episode_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Sesiones históricas de un proceso — para la vista de un proceso cerrado
/// (o activo/pausado, sin distinción).
#[tauri::command]
pub fn list_episode_sessions(episode_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<SessionListItem>, String> {
    state.with_connection(|conn| sessions::list_sessions_by_episode(conn, &episode_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Objetivos relacionados con un proceso — con su estado actual en vivo.
#[tauri::command]
pub fn list_episode_goals(episode_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<GoalListItem>, String> {
    state.with_connection(|conn| goals::list_goals_by_episode(conn, &episode_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
