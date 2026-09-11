//! Reglas de negocio de Documentos y adjuntos clínicos cifrados (Fase 16). Ver
//! `docs/documents.md` para el diseño completo y
//! `Plan-Fase-16-Documentos-cifrados-pendiente-de-aprobacion.md` para la auditoría previa.
//!
//! A diferencia de la mayoría de los verticales del proyecto, varias funciones de este módulo
//! necesitan más que una `&Connection` — también necesitan la capacidad de envolver/desenvolver
//! una DEK de archivo (`security::VaultSession::wrap_file_key`/`unwrap_file_key`) y acceso al
//! directorio `vault/files/` en disco. Mismo patrón ya establecido por
//! `backup::service::create_backup`/`restore_backup` (Fase 10): estas funciones reciben
//! `&VaultSession` y `files_root: &Path` directamente, en vez del `&Connection` simple del resto
//! de los verticales — es una excepción de arquitectura ya precedente, no una nueva.
//!
//! Reglas de negocio clave de esta fase (todas explícitamente aprobadas):
//! - `episode_id`/`session_id` son opcionales — un documento puede ser longitudinal del
//!   paciente, sin proceso ni sesión concretos (a diferencia de Formulación, Fase 15).
//! - Un proceso **cerrado** SÍ puede recibir documentos nuevos (a diferencia de Formulación) —
//!   documentación administrativa o clínica recibida después del cierre.
//! - Un paciente **archivado** NO puede recibir documentos nuevos — mismo criterio que
//!   Pagos/Tareas/Evaluaciones: solo bloquea creación nueva, nunca oculta ni bloquea lo ya
//!   existente (editar metadata, archivar/restaurar, leer/exportar siguen disponibles).
//! - El contenido de un documento es inmutable una vez importado: no existe ninguna función de
//!   "reemplazar el archivo" — solo metadata (categoría/descripción) es editable.

use std::fmt;
use std::path::Path;

use base64ct::{Base64, Encoding};
use rusqlite::Connection;
use serde::Deserialize;
use uuid::Uuid;

use crate::repositories::documents::{self, Document, DocumentMetadataUpdate, DocumentSummary, NewDocumentRow};
use crate::repositories::patients;
use crate::repositories::sessions;
use crate::repositories::treatment_episodes as episodes_repo;
use crate::security::{FileKey, KeyWrapVersion, UnwrapFileKeyError, VaultSession, WrapFileKeyError, WrappedFileKey, FILE_KEY_WRAP_NONCE_LEN};
use crate::services::document_crypto::{self, DocumentCryptoError, StoragePathError};

/// Categorías administrativas válidas (Bloque 13 de la aprobación: se agrega `derivacion` a las
/// seis ya existentes desde `SCHEMA_V1`). Taxonomía deliberadamente pequeña y administrativa,
/// nunca diagnóstica — mismo `CHECK` reflejado aquí en Rust para dar un error de dominio claro
/// antes de llegar a la violación cruda de SQL.
pub const VALID_CATEGORIES: &[&str] = &["informe", "consentimiento", "evaluacion_adjunta", "receta", "correspondencia", "derivacion", "otro"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDocumentInput {
    pub patient_id: String,
    pub episode_id: Option<String>,
    pub session_id: Option<String>,
    pub category: Option<String>,
    pub description: Option<String>,
    /// Ruta absoluta del archivo de origen, ya elegido por la usuaria mediante el selector de
    /// archivos nativo (Bloque 25 de la aprobación — infraestructura ya existente, sin
    /// dependencia nueva). Cuaderno Clínico **lee** ese archivo (que puede estar en plano, porque
    /// le pertenece a la usuaria y vive fuera del vault) y escribe exclusivamente ciphertext
    /// dentro de `vault/files/` — nunca crea una copia intermedia en plano de ningún tipo
    /// (Bloque 11 de la aprobación).
    pub source_path: String,
    /// Informativo únicamente — nunca se usa para construir ninguna ruta ni para decidir qué
    /// hacer con el archivo (Bloque 30 de la aprobación). Si no se informa (o viene vacío), se
    /// adivina a partir de la extensión del archivo de origen.
    pub mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadataInput {
    pub category: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug)]
pub enum DocumentValidationError {
    /// El archivo de origen no tiene un nombre reconocible (ruta termina en `/`, o similar).
    OriginalFilenameMissing,
    InvalidCategory(String),
}
impl fmt::Display for DocumentValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentValidationError::OriginalFilenameMissing => write!(f, "no se pudo determinar el nombre del archivo de origen"),
            DocumentValidationError::InvalidCategory(c) => write!(f, "categoría de documento inválida: {c}"),
        }
    }
}
impl std::error::Error for DocumentValidationError {}

