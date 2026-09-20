//! Reglas de negocio de la Biblioteca global de recursos (fase de continuación post-Fase 19).
//! Distinta de Documentos (Fase 16, por paciente): un recurso se sube **una sola vez** y puede
//! asociarse a cualquier número de pacientes sin duplicar nunca el archivo físico.
//!
//! El archivo de un recurso (si tiene uno) es una fila de `documents` con `patient_id = NULL` —
//! este módulo reutiliza exactamente `services::document_crypto` (cifrado/descifrado, rutas
//! opacas) y las mismas utilidades de `services::documents` (`sha256_hex`,
//! `guess_mime_from_extension`, `wrapped_key_to_columns`), nunca una segunda implementación de
//! cifrado ni una segunda tabla de archivos.
//!
//! Reglas clave (decisiones de esta fase, ninguna pedida explícitamente en sentido contrario):
//! - "Eliminar" un recurso es un **archivado reversible** (mismo criterio que Documentos,
//!   Pacientes, Procesos), nunca un borrado físico — evita el riesgo de que un `ON DELETE CASCADE`
//!   real borre en silencio asociaciones con pacientes. Si el recurso está asociado a algún
//!   paciente, `archive_resource` lo rechaza con `LinkedToPatients(count)` a menos que se pase
//!   `force: true` (la confirmación explícita de la usuaria tras ver la advertencia) — y aun con
//!   `force`, los enlaces en `library_resource_patients` **nunca** se tocan: solo el recurso pasa
//!   a archivado, restaurable en cualquier momento con sus asociaciones intactas.
//! - Vincular un recurso a un paciente **archivado** se rechaza (crear una asociación clínica
//!   nueva para un paciente archivado, mismo criterio que crear cualquier otro contenido nuevo en
//!   el resto del proyecto) — pero desvincular, o simplemente ver los recursos ya asociados, sigue
//!   permitido siempre.

use std::fmt;
use std::path::Path;

use rusqlite::Connection;
use serde::Deserialize;
use uuid::Uuid;

use crate::repositories::library::{self, LibraryResource, LibraryResourceMetadataUpdate, LibraryResourceSummary, NewLibraryResourceRow};
use crate::repositories::patients::{self, PatientSummary};
use crate::security::{FileKey, KeyWrapVersion, UnwrapFileKeyError, VaultSession, WrapFileKeyError, WrappedFileKey, FILE_KEY_WRAP_NONCE_LEN};
use crate::services::document_crypto::{self, DocumentCryptoError, StoragePathError};
use crate::services::documents::{guess_mime_from_extension, sha256_hex, wrapped_key_to_columns};

/// Taxonomía ya fijada por el `CHECK` de `library_resources` desde `SCHEMA_V1` — reflejada aquí
/// para dar un error de dominio claro antes de llegar a la violación cruda de SQL.
pub const VALID_RESOURCE_TYPES: &[&str] = &["articulo", "libro", "protocolo", "escala", "video", "enlace"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewLibraryResourceInput {
    pub title: String,
    pub resource_type: Option<String>,
    pub author: Option<String>,
    pub source_url: Option<String>,
    pub summary: Option<String>,
    /// Ruta absoluta del archivo de origen, ya elegido por la usuaria mediante el selector de
    /// archivos nativo — igual que `services::documents::NewDocumentInput::source_path`.
    /// `None` para un recurso que es solo un enlace/referencia sin archivo adjunto (p. ej.
    /// `resource_type = "enlace"` con `source_url` informado).
    pub source_path: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryResourceMetadataInput {
    pub title: String,
    pub resource_type: Option<String>,
    pub author: Option<String>,
    pub source_url: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug)]
pub enum LibraryValidationError {
    MissingTitle,
    InvalidResourceType(String),
    OriginalFilenameMissing,
}
impl fmt::Display for LibraryValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibraryValidationError::MissingTitle => write!(f, "el recurso necesita un título"),
            LibraryValidationError::InvalidResourceType(t) => write!(f, "tipo de recurso inválido: '{t}'"),
            LibraryValidationError::OriginalFilenameMissing => write!(f, "no se pudo determinar el nombre del archivo de origen"),
        }
    }
}
impl std::error::Error for LibraryValidationError {}

