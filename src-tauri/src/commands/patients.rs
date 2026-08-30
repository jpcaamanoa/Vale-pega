//! Comandos Tauri de pacientes. Cada uno es una capa fina: no contiene SQL
//! ni reglas de negocio, solo obtiene la conexión a través de
//! `VaultSession::with_connection` (que falla con un error genérico si el
//! vault está bloqueado — nunca hay otra forma de llegar a los datos) y
//! delega en `services::patients`.
//!
//! Ningún comando de este archivo recibe ni ejecuta SQL arbitrario: cada
//! uno es una operación de negocio específica y con nombre propio
//! (`create_patient`, `list_patients`, ...), nunca un `run_sql(query)`
//! genérico.

use std::sync::Arc;

use tauri::State;

use crate::security::VaultSession;
use crate::services::patients::{self, PatientInput, PatientListItem};
use crate::repositories::patients::Patient;

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn create_patient(input: PatientInput, state: State<'_, SharedVaultSession>) -> Result<Patient, String> {
    state
        .with_connection(|conn| patients::create_patient(conn, input))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_patient(id: String, state: State<'_, SharedVaultSession>) -> Result<Patient, String> {
    state
        .with_connection(|conn| patients::get_patient(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_patients(
    search: Option<String>,
    state: State<'_, SharedVaultSession>,
) -> Result<Vec<PatientListItem>, String> {
    state
        .with_connection(|conn| patients::list_patients(conn, search))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_patient(
    id: String,
    input: PatientInput,
    state: State<'_, SharedVaultSession>,
) -> Result<Patient, String> {
    state
        .with_connection(|conn| patients::update_patient(conn, &id, input))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Soft delete ("archivar"). No existe un comando de borrado físico.
#[tauri::command]
pub fn archive_patient(id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state
        .with_connection(|conn| patients::archive_patient(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_patient(id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state
        .with_connection(|conn| patients::restore_patient(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}
