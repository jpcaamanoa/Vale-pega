//! Comandos Tauri de antecedentes clínicos específicos de un proceso
//! terapéutico (Fase 9). Capa fina — mismo patrón que
//! `commands::patient_clinical_profile`.
//!
//! Nunca se sincroniza con Google Calendar — este archivo no importa nada
//! de `calendar::*`.

use std::sync::Arc;

use tauri::State;

use crate::repositories::episode_clinical_profile::EpisodeClinicalProfile;
use crate::security::VaultSession;
use crate::services::episode_clinical_profile::{self, EpisodeClinicalProfileInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

/// `None` significa que el proceso existe pero todavía no tiene
/// antecedentes específicos registrados — no es un error.
#[tauri::command]
pub fn get_episode_clinical_profile(episode_id: String, state: State<'_, SharedVaultSession>) -> Result<Option<EpisodeClinicalProfile>, String> {
    state.with_connection(|conn| episode_clinical_profile::get_episode_clinical_profile(conn, &episode_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_episode_clinical_profile(episode_id: String, input: EpisodeClinicalProfileInput, state: State<'_, SharedVaultSession>) -> Result<EpisodeClinicalProfile, String> {
    state.with_connection(|conn| episode_clinical_profile::create_episode_clinical_profile(conn, &episode_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_episode_clinical_profile(episode_id: String, input: EpisodeClinicalProfileInput, state: State<'_, SharedVaultSession>) -> Result<EpisodeClinicalProfile, String> {
    state.with_connection(|conn| episode_clinical_profile::update_episode_clinical_profile(conn, &episode_id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