#[derive(Debug)]
pub enum LibraryError {
    Validation(LibraryValidationError),
    NotFound,
    PatientNotFound,
    PatientArchived,
    /// El recurso sigue asociado a `usize` paciente(s) — `archive_resource` se rechaza a menos
    /// que se llame de nuevo con `force: true` tras mostrar esta advertencia a la usuaria. Nunca
    /// se borran asociaciones en silencio.
    LinkedToPatients(usize),
    /// `hard_delete_resource` exige que el recurso esté archivado primero — nunca se ofrece
    /// "Eliminar permanentemente" como acción directa desde un recurso activo.
    MustBeArchivedFirst,
    /// Mismo caso que `LinkedToPatients`, pero para `hard_delete_resource`: a diferencia de
    /// `archive_resource`, aquí **no existe** un `force` que lo pase por alto — un borrado físico
    /// nunca puede dejar asociaciones apuntando a un recurso que ya no existe. La usuaria debe
    /// desvincular el recurso de cada paciente antes de poder eliminarlo.
    LinkedToPatientsBlocksHardDelete(usize),
    SourceFileNotFound,
    SourceFileEmpty,
    SourceFileTooLarge,
    VaultLocked,
    Crypto(DocumentCryptoError),
    CorruptCryptoMetadata,
    KeyUnwrapFailed,
    Io(std::io::Error),
    Database(rusqlite::Error),
}
impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibraryError::Validation(e) => write!(f, "{e}"),
            LibraryError::NotFound => write!(f, "recurso no encontrado"),
            LibraryError::PatientNotFound => write!(f, "paciente no encontrado"),
            LibraryError::PatientArchived => write!(f, "no se puede asociar un recurso a un paciente archivado"),
            LibraryError::LinkedToPatients(n) => write!(f, "este recurso está asociado a {n} paciente(s) — confirma para archivarlo de todas formas"),
            LibraryError::MustBeArchivedFirst => write!(f, "el recurso debe estar archivado antes de poder eliminarse permanentemente"),
            LibraryError::LinkedToPatientsBlocksHardDelete(n) => write!(f, "este recurso está asociado a {n} paciente(s). Quita esas asociaciones antes de eliminarlo permanentemente."),
            LibraryError::SourceFileNotFound => write!(f, "no se pudo leer el archivo de origen"),
            LibraryError::SourceFileEmpty => write!(f, "el archivo de origen está vacío"),
            LibraryError::SourceFileTooLarge => write!(f, "el archivo supera el tamaño máximo permitido (50 MB)"),
            LibraryError::VaultLocked => write!(f, "el vault está bloqueado"),
            LibraryError::Crypto(_) => write!(f, "no se pudo procesar el contenido cifrado del recurso"),
            LibraryError::CorruptCryptoMetadata => write!(f, "la metadata criptográfica del recurso es inválida"),
            LibraryError::KeyUnwrapFailed => write!(f, "no se pudo recuperar la clave de cifrado del recurso"),
            LibraryError::Io(_) => write!(f, "error de almacenamiento al procesar el recurso"),
            LibraryError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for LibraryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LibraryError::Crypto(e) => Some(e),
            LibraryError::Io(e) => Some(e),
            LibraryError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for LibraryError {
    fn from(e: rusqlite::Error) -> Self {
        LibraryError::Database(e)
    }
}
impl From<LibraryValidationError> for LibraryError {
    fn from(e: LibraryValidationError) -> Self {
        LibraryError::Validation(e)
    }
}
impl From<DocumentCryptoError> for LibraryError {
    fn from(e: DocumentCryptoError) -> Self {
        match e {
            DocumentCryptoError::Empty => LibraryError::SourceFileEmpty,
            DocumentCryptoError::TooLarge => LibraryError::SourceFileTooLarge,
            other => LibraryError::Crypto(other),
        }
    }
}
impl From<WrapFileKeyError> for LibraryError {
    fn from(e: WrapFileKeyError) -> Self {
        match e {
            WrapFileKeyError::Locked => LibraryError::VaultLocked,
            _ => LibraryError::KeyUnwrapFailed,
        }
    }
}
impl From<UnwrapFileKeyError> for LibraryError {
    fn from(e: UnwrapFileKeyError) -> Self {
        match e {
            UnwrapFileKeyError::Locked => LibraryError::VaultLocked,
            _ => LibraryError::KeyUnwrapFailed,
        }
    }
}
impl From<StoragePathError> for LibraryError {
    fn from(_: StoragePathError) -> Self {
        LibraryError::CorruptCryptoMetadata
    }
}
impl From<std::io::Error> for LibraryError {
    fn from(e: std::io::Error) -> Self {
        LibraryError::Io(e)
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

fn validate_title(title: String) -> Result<String, LibraryValidationError> {
    let trimmed = title.trim().to_string();
    if trimmed.is_empty() {
        return Err(LibraryValidationError::MissingTitle);
    }
    Ok(trimmed)
}

fn validate_resource_type(resource_type: Option<String>) -> Result<Option<String>, LibraryValidationError> {
    match none_if_blank(resource_type) {
        None => Ok(None),
        Some(t) if VALID_RESOURCE_TYPES.contains(&t.as_str()) => Ok(Some(t)),
        Some(t) => Err(LibraryValidationError::InvalidResourceType(t)),
    }
}

fn columns_to_wrapped_key(wrapped_file_dek: &str, wrap_nonce: &str) -> Result<WrappedFileKey, LibraryError> {
    use base64ct::{Base64, Encoding};
    let ciphertext = Base64::decode_vec(wrapped_file_dek).map_err(|_| LibraryError::CorruptCryptoMetadata)?;
    let nonce_bytes = Base64::decode_vec(wrap_nonce).map_err(|_| LibraryError::CorruptCryptoMetadata)?;
    let nonce: [u8; FILE_KEY_WRAP_NONCE_LEN] = nonce_bytes.try_into().map_err(|_| LibraryError::CorruptCryptoMetadata)?;
    Ok(WrappedFileKey { nonce, ciphertext })
}

/// Crea un recurso de Biblioteca nuevo. Si trae `source_path`, importa el archivo exactamente
/// como `services::documents::create_document` (cifrado propio con DEK aleatoria, escritura
/// atómica temporal+`rename` en `vault/files/`, fila de `documents` con `patient_id = NULL`) antes
/// de crear la fila de `library_resources` que lo referencia — mismo orden (el `INSERT` de
/// metadata siempre al final) por la misma razón: un crash a mitad de camino nunca deja una fila
/// apuntando a un archivo inexistente.
pub fn create_resource(session: &VaultSession, files_root: &Path, input: NewLibraryResourceInput) -> Result<LibraryResourceSummary, LibraryError> {
    let title = validate_title(input.title)?;
    let resource_type = validate_resource_type(input.resource_type)?;
    let author = none_if_blank(input.author);
    let source_url = none_if_blank(input.source_url);
    let summary = none_if_blank(input.summary);

    let file_document_id = match input.source_path {
        None => None,
        Some(source_path) => Some(import_document_for_resource(session, files_root, &source_path, input.mime_type)?),
    };

    let resource_id = Uuid::new_v4().to_string();
    let resource = session.with_connection(|conn| {
        library::insert(
            conn,
            &NewLibraryResourceRow {
                id: &resource_id,
                title: &title,
                resource_type: resource_type.as_deref(),
                author: author.as_deref(),
                source_url: source_url.as_deref(),
                file_document_id: file_document_id.as_deref(),
                summary: summary.as_deref(),
            },
        )
    });

    match resource {
        Ok(Ok(resource)) => Ok(library_summary(&resource)),
        Ok(Err(db_err)) => Err(LibraryError::Database(db_err)),
        Err(_locked) => Err(LibraryError::VaultLocked),
    }
}

fn library_summary(resource: &LibraryResource) -> LibraryResourceSummary {
    LibraryResourceSummary {
        id: resource.id.clone(),
        title: resource.title.clone(),
        resource_type: resource.resource_type.clone(),
        author: resource.author.clone(),
        source_url: resource.source_url.clone(),
        summary: resource.summary.clone(),
        file_document_id: resource.file_document_id.clone(),
        original_filename: None,
        mime_type: None,
        size_bytes: None,
        created_at: resource.created_at.clone(),
        updated_at: resource.updated_at.clone(),
    }
}

/// Cifra e inserta el archivo de origen como una fila de `documents` con `patient_id = NULL` —
/// exactamente la misma secuencia que `services::documents::create_document`, reutilizando
/// `document_crypto::*` sin ninguna variación. Devuelve el `id` del documento creado.
fn import_document_for_resource(session: &VaultSession, files_root: &Path, source_path: &str, mime_type: Option<String>) -> Result<String, LibraryError> {
    let source = Path::new(source_path);
    let original_filename = source.file_name().and_then(|n| n.to_str()).ok_or(LibraryValidationError::OriginalFilenameMissing)?.to_string();

    let metadata = std::fs::metadata(source).map_err(|_| LibraryError::SourceFileNotFound)?;
    if metadata.len() == 0 {
        return Err(LibraryError::SourceFileEmpty);
    }
    if metadata.len() > document_crypto::MAX_DOCUMENT_SIZE_BYTES {
        return Err(LibraryError::SourceFileTooLarge);
    }

    let mut plaintext = std::fs::read(source).map_err(|_| LibraryError::SourceFileNotFound)?;
    let sha256_plaintext = sha256_hex(&plaintext);
    let mime = none_if_blank(mime_type).unwrap_or_else(|| guess_mime_from_extension(&original_filename));

    let mut file_dek = document_crypto::generate_file_dek()?;
    let encrypted = document_crypto::encrypt_document(&plaintext, &file_dek);
    document_crypto::zeroize_plaintext(&mut plaintext);
    let encrypted = encrypted?;

    let wrapped = session.wrap_file_key(&file_dek)?;
    document_crypto::zeroize_plaintext(&mut file_dek);
    let (wrapped_file_dek, wrap_nonce) = wrapped_key_to_columns(&wrapped);

    let document_id = Uuid::new_v4();
    let storage_path = document_crypto::opaque_storage_path(document_id);
    let abs_path = document_crypto::resolve_within_files_root(files_root, &storage_path).expect("opaque_storage_path siempre produce un storage_path válido");
    let parent = abs_path.parent().expect("resolve_within_files_root siempre produce una ruta con padre");
    std::fs::create_dir_all(parent)?;

    let tmp_path = parent.join(format!("{document_id}.enc.tmp"));
    std::fs::write(&tmp_path, &encrypted)?;
    std::fs::rename(&tmp_path, &abs_path)?;

    let insert_result = session.with_connection(|conn| {
        crate::repositories::documents::insert_document(
            conn,
            &crate::repositories::documents::NewDocumentRow {
                id: &document_id.to_string(),
                patient_id: None,
                episode_id: None,
                session_id: None,
                category: None,
                original_filename: &original_filename,
                mime_type: &mime,
                size_bytes: metadata.len() as i64,
                sha256_plaintext: &sha256_plaintext,
                storage_path: &storage_path,
                description: None,
                wrapped_file_dek: &wrapped_file_dek,
                wrap_nonce: &wrap_nonce,
                key_wrap_version: KeyWrapVersion::DomainSeparated.as_i64(),
            },
        )
    });

    match insert_result {
        Ok(Ok(document)) => Ok(document.id),
        Ok(Err(db_err)) => {
            let _ = std::fs::remove_file(&abs_path);
            Err(LibraryError::Database(db_err))
        }
        Err(_locked) => {
            let _ = std::fs::remove_file(&abs_path);
            Err(LibraryError::VaultLocked)
        }
    }
}

pub fn get_resource(conn: &Connection, id: &str) -> Result<LibraryResourceSummary, LibraryError> {
    library::find_summary_by_id(conn, id)?.ok_or(LibraryError::NotFound)
}

pub fn list_active_resources(conn: &Connection) -> Result<Vec<LibraryResourceSummary>, LibraryError> {
    Ok(library::list_active(conn)?)
}

pub fn list_archived_resources(conn: &Connection) -> Result<Vec<LibraryResourceSummary>, LibraryError> {
    Ok(library::list_archived(conn)?)
}

/// Solo metadata bibliográfica — nunca el archivo adjunto (inmutable una vez creado el recurso).
pub fn update_resource_metadata(conn: &Connection, id: &str, input: LibraryResourceMetadataInput) -> Result<LibraryResourceSummary, LibraryError> {
    let title = validate_title(input.title)?;
    let resource_type = validate_resource_type(input.resource_type)?;
    let author = none_if_blank(input.author);
    let source_url = none_if_blank(input.source_url);
    let summary = none_if_blank(input.summary);
    library::update_metadata(
        conn,
        id,
        &LibraryResourceMetadataUpdate { title: &title, resource_type: resource_type.as_deref(), author: author.as_deref(), source_url: source_url.as_deref(), summary: summary.as_deref() },
    )?
    .ok_or(LibraryError::NotFound)?;
    get_resource(conn, id)
}

/// "Eliminar" un recurso global: archivado reversible, nunca borrado físico ni de sus
/// asociaciones. Si sigue vinculado a algún paciente y `force` es `false`, se rechaza con
/// `LinkedToPatients(count)` — el frontend muestra esa cantidad como advertencia explícita y
/// vuelve a llamar con `force: true` solo si la usuaria confirma. Los enlaces en
/// `library_resource_patients` nunca se tocan, con o sin `force`.
pub fn archive_resource(conn: &Connection, id: &str, force: bool) -> Result<LibraryResourceSummary, LibraryError> {
    get_resource(conn, id)?;
    if !force {
        let linked = library::count_patients_for_resource(conn, id)?;
        if linked > 0 {
            return Err(LibraryError::LinkedToPatients(linked as usize));
        }
    }
    if !library::archive(conn, id)? {
        return Err(LibraryError::NotFound);
    }
    get_resource(conn, id)
}

pub fn restore_resource(conn: &Connection, id: &str) -> Result<LibraryResourceSummary, LibraryError> {
    if !library::restore(conn, id)? {
        return Err(LibraryError::NotFound);
    }
    get_resource(conn, id)
}

/// Borrado físico e irreversible de un recurso — distinto de `archive_resource` (reversible). Solo
/// alcanzable desde "Archivados": exige que el recurso ya esté archivado (`MustBeArchivedFirst`
/// si no) y que no tenga ninguna asociación viva con un paciente (`LinkedToPatientsBlocksHardDelete`
/// si tiene alguna — a diferencia de `archive_resource`, aquí no existe un `force` que lo pase
/// por alto: un borrado físico nunca puede dejar una fila de `library_resource_patients`
/// apuntando a un recurso que ya no existe).
///
/// Modelo de atomicidad (ver CLAUDE.md regla 2 y la sección "Atomicidad" del pedido de esta
/// fase): la fila de `documents` (si el recurso tenía un archivo) y la fila de `library_resources`
/// se borran dentro de una única transacción SQL (`BEGIN IMMEDIATE`/`COMMIT`, con `ROLLBACK`
/// explícito ante cualquier error) — nunca queda un estado a medias en la base. El ciphertext del
/// archivo en disco se borra **después** de que la transacción confirma, en modo best-effort: si
/// ese borrado de archivo falla (permisos, etc.), el error se descarta silenciosamente y la base
/// queda de todas formas 100% consistente (ninguna fila apunta ya al archivo) — el resultado en el
/// peor caso es un ciphertext huérfano en disco, nunca una fila huérfana en la base ni un estado
/// parcial. Ese archivo huérfano es exactamente la clase de discrepancia que ya detecta (sin
/// borrar nada por sí solo) `services::documents::find_orphan_storage_paths`.
pub fn hard_delete_resource(session: &VaultSession, files_root: &Path, id: &str) -> Result<(), LibraryError> {
    let storage_path_to_delete: Option<String> = session
        .with_connection(|conn| -> Result<Option<String>, LibraryError> {
            let resource = library::find_by_id(conn, id)?.ok_or(LibraryError::NotFound)?;
            if resource.deleted_at.is_none() {
                return Err(LibraryError::MustBeArchivedFirst);
            }
            let linked = library::count_patients_for_resource(conn, id)?;
            if linked > 0 {
                return Err(LibraryError::LinkedToPatientsBlocksHardDelete(linked as usize));
            }

            let storage_path = match &resource.file_document_id {
                Some(doc_id) => crate::repositories::documents::find_document_by_id(conn, doc_id)?.map(|d| d.storage_path),
                None => None,
            };

            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result: rusqlite::Result<()> = (|| {
                if let Some(doc_id) = &resource.file_document_id {
                    crate::repositories::documents::delete_document_row(conn, doc_id)?;
                }
                library::hard_delete(conn, id)?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(storage_path)
                }
                Err(e) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(LibraryError::Database(e))
                }
            }
        })
        .map_err(|_locked| LibraryError::VaultLocked)??;

    if let Some(storage_path) = storage_path_to_delete {
        if let Ok(abs_path) = document_crypto::resolve_within_files_root(files_root, &storage_path) {
            let _ = std::fs::remove_file(&abs_path);
        }
    }
    Ok(())
}

/// Descifra el contenido del archivo adjunto de un recurso (para abrir/previsualizar) — igual
/// criterio que `services::documents::get_document_content`: nunca lo cachea ni decide qué hacer
/// con el resultado, eso lo decide `commands::library`.
pub fn get_resource_content(session: &VaultSession, files_root: &Path, id: &str) -> Result<(Vec<u8>, LibraryResourceSummary), LibraryError> {
    let summary = session.with_connection(|conn| library::find_summary_by_id(conn, id)).map_err(|_| LibraryError::VaultLocked)??.ok_or(LibraryError::NotFound)?;
    let document_id = summary.file_document_id.clone().ok_or(LibraryError::NotFound)?;

    let doc = session
        .with_connection(|conn| crate::repositories::documents::find_document_by_id(conn, &document_id))
        .map_err(|_| LibraryError::VaultLocked)??
        .ok_or(LibraryError::NotFound)?;

    if doc.format_version != document_crypto::FILE_FORMAT_VERSION as i64 {
        return Err(LibraryError::Crypto(DocumentCryptoError::UnknownFormatVersion(doc.format_version as u8)));
    }
    let abs_path = document_crypto::resolve_within_files_root(files_root, &doc.storage_path)?;
    let ciphertext = std::fs::read(&abs_path).map_err(|_| LibraryError::CorruptCryptoMetadata)?;
    let wrapped = columns_to_wrapped_key(&doc.wrapped_file_dek, &doc.wrap_nonce)?;
    let key_wrap_version = KeyWrapVersion::try_from(doc.key_wrap_version).map_err(|_| LibraryError::CorruptCryptoMetadata)?;
    let file_key: FileKey = session.unwrap_file_key(&wrapped, key_wrap_version)?;
    let plaintext = document_crypto::decrypt_document(&ciphertext, file_key.expose_secret())?;

    if sha256_hex(&plaintext) != doc.sha256_plaintext {
        return Err(LibraryError::CorruptCryptoMetadata);
    }

    Ok((plaintext, summary))
}

// ---- asociación N:M con pacientes ----

fn require_existing_patient(conn: &Connection, patient_id: &str) -> Result<(), LibraryError> {
    patients::find_by_id(conn, patient_id)?.ok_or(LibraryError::PatientNotFound)?;
    Ok(())
}

/// Asocia un recurso a un paciente. Rechaza un paciente archivado (crear una asociación clínica
/// nueva) y un recurso archivado (reactivarlo primero) — pero es idempotente si el enlace ya
/// existía. Puede llamarse desde la ficha del paciente o desde el recurso: es la misma operación.
pub fn link_resource_to_patient(conn: &Connection, resource_id: &str, patient_id: &str) -> Result<(), LibraryError> {
    let resource = library::find_by_id(conn, resource_id)?.ok_or(LibraryError::NotFound)?;
    if resource.deleted_at.is_some() {
        return Err(LibraryError::NotFound);
    }
    let patient = patients::find_by_id(conn, patient_id)?.ok_or(LibraryError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(LibraryError::PatientArchived);
    }
    library::link_to_patient(conn, resource_id, patient_id)?;
    Ok(())
}

/// Desvincular siempre se permite, sin importar si el recurso o el paciente están archivados —
/// mismo criterio que el resto del proyecto: deshacer una asociación nunca queda bloqueado por un
/// archivado posterior.
pub fn unlink_resource_from_patient(conn: &Connection, resource_id: &str, patient_id: &str) -> Result<(), LibraryError> {
    library::find_by_id(conn, resource_id)?.ok_or(LibraryError::NotFound)?;
    require_existing_patient(conn, patient_id)?;
    library::unlink_from_patient(conn, resource_id, patient_id)?;
    Ok(())
}

pub fn list_patients_for_resource(conn: &Connection, resource_id: &str) -> Result<Vec<PatientSummary>, LibraryError> {
    library::find_by_id(conn, resource_id)?.ok_or(LibraryError::NotFound)?;
    Ok(library::list_patients_for_resource(conn, resource_id)?)
}

pub fn list_resources_for_patient(conn: &Connection, patient_id: &str) -> Result<Vec<LibraryResourceSummary>, LibraryError> {
    require_existing_patient(conn, patient_id)?;
    Ok(library::list_resources_for_patient(conn, patient_id)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::VaultSession;
    use crate::services::patients::{self as patients_service, PatientInput};

    fn test_session(name: &str) -> (VaultSession, std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("cc-library-svc-test-{}-{}", std::process::id(), name));
        if base.exists() {
            std::fs::remove_dir_all(&base).unwrap();
        }
        let vault_dir = base.join("vault");
        std::fs::create_dir_all(&vault_dir).unwrap();
        let files_root = vault_dir.join("files");

        let session = VaultSession::new(&vault_dir);
        session.begin_creation("una-contraseña-de-prueba-larga-1").unwrap();
        session.confirm_creation().unwrap();

        let sources_dir = base.join("sources");
        std::fs::create_dir_all(&sources_dir).unwrap();
        (session, files_root, sources_dir)
    }

    fn write_source_file(sources_dir: &Path, filename: &str, contents: &[u8]) -> String {
        let path = sources_dir.join(filename);
        std::fs::write(&path, contents).unwrap();
        path.to_str().unwrap().to_string()
    }

    fn create_test_patient(session: &VaultSession, name: &str) -> String {
        let input = PatientInput {
            full_name: name.to_string(),
            preferred_name: None,
            rut: None,
            birth_date: None,
            phone: None,
            email: None,
            address: None,
            emergency_contact_name: None,
            emergency_contact_phone: None,
            emergency_contact_relationship: None,
            status: None,
            referred_by: None,
            intake_date: None,
            region: None,
            commune: None,
        };
        session.with_connection(|conn| patients_service::create_patient(conn, input)).unwrap().unwrap().id
    }

    fn minimal_input(title: &str) -> NewLibraryResourceInput {
        NewLibraryResourceInput { title: title.to_string(), resource_type: None, author: None, source_url: None, summary: None, source_path: None, mime_type: None }
    }

    #[test]
    fn creates_a_resource_without_a_file() {
        let (session, files_root, _sources_dir) = test_session("create-no-file");
        let resource = create_resource(&session, &files_root, minimal_input("Solo enlace")).unwrap();
        assert_eq!(resource.title, "Solo enlace");
        assert!(resource.file_document_id.is_none());
    }

    #[test]
    fn rejects_a_blank_title() {
        let (session, files_root, _sources_dir) = test_session("create-blank-title");
        let err = create_resource(&session, &files_root, minimal_input("   ")).unwrap_err();
        assert!(matches!(err, LibraryError::Validation(LibraryValidationError::MissingTitle)));
    }

    #[test]
    fn rejects_an_invalid_resource_type() {
        let (session, files_root, _sources_dir) = test_session("create-invalid-type");
        let mut input = minimal_input("Recurso");
        input.resource_type = Some("no-existe".to_string());
        let err = create_resource(&session, &files_root, input).unwrap_err();
        assert!(matches!(err, LibraryError::Validation(LibraryValidationError::InvalidResourceType(t)) if t == "no-existe"));
    }

    #[test]
    fn creates_a_resource_with_a_file_and_its_content_round_trips() {
        let (session, files_root, sources_dir) = test_session("create-with-file");
        let source_path = write_source_file(&sources_dir, "guia.txt", b"contenido ficticio de una guia");

        let mut input = minimal_input("Guia con archivo");
        input.source_path = Some(source_path);
        let resource = create_resource(&session, &files_root, input).unwrap();
        assert!(resource.file_document_id.is_some());

        let (content, summary) = get_resource_content(&session, &files_root, &resource.id).unwrap();
        assert_eq!(content, b"contenido ficticio de una guia");
        assert_eq!(summary.original_filename.as_deref(), Some("guia.txt"));
    }

    #[test]
    fn rejects_an_empty_source_file() {
        let (session, files_root, sources_dir) = test_session("create-empty-file");
        let source_path = write_source_file(&sources_dir, "vacio.txt", b"");
        let mut input = minimal_input("Vacío");
        input.source_path = Some(source_path);
        let err = create_resource(&session, &files_root, input).unwrap_err();
        assert!(matches!(err, LibraryError::SourceFileEmpty));
    }

    #[test]
    fn update_metadata_never_touches_the_file() {
        let (session, files_root, _sources_dir) = test_session("update-metadata");
        let resource = create_resource(&session, &files_root, minimal_input("Original")).unwrap();
        let updated = session
            .with_connection(|conn| {
                update_resource_metadata(
                    conn,
                    &resource.id,
                    LibraryResourceMetadataInput { title: "Editado".to_string(), resource_type: Some("protocolo".to_string()), author: None, source_url: None, summary: None },
                )
            })
            .unwrap()
            .unwrap();
        assert_eq!(updated.title, "Editado");
        assert!(updated.file_document_id.is_none());
    }

    #[test]
    fn archive_and_restore_round_trip() {
        let (session, files_root, _sources_dir) = test_session("archive-restore");
        let resource = create_resource(&session, &files_root, minimal_input("Recurso")).unwrap();
        session.with_connection(|conn| archive_resource(conn, &resource.id, false)).unwrap().unwrap();
        assert!(session.with_connection(list_active_resources).unwrap().unwrap().is_empty());
        session.with_connection(|conn| restore_resource(conn, &resource.id)).unwrap().unwrap();
        assert_eq!(session.with_connection(list_active_resources).unwrap().unwrap().len(), 1);
    }

    #[test]
    fn rejects_archiving_a_linked_resource_without_force_but_allows_it_with_force() {
        let (session, files_root, _sources_dir) = test_session("archive-linked");
        let resource = create_resource(&session, &files_root, minimal_input("Recurso")).unwrap();
        let patient_id = create_test_patient(&session, "Paciente Uno");
        session.with_connection(|conn| link_resource_to_patient(conn, &resource.id, &patient_id)).unwrap().unwrap();

        let err = session.with_connection(|conn| archive_resource(conn, &resource.id, false)).unwrap().unwrap_err();
        assert!(matches!(err, LibraryError::LinkedToPatients(1)));

        session.with_connection(|conn| archive_resource(conn, &resource.id, true)).unwrap().unwrap();

        // El enlace nunca se toca, con o sin `force`.
        let linked = session.with_connection(|conn| library::count_patients_for_resource(conn, &resource.id)).unwrap().unwrap();
        assert_eq!(linked, 1, "archivar (incluso forzado) nunca borra las asociaciones existentes");
    }

    #[test]
    fn links_and_unlinks_a_resource_to_a_patient() {
        let (session, files_root, _sources_dir) = test_session("link-unlink");
        let resource = create_resource(&session, &files_root, minimal_input("Recurso")).unwrap();
        let patient_id = create_test_patient(&session, "Paciente Uno");

        session.with_connection(|conn| link_resource_to_patient(conn, &resource.id, &patient_id)).unwrap().unwrap();
        let resources = session.with_connection(|conn| list_resources_for_patient(conn, &patient_id)).unwrap().unwrap();
        assert_eq!(resources.len(), 1);
        let patients = session.with_connection(|conn| list_patients_for_resource(conn, &resource.id)).unwrap().unwrap();
        assert_eq!(patients.len(), 1);

        session.with_connection(|conn| unlink_resource_from_patient(conn, &resource.id, &patient_id)).unwrap().unwrap();
        let resources = session.with_connection(|conn| list_resources_for_patient(conn, &patient_id)).unwrap().unwrap();
        assert!(resources.is_empty());
    }

    #[test]
    fn rejects_linking_a_resource_to_an_archived_patient() {
        let (session, files_root, _sources_dir) = test_session("link-archived-patient");
        let resource = create_resource(&session, &files_root, minimal_input("Recurso")).unwrap();
        let patient_id = create_test_patient(&session, "Paciente Archivado");
        session.with_connection(|conn| patients_service::archive_patient(conn, &patient_id)).unwrap().unwrap();

        let err = session.with_connection(|conn| link_resource_to_patient(conn, &resource.id, &patient_id)).unwrap().unwrap_err();
        assert!(matches!(err, LibraryError::PatientArchived));
    }

    #[test]
    fn unlinking_is_still_allowed_after_the_patient_is_archived() {
        let (session, files_root, _sources_dir) = test_session("unlink-archived-patient");
        let resource = create_resource(&session, &files_root, minimal_input("Recurso")).unwrap();
        let patient_id = create_test_patient(&session, "Paciente Uno");
        session.with_connection(|conn| link_resource_to_patient(conn, &resource.id, &patient_id)).unwrap().unwrap();
        session.with_connection(|conn| patients_service::archive_patient(conn, &patient_id)).unwrap().unwrap();

        session.with_connection(|conn| unlink_resource_from_patient(conn, &resource.id, &patient_id)).unwrap().unwrap();
        let linked = session.with_connection(|conn| library::count_patients_for_resource(conn, &resource.id)).unwrap().unwrap();
        assert_eq!(linked, 0);
    }

    #[test]
    fn rejects_linking_a_nonexistent_resource_or_patient() {
        let (session, files_root, _sources_dir) = test_session("link-not-found");
        let resource = create_resource(&session, &files_root, minimal_input("Recurso")).unwrap();
        let patient_id = create_test_patient(&session, "Paciente Uno");

        let err = session.with_connection(|conn| link_resource_to_patient(conn, "no-existe", &patient_id)).unwrap().unwrap_err();
        assert!(matches!(err, LibraryError::NotFound));

        let err = session.with_connection(|conn| link_resource_to_patient(conn, &resource.id, "no-existe")).unwrap().unwrap_err();
        assert!(matches!(err, LibraryError::PatientNotFound));
    }

    #[test]
    fn an_archived_resource_still_lists_its_linked_patients() {
        let (session, files_root, _sources_dir) = test_session("archived-resource-lists-patients");
        let resource = create_resource(&session, &files_root, minimal_input("Recurso")).unwrap();
        let patient_id = create_test_patient(&session, "Paciente Uno");
        session.with_connection(|conn| link_resource_to_patient(conn, &resource.id, &patient_id)).unwrap().unwrap();
        session.with_connection(|conn| archive_resource(conn, &resource.id, true)).unwrap().unwrap();

        let patients = session.with_connection(|conn| list_patients_for_resource(conn, &resource.id)).unwrap().unwrap();
        assert_eq!(patients.len(), 1, "archivar el recurso no debe ocultar sus asociaciones ya existentes");
    }

    // ---- hard_delete_resource (FASE 2A) ----

    #[test]
    fn hard_delete_rejects_a_resource_that_is_not_archived() {
        let (session, files_root, _sources_dir) = test_session("hard-delete-not-archived");
        let resource = create_resource(&session, &files_root, minimal_input("Activo")).unwrap();
        let err = hard_delete_resource(&session, &files_root, &resource.id).unwrap_err();
        assert!(matches!(err, LibraryError::MustBeArchivedFirst));
        // nunca borra nada si rechaza
        assert!(session.with_connection(|conn| get_resource(conn, &resource.id)).unwrap().is_ok());
    }

    #[test]
    fn hard_delete_blocks_when_still_linked_to_a_patient_with_no_force_option() {
        let (session, files_root, _sources_dir) = test_session("hard-delete-blocked-linked");
        let resource = create_resource(&session, &files_root, minimal_input("Con vínculo")).unwrap();
        let patient_id = create_test_patient(&session, "Paciente Uno");
        session.with_connection(|conn| link_resource_to_patient(conn, &resource.id, &patient_id)).unwrap().unwrap();
        session.with_connection(|conn| archive_resource(conn, &resource.id, true)).unwrap().unwrap();

        let err = hard_delete_resource(&session, &files_root, &resource.id).unwrap_err();
        assert!(matches!(err, LibraryError::LinkedToPatientsBlocksHardDelete(1)));
        // el recurso archivado sigue existiendo, y el vínculo también
        let linked = session.with_connection(|conn| library::count_patients_for_resource(conn, &resource.id)).unwrap().unwrap();
        assert_eq!(linked, 1);
    }

    #[test]
    fn hard_delete_succeeds_after_unlinking_from_every_patient() {
        let (session, files_root, _sources_dir) = test_session("hard-delete-after-unlink");
        let resource = create_resource(&session, &files_root, minimal_input("Recurso")).unwrap();
        let patient_id = create_test_patient(&session, "Paciente Uno");
        session.with_connection(|conn| link_resource_to_patient(conn, &resource.id, &patient_id)).unwrap().unwrap();
        session.with_connection(|conn| archive_resource(conn, &resource.id, true)).unwrap().unwrap();
        session.with_connection(|conn| unlink_resource_from_patient(conn, &resource.id, &patient_id)).unwrap().unwrap();

        hard_delete_resource(&session, &files_root, &resource.id).unwrap();
        let err = session.with_connection(|conn| get_resource(conn, &resource.id)).unwrap().unwrap_err();
        assert!(matches!(err, LibraryError::NotFound));
    }

    #[test]
    fn hard_delete_removes_a_resource_without_a_file() {
        let (session, files_root, _sources_dir) = test_session("hard-delete-no-file");
        let resource = create_resource(&session, &files_root, minimal_input("Solo enlace")).unwrap();
        session.with_connection(|conn| archive_resource(conn, &resource.id, false)).unwrap().unwrap();

        hard_delete_resource(&session, &files_root, &resource.id).unwrap();
        let err = session.with_connection(|conn| get_resource(conn, &resource.id)).unwrap().unwrap_err();
        assert!(matches!(err, LibraryError::NotFound));
    }

    #[test]
    fn hard_delete_removes_a_resource_with_a_file_and_its_ciphertext_from_disk() {
        let (session, files_root, sources_dir) = test_session("hard-delete-with-file");
        let source_path = write_source_file(&sources_dir, "guia.txt", b"contenido ficticio");
        let mut input = minimal_input("Con archivo");
        input.source_path = Some(source_path);
        let resource = create_resource(&session, &files_root, input).unwrap();
        let document_id = resource.file_document_id.clone().unwrap();

        let abs_path = session
            .with_connection(|conn| crate::repositories::documents::find_document_by_id(conn, &document_id))
            .unwrap()
            .unwrap()
            .map(|d| document_crypto::resolve_within_files_root(&files_root, &d.storage_path).unwrap())
            .unwrap();
        assert!(abs_path.exists(), "el ciphertext debe existir antes de borrar");

        session.with_connection(|conn| archive_resource(conn, &resource.id, false)).unwrap().unwrap();
        hard_delete_resource(&session, &files_root, &resource.id).unwrap();

        assert!(!abs_path.exists(), "el ciphertext debe borrarse del disco");
        let err = session.with_connection(|conn| get_resource(conn, &resource.id)).unwrap().unwrap_err();
        assert!(matches!(err, LibraryError::NotFound));
        // ninguna fila huérfana en documents apuntando al recurso borrado
        let doc_gone = session.with_connection(|conn| crate::repositories::documents::find_document_by_id(conn, &document_id)).unwrap().unwrap();
        assert!(doc_gone.is_none());
    }

    #[test]
    fn hard_delete_never_touches_other_resources() {
        let (session, files_root, _sources_dir) = test_session("hard-delete-isolated");
        let survivor = create_resource(&session, &files_root, minimal_input("Sobrevive")).unwrap();
        let victim = create_resource(&session, &files_root, minimal_input("Se borra")).unwrap();
        session.with_connection(|conn| archive_resource(conn, &victim.id, false)).unwrap().unwrap();

        hard_delete_resource(&session, &files_root, &victim.id).unwrap();

        let active = session.with_connection(list_active_resources).unwrap().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, survivor.id);
    }
}
