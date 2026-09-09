//! Cifrado de archivos y utilidades de ruta opaca para Documentos (Fase 16).
//!
//! Deliberadamente **fuera de `security/*`** — decisión explícita de la aprobación
//! (`Plan-Fase-16-Documentos-cifrados-pendiente-de-aprobacion.md`, Bloque 3): la única
//! capacidad que este módulo pide a `security::VaultSession` es envolver/desenvolver una
//! DEK de archivo (`wrap_file_key`/`unwrap_file_key`, ver `security/session.rs`) — nunca ve
//! la DEK del vault en sí. Todo lo demás (generar la DEK del archivo, cifrar/descifrar su
//! contenido, construir rutas físicas opacas) es lógica propia de este vertical, reutilizando
//! primitivas ya auditadas (`aes-gcm`, `getrandom`, `uuid`) — nunca criptografía propia.
//!
//! ## Formato de archivo versionado (Bloque 5 de la aprobación)
//!
//! ```text
//! [4 bytes: magic "CCD1"] [1 byte: format_version] [12 bytes: nonce] [ciphertext + tag AES-GCM]
//! ```
//!
//! `format_version` vive **redundantemente** aquí (en el propio header físico) y en la columna
//! `documents.format_version` (`SCHEMA_V9`) — mismo principio de "defensa en profundidad" que
//! `vault.meta.json::FORMAT_VERSION`: si algún día un archivo `.enc` se separa de su fila de
//! metadata (copia manual, recuperación de emergencia), el propio archivo sigue sabiendo cómo
//! debe descifrarse. El header nunca lleva metadata clínica — ni nombre, ni categoría, ni
//! ningún identificador de paciente/proceso/sesión.
//!
//! El campo `magic` permite detectar de inmediato "esto no es un documento cifrado de Cuaderno
//! Clínico" (archivo ajeno, truncado a cero bytes, etc.) sin llegar siquiera a intentar
//! autenticar con AES-GCM. La autenticación de AES-256-GCM (tag de 16 bytes, verificado por la
//! propia librería `aes-gcm` al descifrar) es lo que detecta truncamiento del cuerpo,
//! manipulación del ciphertext, o una clave incorrecta — todos indistinguibles entre sí por
//! diseño, mismo criterio ya usado en `security::envelope`.
//!
//! ## Streaming — explícitamente diferido (Bloque 10 de la aprobación)
//!
//! V1: sin streaming/chunks — límite de tamaño fijo (`MAX_DOCUMENT_SIZE_BYTES`) comprobado
//! *antes* de leer el archivo de origen completo (ver `services::documents::create_document`).
//! El formato de header reserva un solo byte para `format_version` precisamente para que una
//! futura versión con chunks pueda introducirse como `format_version = 2` sin reinterpretar
//! nunca los archivos ya cifrados con `format_version = 1`.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use zeroize::Zeroize;

pub const FILE_MAGIC: [u8; 4] = *b"CCD1";
pub const FILE_FORMAT_VERSION: u8 = 1;
pub const CONTENT_NONCE_LEN: usize = 12;
const HEADER_LEN: usize = FILE_MAGIC.len() + 1 + CONTENT_NONCE_LEN;

/// Límite de tamaño por archivo del V1 de esta fase (Bloque 10, aprobado explícitamente: 50 MB,
/// sin streaming). Se comprueba mediante `std::fs::metadata` **antes** de leer el archivo de
/// origen — nunca cargando primero a memoria para descubrir después que excede el límite.
pub const MAX_DOCUMENT_SIZE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug)]
pub enum DocumentCryptoError {
    Random(getrandom::Error),
    EncryptionFailed,
    /// Autenticación fallida al descifrar: clave incorrecta, o el ciphertext está dañado o
    /// manipulado — indistinguibles por diseño (mismo criterio que
    /// `security::envelope::EnvelopeError::UnwrapFailed`).
    DecryptionFailed,
    /// El archivo es más corto que el header mínimo — truncado o no es un archivo de este
    /// formato en absoluto.
    Truncated,
    /// Los primeros 4 bytes no son `FILE_MAGIC` — no es un documento cifrado de Cuaderno
    /// Clínico (archivo ajeno, o corrupción severa).
    InvalidMagic,
    /// `format_version` presente en el header no es reconocida por esta versión de la
    /// aplicación.
    UnknownFormatVersion(u8),
    /// El contenido excede `MAX_DOCUMENT_SIZE_BYTES`.
    TooLarge,
    /// Un documento vacío no se acepta (Bloque 20 de la aprobación).
    Empty,
}