#[derive(Debug)]
pub enum DocumentError {
    Validation(DocumentValidationError),
    NotFound,
    PatientNotFound,
    PatientArchived,
    EpisodeNotFound,
    EpisodeArchived,
    EpisodePatientMismatch,
    SessionNotFound,
    SessionPatientMismatch,
    /// El proceso indicado y el proceso de la sesión indicada no coinciden — Bloque 8 de la
    /// aprobación: "evita combinaciones contradictorias".
    SessionEpisodeMismatch,
    SourceFileNotFound,
    SourceFileEmpty,
    SourceFileTooLarge,
    VaultLocked,
    Crypto(DocumentCryptoError),
    /// La metadata criptográfica almacenada (`wrapped_file_dek`/`wrap_nonce`) no tiene el
    /// formato esperado — no debería ocurrir nunca con datos escritos por esta misma aplicación;
    /// indica corrupción de la fila o una restauración incompleta.
    CorruptCryptoMetadata,
    KeyUnwrapFailed,
    Io(std::io::Error),
    Database(rusqlite::Error),
}
impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentError::Validation(e) => write!(f, "{e}"),
            DocumentError::NotFound => write!(f, "documento no encontrado"),
            DocumentError::PatientNotFound => write!(f, "paciente no encontrado"),
            DocumentError::PatientArchived => write!(f, "no se pueden importar documentos nuevos para un paciente archivado"),
            DocumentError::EpisodeNotFound => write!(f, "proceso terapéutico no encontrado"),
            DocumentError::EpisodeArchived => write!(f, "este proceso está archivado y no puede recibir documentos nuevos"),
            DocumentError::EpisodePatientMismatch => write!(f, "el proceso indicado pertenece a otro paciente"),
            DocumentError::SessionNotFound => write!(f, "sesión no encontrada"),
            DocumentError::SessionPatientMismatch => write!(f, "la sesión indicada pertenece a otro paciente"),
            DocumentError::SessionEpisodeMismatch => write!(f, "la sesión indicada pertenece a un proceso distinto del indicado"),
            DocumentError::SourceFileNotFound => write!(f, "no se pudo leer el archivo de origen"),
            DocumentError::SourceFileEmpty => write!(f, "el archivo de origen está vacío"),
            DocumentError::SourceFileTooLarge => write!(f, "el archivo supera el tamaño máximo permitido (50 MB)"),
            DocumentError::VaultLocked => write!(f, "el vault está bloqueado"),
            DocumentError::Crypto(_) => write!(f, "no se pudo procesar el contenido cifrado del documento"),
            DocumentError::CorruptCryptoMetadata => write!(f, "la metadata criptográfica del documento es inválida"),
            DocumentError::KeyUnwrapFailed => write!(f, "no se pudo recuperar la clave de cifrado del documento"),
            DocumentError::Io(_) => write!(f, "error de almacenamiento al procesar el documento"),
            DocumentError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DocumentError::Crypto(e) => Some(e),
            DocumentError::Io(e) => Some(e),
            DocumentError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for DocumentError {
    fn from(e: rusqlite::Error) -> Self {
        DocumentError::Database(e)
    }
}
impl From<DocumentValidationError> for DocumentError {
    fn from(e: DocumentValidationError) -> Self {
        DocumentError::Validation(e)
    }
}
impl From<DocumentCryptoError> for DocumentError {
    fn from(e: DocumentCryptoError) -> Self {
        match e {
            DocumentCryptoError::Empty => DocumentError::SourceFileEmpty,
            DocumentCryptoError::TooLarge => DocumentError::SourceFileTooLarge,
            other => DocumentError::Crypto(other),
        }
    }
}
impl From<WrapFileKeyError> for DocumentError {
    fn from(e: WrapFileKeyError) -> Self {
        match e {
            WrapFileKeyError::Locked => DocumentError::VaultLocked,
            _ => DocumentError::KeyUnwrapFailed,
        }
    }
}
impl From<UnwrapFileKeyError> for DocumentError {
    fn from(e: UnwrapFileKeyError) -> Self {
        match e {
            UnwrapFileKeyError::Locked => DocumentError::VaultLocked,
            _ => DocumentError::KeyUnwrapFailed,
        }
    }
}
impl From<StoragePathError> for DocumentError {
    fn from(_: StoragePathError) -> Self {
        // Nunca debería ocurrir con un storage_path escrito por esta misma
        // aplicación — si ocurre, la fila está corrupta o fue manipulada.
        DocumentError::CorruptCryptoMetadata
    }
}
impl From<std::io::Error> for DocumentError {
    fn from(e: std::io::Error) -> Self {
        DocumentError::Io(e)
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

fn validate_category(category: Option<String>) -> Result<Option<String>, DocumentValidationError> {
    match none_if_blank(category) {
        None => Ok(None),
        Some(c) if VALID_CATEGORIES.contains(&c.as_str()) => Ok(Some(c)),
        Some(c) => Err(DocumentValidationError::InvalidCategory(c)),
    }
}

/// Adivina un MIME razonable a partir de la extensión — nunca a partir de contenido (no hay
/// "sniffing" de bytes mágicos en esta fase, ver `docs/documents.md`). Únicamente informativo:
/// nunca se usa para decidir cómo abrir, dónde escribir, ni qué comando ejecutar con el archivo
/// (Bloque 30 de la aprobación).
fn guess_mime_from_extension(filename: &str) -> String {
    let ext = filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" => "text/plain",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn wrapped_key_to_columns(wrapped: &WrappedFileKey) -> (String, String) {
    (Base64::encode_string(&wrapped.ciphertext), Base64::encode_string(&wrapped.nonce))
}

fn columns_to_wrapped_key(wrapped_file_dek: &str, wrap_nonce: &str) -> Result<WrappedFileKey, DocumentError> {
    let ciphertext = Base64::decode_vec(wrapped_file_dek).map_err(|_| DocumentError::CorruptCryptoMetadata)?;
    let nonce_bytes = Base64::decode_vec(wrap_nonce).map_err(|_| DocumentError::CorruptCryptoMetadata)?;
    let nonce: [u8; FILE_KEY_WRAP_NONCE_LEN] = nonce_bytes.try_into().map_err(|_| DocumentError::CorruptCryptoMetadata)?;
    Ok(WrappedFileKey { nonce, ciphertext })
}

/// Si `episode_id` viene informado, comprueba que el proceso exista, pertenezca al mismo
/// paciente, y no esté archivado. Deliberadamente **no** rechaza un proceso `cerrado` — a
/// diferencia de `treatment_episodes::check_episode_assignable` (usado por Formulación/
/// Evaluaciones), Documentos SÍ permite agregar contenido a un proceso ya cerrado (Bloque 16 de
/// la aprobación, decisión explícitamente distinta de la de Formulación).
fn check_episode_belongs_to_patient(conn: &Connection, episode_id: &Option<String>, patient_id: &str) -> Result<(), DocumentError> {
    let Some(episode_id) = episode_id else { return Ok(()) };
    let episode = episodes_repo::find_by_id(conn, episode_id)?.ok_or(DocumentError::EpisodeNotFound)?;
    if episode.patient_id != patient_id {
        return Err(DocumentError::EpisodePatientMismatch);
    }
    if episode.deleted_at.is_some() {
        return Err(DocumentError::EpisodeArchived);
    }
    Ok(())
}

/// Si `session_id` viene informado, comprueba que la sesión exista y pertenezca al mismo
/// paciente — nunca se confía solo en el `patientId` enviado desde React (mismo criterio que
/// `services::payments::check_session_belongs_to_patient`). Si además se informó `episode_id`, y
/// la sesión ya pertenece a un proceso distinto, se rechaza como combinación contradictoria
/// (Bloque 8 de la aprobación) — una sesión sin proceso asignado nunca es contradictoria con
/// ningún `episode_id` informado.
fn check_session_belongs_to_patient(conn: &Connection, session_id: &Option<String>, patient_id: &str, episode_id: &Option<String>) -> Result<(), DocumentError> {
    let Some(session_id) = session_id else { return Ok(()) };
    let session = sessions::find_by_id(conn, session_id)?.ok_or(DocumentError::SessionNotFound)?;
    if session.patient_id != patient_id {
        return Err(DocumentError::SessionPatientMismatch);
    }
    if let (Some(episode_id), Some(session_episode_id)) = (episode_id, &session.episode_id) {
        if episode_id != session_episode_id {
            return Err(DocumentError::SessionEpisodeMismatch);
        }
    }
    Ok(())
}

/// Importa un documento nuevo: lee el archivo de origen, lo cifra con una DEK aleatoria propia
/// (envuelta con la DEK del vault), lo escribe en `vault/files/` mediante un temporal + `rename`
/// atómico, y solo entonces registra su metadata en la base de datos.
///
/// Orden diseñado para que un crash en cualquier punto nunca deje una fila de `documents`
/// apuntando a un archivo inexistente (Bloque 11 de la aprobación): el `INSERT` es siempre el
/// último paso. Si el `INSERT` falla después de que el `rename` ya tuvo éxito (disco lleno, por
/// ejemplo), se intenta limpiar el ciphertext recién escrito antes de propagar el error — a
/// diferencia de un huérfano descubierto más tarde por el reconciliador (que nunca se borra
/// automáticamente sin criterio, ver `docs/documents.md`), aquí se tiene certeza total de que
/// nada más pudo haber llegado a referenciar ese archivo todavía.
pub fn create_document(session: &VaultSession, files_root: &Path, input: NewDocumentInput) -> Result<DocumentSummary, DocumentError> {
    let category = validate_category(input.category)?;
    let description = none_if_blank(input.description);

    session
        .with_connection(|conn| -> Result<(), DocumentError> {
            let patient = patients::find_by_id(conn, &input.patient_id)?.ok_or(DocumentError::PatientNotFound)?;
            if patient.deleted_at.is_some() {
                return Err(DocumentError::PatientArchived);
            }
            check_episode_belongs_to_patient(conn, &input.episode_id, &input.patient_id)?;
            check_session_belongs_to_patient(conn, &input.session_id, &input.patient_id, &input.episode_id)?;
            Ok(())
        })
        .map_err(|_| DocumentError::VaultLocked)??;

    let source_path = Path::new(&input.source_path);
    let original_filename = source_path.file_name().and_then(|n| n.to_str()).ok_or(DocumentValidationError::OriginalFilenameMissing)?.to_string();

    // El límite de tamaño se comprueba con `fs::metadata` (sin leer el contenido) ANTES de
    // intentar cargar el archivo completo a memoria — Bloque 10 de la aprobación.
    let metadata = std::fs::metadata(source_path).map_err(|_| DocumentError::SourceFileNotFound)?;
    if metadata.len() == 0 {
        return Err(DocumentError::SourceFileEmpty);
    }
    if metadata.len() > document_crypto::MAX_DOCUMENT_SIZE_BYTES {
        return Err(DocumentError::SourceFileTooLarge);
    }

    let mut plaintext = std::fs::read(source_path).map_err(|_| DocumentError::SourceFileNotFound)?;
    let sha256_plaintext = sha256_hex(&plaintext);
    let mime_type = none_if_blank(input.mime_type).unwrap_or_else(|| guess_mime_from_extension(&original_filename));

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
        documents::insert_document(
            conn,
            &NewDocumentRow {
                id: &document_id.to_string(),
                patient_id: &input.patient_id,
                episode_id: input.episode_id.as_deref(),
                session_id: input.session_id.as_deref(),
                category: category.as_deref(),
                original_filename: &original_filename,
                mime_type: &mime_type,
                size_bytes: metadata.len() as i64,
                sha256_plaintext: &sha256_plaintext,
                storage_path: &storage_path,
                description: description.as_deref(),
                wrapped_file_dek: &wrapped_file_dek,
                wrap_nonce: &wrap_nonce,
                // CRYPTO-1 (Fase 17): todo documento nuevo se envuelve exclusivamente con el
                // esquema domain-separated (session.wrap_file_key ya solo produce ese esquema) —
                // se fija explícitamente aquí, nunca se depende del DEFAULT 1 de la columna, que
                // existe únicamente para las filas legacy creadas antes de esta fase.
                key_wrap_version: KeyWrapVersion::DomainSeparated.as_i64(),
            },
        )
    });

