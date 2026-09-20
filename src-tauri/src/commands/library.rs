//! Comandos Tauri de la Biblioteca global de recursos (fase de continuación post-Fase 19). Capa
//! fina: cada uno obtiene `files_root` exactamente como `commands::documents`, y delega toda la
//! lógica real a `services::library`. Nunca decide nada de criptografía por sí sola.
//!
//! Ningún comando de este archivo importa nada de `calendar::*` — un recurso de Biblioteca nunca
//! se sincroniza con Google Calendar.

use std::path::PathBuf;
use std::sync::Arc;

use base64ct::{Base64, Encoding};
use tauri::{AppHandle, Manager, State};
use zeroize::Zeroize;

use crate::repositories::library::LibraryResourceSummary;
use crate::repositories::patients::PatientSummary;
use crate::security::VaultSession;
use crate::services::document_temp::DocumentTempRegistry;
use crate::services::library::{self, LibraryResourceMetadataInput, NewLibraryResourceInput};

type SharedVaultSession = Arc<VaultSession>;
type SharedTempRegistry = Arc<DocumentTempRegistry>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

fn files_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("vault").join("files"))
        .map_err(|e| format!("no se pudo determinar el directorio de datos de la aplicación: {e}"))
}

#[tauri::command]
pub fn create_library_resource(app: AppHandle, input: NewLibraryResourceInput, state: State<'_, SharedVaultSession>) -> Result<LibraryResourceSummary, String> {
    let root = files_root(&app)?;
    library::create_resource(&state, &root, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_library_resource(id: String, state: State<'_, SharedVaultSession>) -> Result<LibraryResourceSummary, String> {
    state.with_connection(|conn| library::get_resource(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_library_resources(state: State<'_, SharedVaultSession>) -> Result<Vec<LibraryResourceSummary>, String> {
    state.with_connection(library::list_active_resources).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_archived_library_resources(state: State<'_, SharedVaultSession>) -> Result<Vec<LibraryResourceSummary>, String> {
    state.with_connection(library::list_archived_resources).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Solo metadata bibliográfica — nunca el archivo adjunto.
#[tauri::command]
pub fn update_library_resource_metadata(id: String, input: LibraryResourceMetadataInput, state: State<'_, SharedVaultSession>) -> Result<LibraryResourceSummary, String> {
    state.with_connection(|conn| library::update_resource_metadata(conn, &id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// "Eliminar" un recurso: archivado reversible. Si está asociado a pacientes y `force` es
/// `false`, devuelve un error que el frontend interpreta como advertencia ("este recurso está
/// asociado a N paciente(s)") y vuelve a llamar con `force: true` solo si la usuaria confirma.
#[tauri::command]
pub fn archive_library_resource(id: String, force: bool, state: State<'_, SharedVaultSession>) -> Result<LibraryResourceSummary, String> {
    state.with_connection(|conn| library::archive_resource(conn, &id, force)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_library_resource(id: String, state: State<'_, SharedVaultSession>) -> Result<LibraryResourceSummary, String> {
    state.with_connection(|conn| library::restore_resource(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Estrategia híbrida por MIME (mismo criterio que `commands::documents::get_document_data_url`)
/// — cubre imágenes: descifra en memoria y entrega un `data:` URL, sin escribir ningún archivo
/// temporal en disco.
#[tauri::command]
pub fn get_library_resource_data_url(app: AppHandle, id: String, state: State<'_, SharedVaultSession>) -> Result<String, String> {
    let root = files_root(&app)?;
    let (mut content, summary) = library::get_resource_content(&state, &root, &id).map_err(|e| e.to_string())?;
    let mime_type = summary.mime_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let encoded = Base64::encode_string(&content);
    content.zeroize();
    Ok(format!("data:{mime_type};base64,{encoded}"))
}

/// Cubre PDF/DOCX/otros formatos — descifra a un temporal de nombre opaco y lo abre con la
/// aplicación por defecto del sistema operativo, igual que `commands::documents::open_document_externally`.
#[tauri::command]
pub fn open_library_resource_externally(app: AppHandle, id: String, state: State<'_, SharedVaultSession>, temp_registry: State<'_, SharedTempRegistry>) -> Result<(), String> {
    let root = files_root(&app)?;
    let (mut content, summary) = library::get_resource_content(&state, &root, &id).map_err(|e| e.to_string())?;
    let extension = summary.original_filename.as_deref().and_then(|f| f.rsplit_once('.')).map(|(_, ext)| ext).filter(|e| e.len() <= 10);
    let temp_path = temp_registry.allocate(extension);
    let result = std::fs::write(&temp_path, &content);
    content.zeroize();
    result.map_err(|e| format!("no se pudo escribir el archivo temporal: {e}"))?;
    open::that(&temp_path).map_err(|e| format!("no se pudo abrir el recurso: {e}"))
}

/// "Exportar copia" — mismo criterio que `commands::documents::export_document`: escribe el
/// contenido descifrado directamente en el destino elegido por la usuaria, fuera del vault.
#[tauri::command]
pub fn export_library_resource(app: AppHandle, id: String, destination_path: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    let root = files_root(&app)?;
    let (mut content, _summary) = library::get_resource_content(&state, &root, &id).map_err(|e| e.to_string())?;
    let result = std::fs::write(&destination_path, &content);
    content.zeroize();
    result.map_err(|e| format!("no se pudo escribir el archivo exportado: {e}"))
}

// ---- asociación N:M con pacientes ----

#[tauri::command]
pub fn link_library_resource_to_patient(resource_id: String, patient_id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| library::link_resource_to_patient(conn, &resource_id, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn unlink_library_resource_from_patient(resource_id: String, patient_id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| library::unlink_resource_from_patient(conn, &resource_id, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_patients_for_library_resource(resource_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<PatientSummary>, String> {
    state.with_connection(|conn| library::list_patients_for_resource(conn, &resource_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_library_resources_for_patient(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<LibraryResourceSummary>, String> {
    state.with_connection(|conn| library::list_resources_for_patient(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
