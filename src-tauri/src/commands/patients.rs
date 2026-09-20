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

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::security::VaultSession;
use crate::services::patients::{self, GeographicStatistics, PatientInput, PatientListItem};
use crate::repositories::patients::{Patient, PatientHardDeleteScope};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

fn files_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("vault").join("files"))
        .map_err(|e| format!("no se pudo determinar el directorio de datos de la aplicación: {e}"))
}

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

/// Papelera: pacientes con soft delete aplicado. Separado de `list_patients`
/// a propósito — nunca se mezclan activos y archivados en la misma
/// respuesta.
#[tauri::command]
pub fn list_archived_patients(
    search: Option<String>,
    state: State<'_, SharedVaultSession>,
) -> Result<Vec<PatientListItem>, String> {
    state
        .with_connection(|conn| patients::list_archived_patients(conn, search))
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

/// Resumen de solo lectura para el modal de confirmación de "Eliminar permanentemente" — nunca
/// borra nada.
#[tauri::command]
pub fn get_patient_hard_delete_scope(id: String, state: State<'_, SharedVaultSession>) -> Result<PatientHardDeleteScope, String> {
    state
        .with_connection(|conn| patients::hard_delete_scope(conn, &id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Borrado físico e irreversible — solo alcanzable desde "Archivados" en el frontend, tras la
/// confirmación escrita "ELIMINAR". Ver `services::patients::hard_delete_patient` para el modelo
/// completo de validaciones/atomicidad.
#[tauri::command]
pub fn hard_delete_patient(app: AppHandle, id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    let root = files_root(&app)?;
    patients::hard_delete_patient(&state, &root, &id).map_err(|e| e.to_string())
}

/// Estadísticas geográficas agregadas (Fase 6.1) para la pantalla
/// "Estadísticas". `include_archived = false` (por defecto en el frontend)
/// muestra solo pacientes activos; `true` incluye también los archivados.
/// `suppress_small_categories = false` (lo que usa hoy la pantalla privada
/// "Estadísticas") muestra cada región/comuna tal cual, sin agrupar ninguna
/// en "Otras" — ver `services::patients::geographic_statistics`. La
/// respuesta nunca contiene datos de un paciente individual — ver
/// `services::patients::GeographicStatistics`.
#[tauri::command]
pub fn get_geographic_statistics(
    include_archived: bool,
    suppress_small_categories: bool,
    state: State<'_, SharedVaultSession>,
) -> Result<GeographicStatistics, String> {
    state
        .with_connection(|conn| patients::geographic_statistics(conn, include_archived, suppress_small_categories))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}