    match insert_result {
        Ok(Ok(document)) => Ok(document.to_summary()),
        Ok(Err(db_err)) => {
            // El rename ya tuvo éxito pero el INSERT falló: se tiene certeza total de que
            // ningún otro código pudo haber llegado a referenciar este archivo todavía (se
            // acaba de crear en esta misma llamada) — limpieza best-effort, nunca oculta el
            // error real.
            let _ = std::fs::remove_file(&abs_path);
            Err(DocumentError::Database(db_err))
        }
        Err(_locked) => {
            let _ = std::fs::remove_file(&abs_path);
            Err(DocumentError::VaultLocked)
        }
    }
}

pub fn get_document(conn: &Connection, id: &str) -> Result<DocumentSummary, DocumentError> {
    let doc = documents::find_document_by_id(conn, id)?.ok_or(DocumentError::NotFound)?;
    Ok(doc.to_summary())
}

pub fn list_documents(conn: &Connection, patient_id: &str) -> Result<Vec<DocumentSummary>, DocumentError> {
    Ok(documents::list_documents_by_patient(conn, patient_id)?)
}

pub fn list_archived_documents(conn: &Connection, patient_id: &str) -> Result<Vec<DocumentSummary>, DocumentError> {
    Ok(documents::list_archived_documents_by_patient(conn, patient_id)?)
}