impl fmt::Display for DocumentCryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentCryptoError::Random(_) => write!(f, "no se pudo generar material aleatorio"),
            DocumentCryptoError::EncryptionFailed => write!(f, "no se pudo cifrar el documento"),
            DocumentCryptoError::DecryptionFailed => write!(f, "no se pudo descifrar el documento"),
            DocumentCryptoError::Truncated => write!(f, "el archivo cifrado está truncado"),
            DocumentCryptoError::InvalidMagic => write!(f, "el archivo no tiene el formato esperado"),
            DocumentCryptoError::UnknownFormatVersion(v) => write!(f, "versión de formato de cifrado desconocida: {v}"),
            DocumentCryptoError::TooLarge => write!(f, "el archivo supera el tamaño máximo permitido"),
            DocumentCryptoError::Empty => write!(f, "el archivo está vacío"),
        }
    }
}
impl std::error::Error for DocumentCryptoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DocumentCryptoError::Random(e) => Some(e),
            _ => None,
        }
    }
}
impl From<getrandom::Error> for DocumentCryptoError {
    fn from(e: getrandom::Error) -> Self {
        DocumentCryptoError::Random(e)
    }
}

/// Genera una DEK de archivo aleatoria de 256 bits — independiente para cada documento (Bloque 2
/// de la aprobación: nunca la misma clave para dos archivos). No requiere el vault desbloqueado:
/// es aleatoriedad pura, sin relación con ningún secreto del vault todavía.
pub fn generate_file_dek() -> Result<[u8; 32], DocumentCryptoError> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf)?;
    Ok(buf)
}

/// Cifra el contenido de un documento con AES-256-GCM usando `file_key`, devolviendo el archivo
/// físico completo (header + ciphertext) tal como se escribe en `files/<sharding>/<uuid>.enc`.
/// Nonce aleatorio nuevo en cada llamada — nunca reutilizado, mismo criterio que
/// `security::envelope::wrap_dek`. Rechaza explícitamente un `plaintext` vacío o por encima del
/// límite — nunca cifra "de todas formas" un tamaño que la capa de servicio ya debería haber
/// rechazado antes (defensa en profundidad, no la única comprobación — ver
/// `services::documents::create_document`).
pub fn encrypt_document(plaintext: &[u8], file_key: &[u8; 32]) -> Result<Vec<u8>, DocumentCryptoError> {
    if plaintext.is_empty() {
        return Err(DocumentCryptoError::Empty);
    }
    if plaintext.len() as u64 > MAX_DOCUMENT_SIZE_BYTES {
        return Err(DocumentCryptoError::TooLarge);
    }

    let mut nonce_bytes = [0u8; CONTENT_NONCE_LEN];
    getrandom::fill(&mut nonce_bytes)?;

    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*file_key));
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher.encrypt(&nonce, plaintext).map_err(|_| DocumentCryptoError::EncryptionFailed)?;

    let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    out.extend_from_slice(&FILE_MAGIC);
    out.push(FILE_FORMAT_VERSION);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Descifra un archivo físico completo (header + ciphertext) devuelto por `encrypt_document`.
/// Detecta, en este orden: archivo truncado (más corto que el header mínimo), magic inválido,
/// `format_version` desconocida, y finalmente fallo de autenticación de AES-GCM (clave
/// incorrecta o ciphertext manipulado/truncado dentro del cuerpo).
pub fn decrypt_document(bytes: &[u8], file_key: &[u8; 32]) -> Result<Vec<u8>, DocumentCryptoError> {
    if bytes.len() < HEADER_LEN {
        return Err(DocumentCryptoError::Truncated);
    }
    let (magic, rest) = bytes.split_at(FILE_MAGIC.len());
    if magic != FILE_MAGIC {
        return Err(DocumentCryptoError::InvalidMagic);
    }
    let (version, rest) = rest.split_at(1);
    let version = version[0];
    if version != FILE_FORMAT_VERSION {
        return Err(DocumentCryptoError::UnknownFormatVersion(version));
    }
    let (nonce_bytes, ciphertext) = rest.split_at(CONTENT_NONCE_LEN);

    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*file_key));
    let nonce = Nonce::from(<[u8; CONTENT_NONCE_LEN]>::try_from(nonce_bytes).expect("split_at ya garantiza el largo exacto"));
    cipher.decrypt(&nonce, ciphertext).map_err(|_| DocumentCryptoError::DecryptionFailed)
}

