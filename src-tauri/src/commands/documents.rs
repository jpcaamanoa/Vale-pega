//! Comandos Tauri de Documentos y adjuntos clínicos cifrados (Fase 16). Capa fina: cada uno
//! obtiene `files_root` exactamente como `commands::backup` obtiene `vault_dir`, y delega toda la
//! lógica real a `services::documents`/`services::document_crypto`. Nunca decide nada de
//! criptografía por sí sola.
//!
//! Ningún comando de este archivo importa nada de `calendar::*` — los documentos nunca se
//! sincronizan con Google Calendar.

use std::path::PathBuf;
use std::sync::Arc;

use base64ct::{Base64, Encoding};
use tauri::{AppHandle, Manager, State};
use zeroize::Zeroize;

use crate::repositories::documents::DocumentSummary;
use crate::security::VaultSession;
use crate::services::document_temp::DocumentTempRegistry;
use crate::services::documents::{self, DocumentConsistencyReport, DocumentMetadataInput, NewDocumentInput};

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
pub fn create_document(app: AppHandle, input: NewDocumentInput, state: State<'_, SharedVaultSession>) -> Result<DocumentSummary, String> {
    let root = files_root(&app)?;
    documents::create_document(&state, &root, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_document(id: String, state: State<'_, SharedVaultSession>) -> Result<DocumentSummary, String> {
    state.with_connection(|conn| documents::get_document(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Documentos activos del paciente — nunca lleva `storage_path` ni ningún detalle criptográfico
/// (`DocumentSummary`, ver `repositories::documents`).
#[tauri::command]
pub fn list_documents(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<DocumentSummary>, String> {
    state.with_connection(|conn| documents::list_documents(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_archived_documents(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<DocumentSummary>, String> {
    state.with_connection(|conn| documents::list_archived_documents(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Solo categoría/descripción — nunca el contenido ni ninguna asociación (paciente/proceso/
/// sesión son inmutables tras crear el documento).
#[tauri::command]
pub fn update_document_metadata(id: String, input: DocumentMetadataInput, state: State<'_, SharedVaultSession>) -> Result<DocumentSummary, String> {
    state.with_connection(|conn| documents::update_document_metadata(conn, &id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_document(id: String, state: State<'_, SharedVaultSession>) -> Result<DocumentSummary, String> {
    state.with_connection(|conn| documents::archive_document(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_document(id: String, state: State<'_, SharedVaultSession>) -> Result<DocumentSummary, String> {
    state.with_connection(|conn| documents::restore_document(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Estrategia híbrida por MIME (Bloque 20 de la aprobación) — este comando cubre el caso de las
/// imágenes: descifra en memoria y entrega un `data:` URL, sin escribir ningún archivo temporal
/// en disco en ningún momento. El frontend decide cuándo llamarlo según el MIME del documento.
#[tauri::command]
pub fn get_document_data_url(app: AppHandle, id: String, state: State<'_, SharedVaultSession>) -> Result<String, String> {
    let root = files_root(&app)?;
    let (mut content, summary) = documents::get_document_content(&state, &root, &id).map_err(|e| e.to_string())?;
    let encoded = Base64::encode_string(&content);
    content.zeroize();
    Ok(format!("data:{};base64,{}", summary.mime_type, encoded))
}

/// Estrategia híbrida por MIME (Bloque 20) — cubre PDF/DOCX/otros formatos: descifra a un
/// temporal de nombre opaco en el directorio temporal del sistema (nunca dentro de `vault/
/// files/`) y lo abre con la aplicación por defecto del sistema operativo (crate `open`, ya en
/// uso en `calendar::oauth` para el flujo OAuth — sin dependencia nueva). El temporal queda
/// registrado en `DocumentTempRegistry` para limpiarse al bloquear el vault o cerrar la
/// aplicación (Bloque 21 de la aprobación).
#[tauri::command]
pub fn open_document_externally(app: AppHandle, id: String, state: State<'_, SharedVaultSession>, temp_registry: State<'_, SharedTempRegistry>) -> Result<(), String> {
    let root = files_root(&app)?;
    let (mut content, summary) = documents::get_document_content(&state, &root, &id).map_err(|e| e.to_string())?;
    let extension = summary.original_filename.rsplit_once('.').map(|(_, ext)| ext).filter(|e| e.len() <= 10);
    let temp_path = temp_registry.allocate(extension);
    let result = std::fs::write(&temp_path, &content);
    content.zeroize();
    result.map_err(|e| format!("no se pudo escribir el archivo temporal: {e}"))?;
    open::that(&temp_path).map_err(|e| format!("no se pudo abrir el documento: {e}"))
}

/// "Exportar copia" (Bloque 22 de la aprobación) — acción explícita y consciente de la usuaria,
/// distinta de "abrir": escribe el contenido descifrado directamente en el destino que ella
/// eligió (mediante el diálogo nativo de guardar, ya usado por Backup — sin dependencia nueva),
/// fuera del perímetro cifrado del vault. Nunca ocurre automáticamente ni como consecuencia de
/// abrir/visualizar un documento.
#[tauri::command]
pub fn export_document(app: AppHandle, id: String, destination_path: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    let root = files_root(&app)?;
    let (mut content, _summary) = documents::get_document_content(&state, &root, &id).map_err(|e| e.to_string())?;
    let result = std::fs::write(&destination_path, &content);
    content.zeroize();
    result.map_err(|e| format!("no se pudo escribir el archivo exportado: {e}"))
}

/// Diagnóstico manual de consistencia DB/filesystem (Bloque 12/34) — nunca borra ni repara nada
/// por sí solo, solo informa cuántos casos de cada tipo existen.
#[tauri::command]
pub fn check_document_consistency(app: AppHandle, state: State<'_, SharedVaultSession>) -> Result<DocumentConsistencyReport, String> {
    let root = files_root(&app)?;
    documents::check_consistency(&state, &root).map_err(|e| e.to_string())
}