/// Edita únicamente categoría/descripción. Nunca reemplaza el contenido cifrado ni ninguna
/// asociación. No repite la comprobación de paciente archivado: mismo criterio ya establecido en
/// el resto del proyecto (Pagos/Tareas/Evaluaciones) de que corregir metadata de un registro ya
/// existente sigue permitido para un paciente archivado — solo la creación de contenido nuevo se
/// bloquea (Bloque 15 de la aprobación).
pub fn update_document_metadata(conn: &Connection, id: &str, input: DocumentMetadataInput) -> Result<DocumentSummary, DocumentError> {
    let category = validate_category(input.category)?;
    let description = none_if_blank(input.description);
    let doc = documents::update_document_metadata(conn, id, &DocumentMetadataUpdate { category: category.as_deref(), description: description.as_deref() })?
        .ok_or(DocumentError::NotFound)?;
    Ok(doc.to_summary())
}

/// Soft delete. El ciphertext físico nunca se borra como consecuencia de archivar (Bloque 17 de
/// la aprobación) — permanece en `vault/files/` indefinidamente, disponible para restaurar.
pub fn archive_document(conn: &Connection, id: &str) -> Result<DocumentSummary, DocumentError> {
    if !documents::archive_document(conn, id)? {
        return Err(DocumentError::NotFound);
    }
    get_document(conn, id)
}

pub fn restore_document(conn: &Connection, id: &str) -> Result<DocumentSummary, DocumentError> {
    if !documents::restore_document(conn, id)? {
        return Err(DocumentError::NotFound);
    }
    get_document(conn, id)
}

/// Descifra el contenido completo de un documento (para abrir/exportar) — nunca lo cachea, nunca
/// lo escribe a ningún archivo por sí sola: el llamador (`commands::documents`) decide qué hacer
/// con el resultado (mostrarlo en memoria para una imagen, o escribirlo a un temporal para abrir
/// externamente — Bloque 20 de la aprobación).
pub fn get_document_content(session: &VaultSession, files_root: &Path, id: &str) -> Result<(Vec<u8>, DocumentSummary), DocumentError> {
    let doc: Document = session
        .with_connection(|conn| documents::find_document_by_id(conn, id))
        .map_err(|_| DocumentError::VaultLocked)??
        .ok_or(DocumentError::NotFound)?;
    // Comprobación de versión de formato ANTES de intentar nada más (Bloque 5/25 de la
    // aprobación: "detectar versión desconocida" / "versiones compatibles") — falla rápido con
    // un mensaje claro en vez de intentar un descifrado condenado a no coincidir con lo que el
    // header del propio archivo declara.
    if doc.format_version != document_crypto::FILE_FORMAT_VERSION as i64 {
        return Err(DocumentError::Crypto(DocumentCryptoError::UnknownFormatVersion(doc.format_version as u8)));
    }
    let abs_path = document_crypto::resolve_within_files_root(files_root, &doc.storage_path)?;
    let ciphertext = std::fs::read(&abs_path).map_err(|_| DocumentError::CorruptCryptoMetadata)?;
    let wrapped = columns_to_wrapped_key(&doc.wrapped_file_dek, &doc.wrap_nonce)?;
    // CRYPTO-1 (Fase 17): la columna key_wrap_version de ESTA fila es la única fuente de verdad
    // sobre qué esquema de envoltura usar — nunca autodetección ni un intento con el otro esquema
    // si uno falla. Un valor fuera de {1, 2} no debería ocurrir nunca (CHECK de SCHEMA_V10), pero
    // se trata igual como metadata criptográfica corrupta en vez de asumir un esquema por defecto.
    let key_wrap_version = KeyWrapVersion::try_from(doc.key_wrap_version).map_err(|_| DocumentError::CorruptCryptoMetadata)?;
    let file_key: FileKey = session.unwrap_file_key(&wrapped, key_wrap_version)?;
    let plaintext = document_crypto::decrypt_document(&ciphertext, file_key.expose_secret())?;

    // Verificación adicional de integridad (más allá de la autenticación ya provista por
    // AES-256-GCM): confirma que el contenido descifrado coincide con el hash calculado al
    // importar (`sha256_plaintext`) — detecta, por ejemplo, que la fila de `documents` fue
    // restaurada desde un backup inconsistente con un ciphertext de otro origen. Nunca se
    // expone este hash por IPC ni se usa para nada más que esta verificación.
    if sha256_hex(&plaintext) != doc.sha256_plaintext {
        return Err(DocumentError::CorruptCryptoMetadata);
    }

    Ok((plaintext, doc.to_summary()))
}

/// Reporte de consistencia DB/filesystem (Bloque 12/34 de la aprobación) — puramente
/// diagnóstico, nunca actúa por sí solo. Compara los `storage_path` registrados en `documents`
/// (activos o archivados — un documento archivado sigue teniendo su ciphertext) contra los
/// archivos realmente presentes en `vault/files/`.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentConsistencyReport {
    /// Filas en `documents` cuyo `storage_path` no tiene un archivo físico correspondiente —
    /// posible pérdida de datos real, nunca se borra la fila automáticamente.
    pub missing_ciphertext_count: usize,
    /// Archivos bajo `vault/files/` que no corresponden a ninguna fila de `documents` — nunca se
    /// borran automáticamente, podrían ser recuperables (ver `docs/documents.md`).
    pub orphan_ciphertext_count: usize,
}