/// Zeroiza un buffer de texto plano tan pronto como deja de necesitarse (tras cifrar al
/// importar, o antes de que la Opción A de visualización lo escriba a un temporal) — mismo
/// criterio de higiene ya aplicado a la KEK/DEK en `security::*`.
pub fn zeroize_plaintext(buf: &mut [u8]) {
    buf.zeroize();
}

// ---------------------------------------------------------------------
// Rutas físicas opacas (Bloque 6 de la aprobación) — sharding de 2 caracteres hexadecimales,
// nunca deriva nada del nombre real, del paciente, ni de ningún dato clínico.
// ---------------------------------------------------------------------

/// Construye la ruta relativa opaca de un documento nuevo: `files/<2-hex>/<uuid>.enc`. El
/// sharding usa los dos primeros caracteres hexadecimales del propio UUID (siempre minúsculas,
/// como los genera `Uuid::to_string()`) — no revela nada adicional, solo evita miles de archivos
/// en un único directorio plano.
pub fn opaque_storage_path(document_id: Uuid) -> String {
    let id = document_id.to_string();
    let shard = &id[0..2];
    format!("files/{shard}/{id}.enc")
}

#[derive(Debug)]
pub enum StoragePathError {
    /// No tiene la forma `files/<2-hex>/<uuid>.enc` esperada — potencial intento de path
    /// traversal, o un dato corrupto en la columna `storage_path`.
    InvalidFormat,
}
impl fmt::Display for StoragePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "storage_path con un formato inesperado")
    }
}
impl std::error::Error for StoragePathError {}

