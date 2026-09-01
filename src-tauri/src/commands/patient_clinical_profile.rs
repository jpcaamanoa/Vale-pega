//! Comandos Tauri de antecedentes clínicos. Capa fina: sin SQL, sin reglas
//! de negocio — cada uno obtiene la conexión vía
//! `VaultSession::with_connection` y delega en
//! `services::patient_clinical_profile`.
//!
//! Ningún antecedente clínico se sincroniza jamás con Google Calendar —
//! este archivo no importa nada de `calendar::*`, y así se queda.

use std::sync::Arc;

use tauri::State;

use crate::repositories::patient_clinical_profile::ClinicalProfile;
use crate::security::VaultSession;
use crate::services::patient_clinical_profile::{self, ClinicalProfileInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

/// `None` significa que el paciente existe pero todavía no tiene
/// antecedentes registrados — no es un error.
#[tauri::command]
pub fn get_clinical_profile(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Option<ClinicalProfile>, String> {
    state
        .with_connection(|conn| patient_clinical_profile::get_clinical_profile(conn, &patient_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_clinical_profile(patient_id: String, input: ClinicalProfileInput, state: State<'_, SharedVaultSession>) -> Result<ClinicalProfile, String> {
    state
        .with_connection(|conn| patient_clinical_profile::create_clinical_profile(conn, &patient_id, input))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_clinical_profile(patient_id: String, input: ClinicalProfileInput, state: State<'_, SharedVaultSession>) -> Result<ClinicalProfile, String> {
    state
        .with_connection(|conn| patient_clinical_profile::update_clinical_profile(conn, &patient_id, input))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}