/// Recorre `files_root` y lo compara contra `documents.storage_path`. Nunca borra nada — ver
/// `DocumentConsistencyReport`. Pensado para ejecutarse al arrancar (diagnóstico silencioso) y
/// para exponerse como comando manual de diagnóstico.
pub fn check_consistency(session: &VaultSession, files_root: &Path) -> Result<DocumentConsistencyReport, DocumentError> {
    let expected: std::collections::HashSet<String> = session.with_connection(documents::list_all_storage_paths).map_err(|_| DocumentError::VaultLocked)??.into_iter().collect();

    let mut present = std::collections::HashSet::new();
    if files_root.exists() {
        for shard in std::fs::read_dir(files_root)?.flatten() {
            let shard_path = shard.path();
            if !shard_path.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(&shard_path)?.flatten() {
                if let Some(relative) = entry.path().strip_prefix(files_root).ok().and_then(|p| p.to_str()) {
                    present.insert(relative.replace('\\', "/"));
                }
            }
        }
    }

    let missing_ciphertext_count = expected.difference(&present).count();
    let orphan_ciphertext_count = present.difference(&expected).count();
    Ok(DocumentConsistencyReport { missing_ciphertext_count, orphan_ciphertext_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::patients::{self as patients_repo, NewPatientRow};
    use crate::repositories::sessions::{self as sessions_repo, NewSessionRow};
    use crate::repositories::treatment_episodes::{self as episodes_repo, NewTreatmentEpisodeRow};
    use crate::security::VaultSession;
    use crate::services::patients as patients_service;

    fn test_env(name: &str) -> (VaultSession, std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("cc-documents-svc-test-{}-{}", std::process::id(), name));
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

    fn create_test_patient(conn: &Connection, name: &str) -> String {
        let id = Uuid::new_v4().to_string();
        patients_repo::insert(
            conn,
            &NewPatientRow {
                id: &id,
                full_name: name,
                preferred_name: None,
                rut: None,
                birth_date: None,
                phone: None,
                email: None,
                address: None,
                emergency_contact_name: None,
                emergency_contact_phone: None,
                emergency_contact_relationship: None,
                status: "activo",
                referred_by: None,
                intake_date: None,
                region: None,
                commune: None,
            },
        )
        .unwrap();
        id
    }

    fn create_test_episode(conn: &Connection, patient_id: &str) -> String {
        let id = Uuid::new_v4().to_string();
        episodes_repo::insert(conn, &NewTreatmentEpisodeRow { id: &id, patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        id
    }

    fn create_test_session(conn: &Connection, patient_id: &str, episode_id: Option<&str>) -> String {
        let id = Uuid::new_v4().to_string();
        sessions_repo::insert(
            conn,
            &NewSessionRow { id: &id, patient_id, appointment_id: None, episode_id, session_date: "2026-01-05", start_time: None, duration_minutes: None, modality: None, status: "realizada" },
        )
        .unwrap();
        id
    }

    fn minimal_input(patient_id: &str, source_path: String) -> NewDocumentInput {
        NewDocumentInput { patient_id: patient_id.to_string(), episode_id: None, session_id: None, category: Some("informe".to_string()), description: None, source_path, mime_type: None }
    }

    #[test]
    fn creates_a_document_and_persists_it_encrypted_on_disk() {
        let (session, files_root, sources_dir) = test_env("create-basic");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Uno")).unwrap();
        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido ficticio de prueba");

        let summary = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap();
        assert_eq!(summary.original_filename, "informe.pdf");
        assert_eq!(summary.category.as_deref(), Some("informe"));

        // El único archivo físico bajo files_root debe existir y NUNCA contener el plaintext.
        let entries: Vec<_> = walk_files(&files_root);
        assert_eq!(entries.len(), 1);
        let physical_bytes = std::fs::read(&entries[0]).unwrap();
        assert!(!contains_subslice(&physical_bytes, b"contenido ficticio de prueba"), "el archivo físico nunca debe contener el plaintext");
    }

    fn walk_files(root: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if !root.exists() {
            return out;
        }
        for shard in std::fs::read_dir(root).unwrap() {
            let shard = shard.unwrap().path();
            if shard.is_dir() {
                for entry in std::fs::read_dir(&shard).unwrap() {
                    out.push(entry.unwrap().path());
                }
            }
        }
        out
    }

    fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn round_trips_content_through_get_document_content() {
        let (session, files_root, sources_dir) = test_env("roundtrip");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Dos")).unwrap();
        let source = write_source_file(&sources_dir, "imagen.png", b"bytes ficticios de una imagen");

        let summary = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap();
        let (content, fetched) = get_document_content(&session, &files_root, &summary.id).unwrap();
        assert_eq!(content, b"bytes ficticios de una imagen");
        assert_eq!(fetched.id, summary.id);
    }

    /// CRYPTO-1 (Fase 17): un documento creado bajo el algoritmo EXACTO de Fase 16
    /// (`key_wrap_version = 1`, DEK del vault directa) debe seguir abriendo correctamente —
    /// nunca se migra automáticamente, nunca se reenvuelve. Se construye la fila manualmente
    /// (en vez de vía `create_document`, que desde esta fase solo produce `key_wrap_version = 2`)
    /// usando `wrap_file_key_as_legacy_for_tests` — el mismo cuerpo, sin cambios, de lo que era
    /// `wrap_file_key` antes de esta fase.
    #[test]
    fn a_legacy_key_wrap_version_one_document_still_opens_correctly() {
        let (session, files_root, _sources_dir) = test_env("legacy-v1-still-opens");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Legacy")).unwrap();

        let plaintext = b"contenido de un documento creado antes de Fase 17";
        let file_dek = document_crypto::generate_file_dek().unwrap();
        let encrypted = document_crypto::encrypt_document(plaintext, &file_dek).unwrap();
        let wrapped = session.wrap_file_key_as_legacy_for_tests(&file_dek).unwrap();
        let (wrapped_file_dek, wrap_nonce) = wrapped_key_to_columns(&wrapped);

        let document_id = Uuid::new_v4();
        let storage_path = document_crypto::opaque_storage_path(document_id);
        let abs_path = document_crypto::resolve_within_files_root(&files_root, &storage_path).unwrap();
        std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
        std::fs::write(&abs_path, &encrypted).unwrap();

        let sha256_plaintext = sha256_hex(plaintext);
        session
            .with_connection(|conn| {
                documents::insert_document(
                    conn,
                    &documents::NewDocumentRow {
                        id: &document_id.to_string(),
                        patient_id: &patient_id,
                        episode_id: None,
                        session_id: None,
                        category: Some("informe"),
                        original_filename: "legacy.pdf",
                        mime_type: "application/pdf",
                        size_bytes: plaintext.len() as i64,
                        sha256_plaintext: &sha256_plaintext,
                        storage_path: &storage_path,
                        description: None,
                        wrapped_file_dek: &wrapped_file_dek,
                        wrap_nonce: &wrap_nonce,
                        key_wrap_version: 1,
                    },
                )
            })
            .unwrap()
            .unwrap();

        let (content, fetched) = get_document_content(&session, &files_root, &document_id.to_string()).unwrap();
        assert_eq!(content, plaintext);
        assert_eq!(fetched.original_filename, "legacy.pdf");
    }

    /// Contraparte del test anterior: un documento nuevo (creado con `create_document`, esquema
    /// `key_wrap_version = 2` desde esta fase) también abre correctamente — mismo `get_document_
    /// content`, sin ninguna rama especial visible desde el llamador.
    #[test]
    fn a_new_domain_separated_key_wrap_version_two_document_opens_correctly() {
        let (session, files_root, sources_dir) = test_env("v2-opens-correctly");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Nuevo")).unwrap();
        let source = write_source_file(&sources_dir, "nuevo.pdf", b"contenido de un documento creado en Fase 17 o despues");

        let summary = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap();
        let stored = session.with_connection(|conn| documents::find_document_by_id(conn, &summary.id)).unwrap().unwrap().unwrap();
        assert_eq!(stored.key_wrap_version, 2, "todo documento nuevo debe quedar en key_wrap_version = 2");

        let (content, _) = get_document_content(&session, &files_root, &summary.id).unwrap();
        assert_eq!(content, b"contenido de un documento creado en Fase 17 o despues");
    }

    /// CRYPTO-1: cambiar la contraseña del vault re-envuelve la DEK del vault bajo una nueva
    /// clave derivada de la nueva contraseña (`vault.meta.json`) — pero la DEK en sí no cambia
    /// (`dek_is_unchanged_by_a_password_change`, `security::vault_manager`), así que ni
    /// wrap_file_key ni unwrap_file_key dependen de la contraseña en ningún momento. Se comprueba
    /// end-to-end para ambos esquemas: un documento legacy (v1) y uno domain-separated (v2) creados
    /// ANTES del cambio de contraseña siguen abriendo exactamente igual después, incluso tras
    /// bloquear y desbloquear de nuevo con la contraseña nueva.
    #[test]
    fn documents_of_both_key_wrap_versions_remain_readable_after_a_password_change() {
        let (session, files_root, sources_dir) = test_env("password-change-preserves-v1-v2");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Contraseña")).unwrap();

        let source = write_source_file(&sources_dir, "v2.pdf", b"contenido v2 antes del cambio de contrasena");
        let v2_summary = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap();

        let plaintext_v1 = b"contenido v1 antes del cambio de contrasena";
        let file_dek = document_crypto::generate_file_dek().unwrap();
        let encrypted = document_crypto::encrypt_document(plaintext_v1, &file_dek).unwrap();
        let wrapped = session.wrap_file_key_as_legacy_for_tests(&file_dek).unwrap();
        let (wrapped_file_dek, wrap_nonce) = wrapped_key_to_columns(&wrapped);
        let document_id = Uuid::new_v4();
        let storage_path = document_crypto::opaque_storage_path(document_id);
        let abs_path = document_crypto::resolve_within_files_root(&files_root, &storage_path).unwrap();
        std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
        std::fs::write(&abs_path, &encrypted).unwrap();
        let sha256_plaintext = sha256_hex(plaintext_v1);
        session
            .with_connection(|conn| {
                documents::insert_document(
                    conn,
                    &documents::NewDocumentRow {
                        id: &document_id.to_string(),
                        patient_id: &patient_id,
                        episode_id: None,
                        session_id: None,
                        category: Some("informe"),
                        original_filename: "v1.pdf",
                        mime_type: "application/pdf",
                        size_bytes: plaintext_v1.len() as i64,
                        sha256_plaintext: &sha256_plaintext,
                        storage_path: &storage_path,
                        description: None,
                        wrapped_file_dek: &wrapped_file_dek,
                        wrap_nonce: &wrap_nonce,
                        key_wrap_version: 1,
                    },
                )
            })
            .unwrap()
            .unwrap();

        session.change_password("una-contraseña-de-prueba-larga-1", "una-contraseña-completamente-nueva-2").unwrap();
        session.lock();
        session.unlock("una-contraseña-completamente-nueva-2").unwrap();

        let (content_v2, _) = get_document_content(&session, &files_root, &v2_summary.id).unwrap();
        assert_eq!(content_v2, b"contenido v2 antes del cambio de contrasena");
        let (content_v1, _) = get_document_content(&session, &files_root, &document_id.to_string()).unwrap();
        assert_eq!(content_v1, plaintext_v1);
    }

    /// Misma propiedad que el test anterior, pero para el flujo de recuperación por código en vez
    /// de cambio de contraseña — `recover_access` también termina desenvolviendo la MISMA DEK del
    /// vault (nunca genera una nueva), así que documentos v1 y v2 creados antes de recuperar el
    /// acceso siguen abriendo exactamente igual después.
    #[test]
    fn documents_of_both_key_wrap_versions_remain_readable_after_recovery() {
        let base = std::env::temp_dir().join(format!("cc-documents-svc-test-{}-recovery-preserves-v1-v2", std::process::id()));
        if base.exists() {
            std::fs::remove_dir_all(&base).unwrap();
        }
        let vault_dir = base.join("vault");
        std::fs::create_dir_all(&vault_dir).unwrap();
        let files_root = vault_dir.join("files");
        let sources_dir = base.join("sources");
        std::fs::create_dir_all(&sources_dir).unwrap();

        let session = VaultSession::new(&vault_dir);
        let recovery_code = session.begin_creation("una-contraseña-de-prueba-larga-1").unwrap();
        session.confirm_creation().unwrap();

        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Recuperación")).unwrap();
        let source = write_source_file(&sources_dir, "v2.pdf", b"contenido v2 antes de recuperar acceso");
        let v2_summary = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap();

        let plaintext_v1 = b"contenido v1 antes de recuperar acceso";
        let file_dek = document_crypto::generate_file_dek().unwrap();
        let encrypted = document_crypto::encrypt_document(plaintext_v1, &file_dek).unwrap();
        let wrapped = session.wrap_file_key_as_legacy_for_tests(&file_dek).unwrap();
        let (wrapped_file_dek, wrap_nonce) = wrapped_key_to_columns(&wrapped);
        let document_id = Uuid::new_v4();
        let storage_path = document_crypto::opaque_storage_path(document_id);
        let abs_path = document_crypto::resolve_within_files_root(&files_root, &storage_path).unwrap();
        std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
        std::fs::write(&abs_path, &encrypted).unwrap();
        let sha256_plaintext = sha256_hex(plaintext_v1);
        session
            .with_connection(|conn| {
                documents::insert_document(
                    conn,
                    &documents::NewDocumentRow {
                        id: &document_id.to_string(),
                        patient_id: &patient_id,
                        episode_id: None,
                        session_id: None,
                        category: Some("informe"),
                        original_filename: "v1.pdf",
                        mime_type: "application/pdf",
                        size_bytes: plaintext_v1.len() as i64,
                        sha256_plaintext: &sha256_plaintext,
                        storage_path: &storage_path,
                        description: None,
                        wrapped_file_dek: &wrapped_file_dek,
                        wrap_nonce: &wrap_nonce,
                        key_wrap_version: 1,
                    },
                )
            })
            .unwrap()
            .unwrap();

        session.lock();
        session.recover_access(&recovery_code, "una-contraseña-restablecida-3").unwrap();

        let (content_v2, _) = get_document_content(&session, &files_root, &v2_summary.id).unwrap();
        assert_eq!(content_v2, b"contenido v2 antes de recuperar acceso");
        let (content_v1, _) = get_document_content(&session, &files_root, &document_id.to_string()).unwrap();
        assert_eq!(content_v1, plaintext_v1);
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let (session, files_root, sources_dir) = test_env("patient-not-found");
        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let err = create_document(&session, &files_root, minimal_input("no-existe", source)).unwrap_err();
        assert!(matches!(err, DocumentError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let (session, files_root, sources_dir) = test_env("patient-archived");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Tres")).unwrap();
        session.with_connection(|conn| patients_service::archive_patient(conn, &patient_id)).unwrap().unwrap();
        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let err = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap_err();
        assert!(matches!(err, DocumentError::PatientArchived));
    }

    #[test]
    fn rejects_an_episode_belonging_to_a_different_patient() {
        let (session, files_root, sources_dir) = test_env("episode-mismatch");
        let patient_a = session.with_connection(|conn| create_test_patient(conn, "Paciente A")).unwrap();
        let patient_b = session.with_connection(|conn| create_test_patient(conn, "Paciente B")).unwrap();
        let episode_of_b = session.with_connection(|conn| create_test_episode(conn, &patient_b)).unwrap();
        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let mut input = minimal_input(&patient_a, source);
        input.episode_id = Some(episode_of_b);
        let err = create_document(&session, &files_root, input).unwrap_err();
        assert!(matches!(err, DocumentError::EpisodePatientMismatch));
    }

    #[test]
    fn allows_a_document_for_a_closed_episode_unlike_formulation() {
        let (session, files_root, sources_dir) = test_env("closed-episode-allowed");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Cuatro")).unwrap();
        let episode_id = session.with_connection(|conn| create_test_episode(conn, &patient_id)).unwrap();
        session.with_connection(|conn| conn.execute("UPDATE treatment_episodes SET status = 'cerrado' WHERE id = ?1", [&episode_id])).unwrap().unwrap();

        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let mut input = minimal_input(&patient_id, source);
        input.episode_id = Some(episode_id.clone());
        let summary = create_document(&session, &files_root, input).unwrap();
        assert_eq!(summary.episode_id.as_deref(), Some(episode_id.as_str()));
    }

    #[test]
    fn rejects_a_session_belonging_to_a_different_patient() {
        let (session, files_root, sources_dir) = test_env("session-mismatch");
        let patient_a = session.with_connection(|conn| create_test_patient(conn, "Paciente A")).unwrap();
        let patient_b = session.with_connection(|conn| create_test_patient(conn, "Paciente B")).unwrap();
        let session_of_b = session.with_connection(|conn| create_test_session(conn, &patient_b, None)).unwrap();
        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let mut input = minimal_input(&patient_a, source);
        input.session_id = Some(session_of_b);
        let err = create_document(&session, &files_root, input).unwrap_err();
        assert!(matches!(err, DocumentError::SessionPatientMismatch));
    }

    #[test]
    fn rejects_a_session_whose_episode_conflicts_with_the_given_episode() {
        let (session, files_root, sources_dir) = test_env("session-episode-conflict");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Cinco")).unwrap();
        let episode_a = session.with_connection(|conn| create_test_episode(conn, &patient_id)).unwrap();
        // Solo puede haber un proceso "activo" por paciente a la vez — se pausa el primero antes
        // de crear el segundo, para poder tener dos procesos distintos y probar el conflicto.
        session.with_connection(|conn| conn.execute("UPDATE treatment_episodes SET status = 'pausado' WHERE id = ?1", [&episode_a])).unwrap().unwrap();
        let episode_b = session.with_connection(|conn| create_test_episode(conn, &patient_id)).unwrap();
        let session_of_a = session.with_connection(|conn| create_test_session(conn, &patient_id, Some(&episode_a))).unwrap();

        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let mut input = minimal_input(&patient_id, source);
        input.episode_id = Some(episode_b);
        input.session_id = Some(session_of_a);
        let err = create_document(&session, &files_root, input).unwrap_err();
        assert!(matches!(err, DocumentError::SessionEpisodeMismatch));
    }

    #[test]
    fn a_session_without_episode_never_conflicts_with_a_given_episode() {
        let (session, files_root, sources_dir) = test_env("session-no-episode-ok");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Seis")).unwrap();
        let episode_id = session.with_connection(|conn| create_test_episode(conn, &patient_id)).unwrap();
        let session_no_episode = session.with_connection(|conn| create_test_session(conn, &patient_id, None)).unwrap();

        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let mut input = minimal_input(&patient_id, source);
        input.episode_id = Some(episode_id);
        input.session_id = Some(session_no_episode);
        create_document(&session, &files_root, input).unwrap();
    }

    #[test]
    fn rejects_an_invalid_category() {
        let (session, files_root, sources_dir) = test_env("invalid-category");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Siete")).unwrap();
        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let mut input = minimal_input(&patient_id, source);
        input.category = Some("diagnostico-inventado".to_string());
        let err = create_document(&session, &files_root, input).unwrap_err();
        assert!(matches!(err, DocumentError::Validation(DocumentValidationError::InvalidCategory(_))));
    }

    #[test]
    fn accepts_the_new_derivacion_category() {
        let (session, files_root, sources_dir) = test_env("derivacion-category");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Ocho")).unwrap();
        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let mut input = minimal_input(&patient_id, source);
        input.category = Some("derivacion".to_string());
        let summary = create_document(&session, &files_root, input).unwrap();
        assert_eq!(summary.category.as_deref(), Some("derivacion"));
    }

    #[test]
    fn rejects_a_nonexistent_source_file() {
        let (session, files_root, sources_dir) = test_env("source-missing");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Nueve")).unwrap();
        let nonexistent = sources_dir.join("no-existe.pdf").to_str().unwrap().to_string();
        let err = create_document(&session, &files_root, minimal_input(&patient_id, nonexistent)).unwrap_err();
        assert!(matches!(err, DocumentError::SourceFileNotFound));
    }

    #[test]
    fn rejects_an_empty_source_file() {
        let (session, files_root, sources_dir) = test_env("source-empty");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Diez")).unwrap();
        let source = write_source_file(&sources_dir, "vacio.pdf", b"");
        let err = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap_err();
        assert!(matches!(err, DocumentError::SourceFileEmpty));
    }

    #[test]
    fn rejects_a_source_file_over_the_size_limit() {
        let (session, files_root, sources_dir) = test_env("source-too-large");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Once")).unwrap();
        let big = vec![7u8; document_crypto::MAX_DOCUMENT_SIZE_BYTES as usize + 1];
        let source = write_source_file(&sources_dir, "grande.pdf", &big);
        let err = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap_err();
        assert!(matches!(err, DocumentError::SourceFileTooLarge));
    }

    #[test]
    fn update_metadata_never_touches_content() {
        let (session, files_root, sources_dir) = test_env("update-metadata");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Doce")).unwrap();
        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let summary = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap();

        let updated = session
            .with_connection(|conn| update_document_metadata(conn, &summary.id, DocumentMetadataInput { category: Some("otro".to_string()), description: Some("Nota".to_string()) }))
            .unwrap()
            .unwrap();
        assert_eq!(updated.category.as_deref(), Some("otro"));

        let (content, _) = get_document_content(&session, &files_root, &summary.id).unwrap();
        assert_eq!(content, b"contenido");
    }

    #[test]
    fn archive_and_restore_round_trip() {
        let (session, files_root, sources_dir) = test_env("archive-restore");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Trece")).unwrap();
        let source = write_source_file(&sources_dir, "informe.pdf", b"contenido");
        let summary = create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap();

        session.with_connection(|conn| archive_document(conn, &summary.id)).unwrap().unwrap();
        let active = session.with_connection(|conn| list_documents(conn, &patient_id)).unwrap().unwrap();
        assert!(active.is_empty());
        let archived = session.with_connection(|conn| list_archived_documents(conn, &patient_id)).unwrap().unwrap();
        assert_eq!(archived.len(), 1);

        // El ciphertext sigue existiendo físicamente mientras está archivado.
        let (content, _) = get_document_content(&session, &files_root, &summary.id).unwrap();
        assert_eq!(content, b"contenido");

        session.with_connection(|conn| restore_document(conn, &summary.id)).unwrap().unwrap();
        let active_again = session.with_connection(|conn| list_documents(conn, &patient_id)).unwrap().unwrap();
        assert_eq!(active_again.len(), 1);
    }

    #[test]
    fn two_documents_for_the_same_patient_use_independent_encryption() {
        let (session, files_root, sources_dir) = test_env("independent-encryption");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Catorce")).unwrap();
        let source_a = write_source_file(&sources_dir, "a.pdf", b"mismo contenido");
        let source_b = write_source_file(&sources_dir, "b.pdf", b"mismo contenido");

        create_document(&session, &files_root, minimal_input(&patient_id, source_a)).unwrap();
        create_document(&session, &files_root, minimal_input(&patient_id, source_b)).unwrap();

        let files = walk_files(&files_root);
        assert_eq!(files.len(), 2);
        let bytes_a = std::fs::read(&files[0]).unwrap();
        let bytes_b = std::fs::read(&files[1]).unwrap();
        assert_ne!(bytes_a, bytes_b, "dos documentos con el mismo contenido deben producir ciphertexts distintos (DEKs y nonces independientes)");
    }

    #[test]
    fn document_content_is_unrecoverable_directly_from_the_vault_meta_or_db_without_unlocking() {
        // No se puede probar "sin desbloquear" directamente porque los tests usan la sesión ya
        // desbloqueada, pero se confirma la propiedad estructural equivalente: descifrar
        // directamente el archivo físico con la DEK del archivo (sin pasar por
        // unwrap_file_key/la DEK del vault) es exactamente lo que protege wrap_file_key — cubierto
        // ya por los tests de `security::session` y `document_crypto`. Este test documenta la
        // integración: el contenido físico en disco nunca es igual al plaintext.
        let (session, files_root, sources_dir) = test_env("content-protected");
        let patient_id = session.with_connection(|conn| create_test_patient(conn, "Paciente Quince")).unwrap();
        let source = write_source_file(&sources_dir, "informe.pdf", b"XYZFASE16DOCUMENTOSMARKER contenido ficticio");
        create_document(&session, &files_root, minimal_input(&patient_id, source)).unwrap();

        let files = walk_files(&files_root);
        let physical_bytes = std::fs::read(&files[0]).unwrap();
        assert!(!contains_subslice(&physical_bytes, b"XYZFASE16DOCUMENTOSMARKER"), "el marcador de privacidad nunca debe aparecer en el ciphertext físico");
    }
}
