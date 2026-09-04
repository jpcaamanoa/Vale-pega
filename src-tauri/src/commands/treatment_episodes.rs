//! Comandos Tauri de procesos terapéuticos (Fase 9). Capa fina: sin SQL,
//! sin reglas de negocio — cada uno obtiene la conexión vía
//! `VaultSession::with_connection` y delega en
//! `services::treatment_episodes`.
//!
//! Ningún proceso terapéutico se sincroniza jamás con Google Calendar —
//! este archivo no importa nada de `calendar::*`, y así se queda.

use std::sync::Arc;

use tauri::State;

use crate::repositories::treatment_episodes::TreatmentEpisode;
use crate::security::VaultSession;
use crate::services::treatment_episodes::{self, TreatmentEpisodeInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn create_treatment_episode(input: TreatmentEpisodeInput, state: State<'_, SharedVaultSession>) -> Result<TreatmentEpisode, String> {
    state.with_connection(|conn| treatment_episodes::create_episode(conn, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_treatment_episode(id: String, state: State<'_, SharedVaultSession>) -> Result<TreatmentEpisode, String> {
    state.with_connection(|conn| treatment_episodes::get_episode(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_treatment_episodes(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<TreatmentEpisode>, String> {
    state.with_connection(|conn| treatment_episodes::list_episodes(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_archived_treatment_episodes(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<TreatmentEpisode>, String> {
    state.with_connection(|conn| treatment_episodes::list_archived_episodes(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_treatment_episode_status(id: String, status: String, state: State<'_, SharedVaultSession>) -> Result<TreatmentEpisode, String> {
    state.with_connection(|conn| treatment_episodes::set_episode_status(conn, &id, &status)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_treatment_episode(id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| treatment_episodes::archive_episode(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_treatment_episode(id: String, state: State<'_, SharedVaultSession>) -> Result<TreatmentEpisode, String> {
    state.with_connection(|conn| treatment_episodes::restore_episode(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