/// Valida que `storage_path` tenga exactamente la forma `files/<2-hex-minúsculas>/<uuid>.enc`
/// generada por `opaque_storage_path`, y resuelve la ruta absoluta dentro de `files_root`
/// (Bloque 6: nunca escapar de `vault/files` mediante `..`, separadores fuera de lo esperado,
/// rutas absolutas o symlinks inesperados).
///
/// `storage_path` en este proyecto **siempre** se genera internamente (nunca a partir de un
/// input directo del usuario) — esta validación es una segunda capa de defensa, no la única: se
/// aplica tanto al escribir (import) como al leer (abrir/exportar/backup), así que una fila de
/// `documents` corrupta o manipulada externamente (p. ej. edición directa del archivo `vault.db`
/// fuera de la aplicación) nunca puede hacer que la aplicación lea o escriba fuera de
/// `vault/files`.
pub fn resolve_within_files_root(files_root: &Path, storage_path: &str) -> Result<PathBuf, StoragePathError> {
    let rest = storage_path.strip_prefix("files/").ok_or(StoragePathError::InvalidFormat)?;
    let (shard, filename) = rest.split_once('/').ok_or(StoragePathError::InvalidFormat)?;

    if shard.len() != 2 || !shard.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err(StoragePathError::InvalidFormat);
    }
    let stem = filename.strip_suffix(".enc").ok_or(StoragePathError::InvalidFormat)?;
    let parsed = Uuid::parse_str(stem).map_err(|_| StoragePathError::InvalidFormat)?;
    if !stem.starts_with(shard) {
        return Err(StoragePathError::InvalidFormat);
    }
    // `Uuid::parse_str` ya rechaza cualquier cosa que no sea exactamente un UUID bien formado
    // (nunca `..`, nunca separadores, nunca una ruta absoluta) — pero se reconfirma
    // explícitamente por componentes antes de unir la ruta, para no depender únicamente del
    // parseo de UUID como única barrera contra traversal.
    let relative = PathBuf::from(shard).join(format!("{parsed}.enc"));
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            _ => return Err(StoragePathError::InvalidFormat),
        }
    }

    Ok(files_root.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // Cripto: roundtrip, claves/nonces incorrectos, truncamiento, tamaños límite.
    // -----------------------------------------------------------------

    #[test]
    fn roundtrip_recovers_the_exact_plaintext() {
        let key = generate_file_dek().unwrap();
        let plaintext = b"contenido ficticio de un documento de prueba";
        let encrypted = encrypt_document(plaintext, &key).unwrap();
        let decrypted = decrypt_document(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypting_with_the_wrong_key_fails() {
        let key_a = generate_file_dek().unwrap();
        let key_b = generate_file_dek().unwrap();
        let encrypted = encrypt_document(b"contenido", &key_a).unwrap();
        let err = decrypt_document(&encrypted, &key_b).unwrap_err();
        assert!(matches!(err, DocumentCryptoError::DecryptionFailed));
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let key = generate_file_dek().unwrap();
        let mut encrypted = encrypt_document(b"contenido original", &key).unwrap();
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xFF;
        let err = decrypt_document(&encrypted, &key).unwrap_err();
        assert!(matches!(err, DocumentCryptoError::DecryptionFailed));
    }

    #[test]
    fn tampered_nonce_is_rejected() {
        let key = generate_file_dek().unwrap();
        let mut encrypted = encrypt_document(b"contenido", &key).unwrap();
        encrypted[5] ^= 0xFF; // dentro de los 12 bytes del nonce (offset 5..17)
        let err = decrypt_document(&encrypted, &key).unwrap_err();
        assert!(matches!(err, DocumentCryptoError::DecryptionFailed));
    }

    #[test]
    fn truncated_file_is_rejected() {
        let key = generate_file_dek().unwrap();
        let encrypted = encrypt_document(b"contenido", &key).unwrap();
        let truncated = &encrypted[..HEADER_LEN - 1];
        let err = decrypt_document(truncated, &key).unwrap_err();
        assert!(matches!(err, DocumentCryptoError::Truncated));
    }

    #[test]
    fn truncated_ciphertext_body_fails_authentication_not_silently() {
        let key = generate_file_dek().unwrap();
        let encrypted = encrypt_document(b"contenido de prueba mas largo", &key).unwrap();
        let truncated = &encrypted[..encrypted.len() - 3];
        let err = decrypt_document(truncated, &key).unwrap_err();
        assert!(matches!(err, DocumentCryptoError::DecryptionFailed));
    }

    #[test]
    fn empty_plaintext_is_rejected() {
        let key = generate_file_dek().unwrap();
        let err = encrypt_document(b"", &key).unwrap_err();
        assert!(matches!(err, DocumentCryptoError::Empty));
    }

    #[test]
    fn plaintext_at_the_exact_size_limit_is_accepted() {
        let key = generate_file_dek().unwrap();
        let plaintext = vec![7u8; MAX_DOCUMENT_SIZE_BYTES as usize];
        let encrypted = encrypt_document(&plaintext, &key).unwrap();
        let decrypted = decrypt_document(&encrypted, &key).unwrap();
        assert_eq!(decrypted.len(), plaintext.len());
    }

    #[test]
    fn plaintext_over_the_size_limit_is_rejected() {
        let key = generate_file_dek().unwrap();
        let plaintext = vec![7u8; MAX_DOCUMENT_SIZE_BYTES as usize + 1];
        let err = encrypt_document(&plaintext, &key).unwrap_err();
        assert!(matches!(err, DocumentCryptoError::TooLarge));
    }

    #[test]
    fn two_identical_plaintexts_produce_different_ciphertext() {
        let key = generate_file_dek().unwrap();
        let a = encrypt_document(b"mismo contenido", &key).unwrap();
        let b = encrypt_document(b"mismo contenido", &key).unwrap();
        assert_ne!(a, b, "el nonce aleatorio debe variar entre llamadas");
    }

    #[test]
    fn two_files_use_independent_deks() {
        let a = generate_file_dek().unwrap();
        let b = generate_file_dek().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn unknown_format_version_is_rejected_before_attempting_decryption() {
        let key = generate_file_dek().unwrap();
        let mut encrypted = encrypt_document(b"contenido", &key).unwrap();
        encrypted[4] = 99; // byte de format_version
        let err = decrypt_document(&encrypted, &key).unwrap_err();
        assert!(matches!(err, DocumentCryptoError::UnknownFormatVersion(99)));
    }

    #[test]
    fn invalid_magic_is_rejected() {
        let key = generate_file_dek().unwrap();
        let mut encrypted = encrypt_document(b"contenido", &key).unwrap();
        encrypted[0] = b'X';
        let err = decrypt_document(&encrypted, &key).unwrap_err();
        assert!(matches!(err, DocumentCryptoError::InvalidMagic));
    }

    // -----------------------------------------------------------------
    // Rutas físicas opacas: sharding, traversal, formatos inválidos.
    // -----------------------------------------------------------------

    #[test]
    fn opaque_storage_path_has_the_expected_shape() {
        let id = Uuid::new_v4();
        let path = opaque_storage_path(id);
        assert_eq!(path, format!("files/{}/{}.enc", &id.to_string()[0..2], id));
    }

    #[test]
    fn resolve_within_files_root_accepts_a_well_formed_path() {
        let id = Uuid::new_v4();
        let storage_path = opaque_storage_path(id);
        let root = Path::new("/vault/files-root-does-not-need-to-exist");
        let resolved = resolve_within_files_root(root, &storage_path).unwrap();
        assert!(resolved.starts_with(root));
        assert!(resolved.to_string_lossy().ends_with(&format!("{id}.enc")));
    }

    #[test]
    fn resolve_within_files_root_rejects_dot_dot_traversal() {
        let root = Path::new("/vault/files-root");
        let err = resolve_within_files_root(root, "files/../../../etc/passwd").unwrap_err();
        assert!(matches!(err, StoragePathError::InvalidFormat));
    }

    #[test]
    fn resolve_within_files_root_rejects_an_absolute_path_disguised_as_a_shard() {
        let root = Path::new("/vault/files-root");
        let err = resolve_within_files_root(root, "files//etc/passwd.enc").unwrap_err();
        assert!(matches!(err, StoragePathError::InvalidFormat));
    }

    #[test]
    fn resolve_within_files_root_rejects_a_shard_that_is_not_two_hex_chars() {
        let root = Path::new("/vault/files-root");
        let id = Uuid::new_v4();
        let bad = format!("files/xy/{id}.enc");
        let err = resolve_within_files_root(root, &bad).unwrap_err();
        assert!(matches!(err, StoragePathError::InvalidFormat));
    }

    #[test]
    fn resolve_within_files_root_rejects_a_filename_that_is_not_a_valid_uuid() {
        let root = Path::new("/vault/files-root");
        let err = resolve_within_files_root(root, "files/ab/not-a-uuid.enc").unwrap_err();
        assert!(matches!(err, StoragePathError::InvalidFormat));
    }

    #[test]
    fn resolve_within_files_root_rejects_missing_enc_extension() {
        let root = Path::new("/vault/files-root");
        let id = Uuid::new_v4();
        let bad = format!("files/{}/{}", &id.to_string()[0..2], id);
        let err = resolve_within_files_root(root, &bad).unwrap_err();
        assert!(matches!(err, StoragePathError::InvalidFormat));
    }

    #[test]
    fn resolve_within_files_root_rejects_an_uppercase_shard() {
        let root = Path::new("/vault/files-root");
        let id = Uuid::new_v4();
        let shard_upper = id.to_string()[0..2].to_uppercase();
        let bad = format!("files/{shard_upper}/{id}.enc");
        let err = resolve_within_files_root(root, &bad).unwrap_err();
        assert!(matches!(err, StoragePathError::InvalidFormat));
    }

    #[test]
    fn resolve_within_files_root_rejects_shard_mismatch_with_the_uuid_itself() {
        let root = Path::new("/vault/files-root");
        let id = Uuid::new_v4();
        // Un shard sintácticamente válido (2 hex) pero que no corresponde al UUID del nombre —
        // no debería poder pasar como si fuera una ruta legítima generada por esta app.
        let mismatched_shard = if &id.to_string()[0..2] == "00" { "11" } else { "00" };
        let bad = format!("files/{mismatched_shard}/{id}.enc");
        let err = resolve_within_files_root(root, &bad).unwrap_err();
        assert!(matches!(err, StoragePathError::InvalidFormat));
    }
}
