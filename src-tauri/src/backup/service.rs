//! Orquestación de Backup y Restore (Fase 10). Funciones puras sobre rutas
//! de archivo (`&Path`) y sobre `VaultSession` — sin ningún conocimiento de
//! Tauri, para que la lógica central quede separada de cómo se elige el
//! origen/destino en cada plataforma (ver `docs/backup-restore.md`, sección
//! multiplataforma).
//!
//! ## Snapshot consistente: `VACUUM INTO`
//!
//! `create_backup` nunca copia `vault.db` con `fs::copy` mientras hay una
//! conexión viva. Usa `VACUUM INTO`, ejecutado sobre la conexión que ya
//! tiene la aplicación abierta (única forma de obtenerla:
//! `VaultSession::with_connection`, que ya exige que el vault esté
//! desbloqueado). Es una de las formas oficialmente soportadas por SQLite
//! de producir una copia consistente de una base viva — corre dentro de su
//! propia transacción de lectura, nunca ve una escritura a medias, y no
//! depende del modo de journal (funciona igual con el rollback journal por
//! defecto de este proyecto que con WAL) — ver el test
//! `tests::backup_db_entry_opens_with_the_same_key_and_preserves_data` para
//! la verificación real, no solo teórica, de que el archivo resultante
//! sigue siendo un SQLCipher válido con la misma clave.
//!
//! ## Restore: reemplazo, nunca fusión
//!
//! `restore_backup` nunca escribe sobre el vault activo directamente.
//! Extrae y valida todo en un directorio de *staging* desechable; solo si
//! *todas* las validaciones pasan (manifest, hashes, credencial,
//! compatibilidad de esquema, `foreign_key_check`) se bloquea la sesión
//! activa y se promueve el staging mediante dos `rename` — el vault
//! anterior se conserva en `vault-rescue` hasta que el vault restaurado se
//! reabre y valida una segunda vez. Un backup inválido en cualquier punto
//! de la validación nunca llega a tocar un solo byte del vault actual.

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use uuid::Uuid;

use crate::db;
use crate::security::{self, VaultPaths, VaultSession};

use super::archive::{self, ArchiveError};
use super::manifest::{BackupFileEntry, BackupManifest, BACKUP_FORMAT_VERSION, MANIFEST_ENTRY, VAULT_DB_ENTRY, VAULT_META_ENTRY};

fn now_iso8601() -> String {
    // Mismo criterio ya aceptado en el resto del proyecto (Fase 1.4,
    // `security::vault_manager::now_iso8601`): usar el `strftime` nativo de
    // SQLite en memoria en vez de agregar una librería de fechas.
    let conn = Connection::open_in_memory().expect("SQLite en memoria siempre se puede abrir");
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| r.get(0))
        .expect("strftime siempre responde")
}

fn rescue_dir_for(vault_dir: &Path) -> Option<PathBuf> {
    vault_dir.parent().map(|p| p.join("vault-rescue"))
}
fn staging_root_for(vault_dir: &Path) -> Option<PathBuf> {
    vault_dir.parent().map(|p| p.join("vault-restore-tmp"))
}
fn backup_scratch_root_for(vault_dir: &Path) -> Option<PathBuf> {
    vault_dir.parent().map(|p| p.join("backup-scratch"))
}

/// Se llama una única vez al arrancar la aplicación, antes de crear
/// `VaultSession`. No hace nada en el caso normal (ningún restore en
/// curso). Ver la nota de módulo sobre las dos ventanas de inconsistencia
/// posibles de `restore_backup`.
pub fn run_startup_recovery(vault_dir: &Path) {
    if let Some(rescue_dir) = rescue_dir_for(vault_dir) {
        if rescue_dir.exists() {
            if vault_dir.exists() {
                // El restore ya había promovido el staging; el rescue es
                // basura segura de eliminar (nunca se llegó a borrar antes
                // del cierre/crash).
                let _ = std::fs::remove_dir_all(&rescue_dir);
            } else {
                // Crash exactamente entre mover el vault anterior a rescue
                // y promover el staging: recuperar el estado anterior.
                let _ = std::fs::rename(&rescue_dir, vault_dir);
            }
        }
    }
    // Cualquier directorio de staging/scratch que haya quedado de un
    // intento anterior (de restore o de backup) interrumpido a mitad de
    // camino nunca es necesario en un arranque nuevo — se limpia entero.
    if let Some(staging_root) = staging_root_for(vault_dir) {
        let _ = std::fs::remove_dir_all(&staging_root);
    }
    if let Some(scratch_root) = backup_scratch_root_for(vault_dir) {
        let _ = std::fs::remove_dir_all(&scratch_root);
    }
}

// ---------------------------------------------------------------------
// Crear backup
// ---------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub backup_id: String,
    pub created_at: String,
}

#[derive(Debug)]
pub enum BackupError {
    /// El vault no está desbloqueado — crear un backup exige poder obtener
    /// una conexión viva y consistente (ver nota de módulo).
    VaultLocked,
    Database(rusqlite::Error),
    Io(io::Error),
    Archive(ArchiveError),
    /// El archivo de destino elegido por la usuaria ya existe. Nunca se
    /// sobrescribe un backup existente en silencio.
    DestinationAlreadyExists,
}
impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupError::VaultLocked => write!(f, "el vault debe estar desbloqueado para crear un respaldo"),
            BackupError::Database(e) => write!(f, "error al generar la copia consistente: {e}"),
            BackupError::Io(e) => write!(f, "error de E/S: {e}"),
            BackupError::Archive(e) => write!(f, "error al empaquetar el respaldo: {e}"),
            BackupError::DestinationAlreadyExists => write!(f, "ya existe un archivo en la ubicación elegida"),
        }
    }
}
impl std::error::Error for BackupError {}
impl From<io::Error> for BackupError {
    fn from(e: io::Error) -> Self {
        BackupError::Io(e)
    }
}
impl From<ArchiveError> for BackupError {
    fn from(e: ArchiveError) -> Self {
        BackupError::Archive(e)
    }
}

fn read_vault_meta_format_version(meta_path: &Path) -> io::Result<u32> {
    let contents = std::fs::read_to_string(meta_path)?;
    let value: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    value
        .get("format_version")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "vault.meta.json sin format_version"))
}

fn manifest_entry_for(name: &str, path: &Path) -> io::Result<BackupFileEntry> {
    let size_bytes = std::fs::metadata(path)?.len();
    let sha256 = archive::sha256_file(path)?;
    Ok(BackupFileEntry { path: name.to_string(), size_bytes, sha256 })
}

/// Crea un respaldo `.cclinbackup` en `dest_path` a partir del vault activo
/// de `session`, ubicado en `vault_dir`. Exige que el vault esté
/// desbloqueado (ver nota de módulo). `dest_path` no debe existir.
pub fn create_backup(session: &VaultSession, vault_dir: &Path, dest_path: &Path) -> Result<BackupSummary, BackupError> {
    if dest_path.exists() {
        return Err(BackupError::DestinationAlreadyExists);
    }

    let scratch_root = vault_dir.parent().ok_or_else(|| {
        BackupError::Io(io::Error::other("el directorio del vault no tiene padre"))
    })?.join("backup-scratch");
    let scratch = scratch_root.join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&scratch)?;

    let result = (|| -> Result<BackupSummary, BackupError> {
        let snapshot_db = scratch.join(VAULT_DB_ENTRY);
        let snapshot_db_str = snapshot_db
            .to_str()
            .ok_or_else(|| BackupError::Io(io::Error::new(io::ErrorKind::InvalidInput, "ruta de staging no es UTF-8 válida")))?
            .replace('\'', "''");

        // VACUUM INTO exige una conexión viva y desbloqueada — con el vault
        // bloqueado, `with_connection` devuelve `Err` sin llegar a intentar
        // nada, que es exactamente la regla de esta fase (§17).
        let schema_version: i64 = session
            .with_connection(|conn| -> rusqlite::Result<i64> {
                conn.execute_batch(&format!("VACUUM INTO '{snapshot_db_str}';"))?;
                conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            })
            .map_err(|_| BackupError::VaultLocked)?
            .map_err(BackupError::Database)?;

        let meta_src = vault_dir.join(VAULT_META_ENTRY);
        let snapshot_meta = scratch.join(VAULT_META_ENTRY);
        std::fs::copy(&meta_src, &snapshot_meta)?;
        let vault_meta_format_version = read_vault_meta_format_version(&snapshot_meta)?;

        let files = vec![
            manifest_entry_for(VAULT_DB_ENTRY, &snapshot_db)?,
            manifest_entry_for(VAULT_META_ENTRY, &snapshot_meta)?,
        ];

        let manifest = BackupManifest {
            backup_format_version: BACKUP_FORMAT_VERSION,
            backup_id: Uuid::new_v4().to_string(),
            created_at: now_iso8601(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version,
            vault_meta_format_version,
            files,
        };
        let manifest_path = scratch.join(MANIFEST_ENTRY);
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).expect("BackupManifest siempre es serializable"))?;

        archive::write_container(dest_path, &[(MANIFEST_ENTRY, &manifest_path), (VAULT_DB_ENTRY, &snapshot_db), (VAULT_META_ENTRY, &snapshot_meta)])?;

        Ok(BackupSummary { backup_id: manifest.backup_id, created_at: manifest.created_at })
    })();

    let _ = std::fs::remove_dir_all(&scratch);
    result
}

// ---------------------------------------------------------------------
// Inspeccionar backup (solo lectura, sin restaurar)
// ---------------------------------------------------------------------

#[derive(Debug)]
pub enum InspectError {
    ArchiveUnreadable,
    ManifestMissing,
    ManifestInvalid,
}

/// Lee únicamente `manifest.json` de un `.cclinbackup`, sin extraer
/// `vault.db` ni intentar credencial alguna. Pensado para que la UI pueda
/// mostrar "creado el ..." antes de pedir confirmación de restauración.
pub fn inspect_backup(archive_path: &Path) -> Result<BackupManifest, InspectError> {
    let file = File::open(archive_path).map_err(|_| InspectError::ArchiveUnreadable)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|_| InspectError::ArchiveUnreadable)?;
    let mut entry = zip.by_name(MANIFEST_ENTRY).map_err(|_| InspectError::ManifestMissing)?;
    let mut contents = String::new();
    entry.read_to_string(&mut contents).map_err(|_| InspectError::ManifestInvalid)?;
    serde_json::from_str(&contents).map_err(|_| InspectError::ManifestInvalid)
}

// ---------------------------------------------------------------------
// Restaurar backup
// ---------------------------------------------------------------------

#[derive(Debug)]
pub enum RestoreCredential {
    Password(String),
    RecoveryCode { code: String, new_password: String },
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSummary {
    pub restored_at: String,
}

#[derive(Debug)]
pub enum RestoreError {
    ArchiveUnreadable,
    ManifestMissing,
    ManifestInvalid,
    UnsupportedBackupFormatVersion(u32),
    MissingRequiredFile(String),
    FileSizeMismatch(String),
    FileHashMismatch(String),
    IncorrectCredential,
    CorruptStagedDatabase,
    SchemaTooNew { backup_schema_version: i64, supported_schema_version: i64 },
    MigrationFailed,
    IntegrityCheckFailed,
    Io(io::Error),
    SwapFailed(io::Error),
    PostRestoreValidationFailed,
}
impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RestoreError::ArchiveUnreadable => write!(f, "el archivo de respaldo no se pudo leer"),
            RestoreError::ManifestMissing => write!(f, "el respaldo no contiene manifest.json"),
            RestoreError::ManifestInvalid => write!(f, "el manifest del respaldo no es válido"),
            RestoreError::UnsupportedBackupFormatVersion(v) => {
                write!(f, "este respaldo usa un formato de contenedor no soportado (versión {v})")
            }
            RestoreError::MissingRequiredFile(p) => write!(f, "falta un archivo requerido dentro del respaldo: {p}"),
            RestoreError::FileSizeMismatch(p) => write!(f, "un archivo del respaldo tiene un tamaño inesperado: {p}"),
            RestoreError::FileHashMismatch(p) => write!(f, "un archivo del respaldo no coincide con su huella de integridad: {p}"),
            RestoreError::IncorrectCredential => write!(f, "la contraseña o el código de recuperación no corresponden a este respaldo"),
            RestoreError::CorruptStagedDatabase => write!(f, "la base de datos dentro del respaldo está dañada"),
            RestoreError::SchemaTooNew { backup_schema_version, supported_schema_version } => write!(
                f,
                "este respaldo fue creado con una versión más nueva de Cuaderno Clínico (esquema {backup_schema_version}, esta instalación soporta hasta {supported_schema_version}). Actualiza la aplicación antes de restaurarlo."
            ),
            RestoreError::MigrationFailed => write!(f, "no se pudo actualizar el respaldo al esquema actual"),
            RestoreError::IntegrityCheckFailed => write!(f, "el respaldo no pasó la verificación de integridad"),
            RestoreError::Io(e) => write!(f, "error de E/S: {e}"),
            RestoreError::SwapFailed(e) => write!(f, "no se pudo reemplazar el vault actual: {e}"),
            RestoreError::PostRestoreValidationFailed => write!(f, "el vault restaurado no pasó la verificación final"),
        }
    }
}
impl std::error::Error for RestoreError {}

/// Determina hasta qué versión de esquema sabe migrar esta build, sin
/// depender de ninguna conexión externa: migra una base en memoria desde
/// cero y lee el `user_version` resultante. Reutiliza exactamente
/// `db::run_migrations` — no duplica la cadena de migraciones en ningún
/// lado.
fn current_app_schema_version() -> rusqlite::Result<i64> {
    let mut probe = Connection::open_in_memory()?;
    db::run_migrations(&mut probe).map_err(|_| rusqlite::Error::InvalidQuery)?;
    probe.query_row("PRAGMA user_version", [], |r| r.get(0))
}

struct StagingGuard(PathBuf);
impl Drop for StagingGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Restaura `archive_path` sobre el vault de `vault_dir`, gestionado por
/// `session`. Reemplaza — nunca fusiona (ver nota de módulo). Si `vault_dir`
/// no tiene un vault existente todavía (instalación nueva), lo crea a
/// partir del respaldo.
pub fn restore_backup(
    session: &VaultSession,
    vault_dir: &Path,
    archive_path: &Path,
    credential: RestoreCredential,
) -> Result<RestoreSummary, RestoreError> {
    let staging_root = staging_root_for(vault_dir)
        .ok_or_else(|| RestoreError::Io(io::Error::other("el directorio del vault no tiene padre")))?;
    let staging = staging_root.join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&staging).map_err(RestoreError::Io)?;
    let guard = StagingGuard(staging.clone());

    let extracted = archive::extract_container(archive_path, &staging).map_err(|_| RestoreError::ArchiveUnreadable)?;
    if !extracted.iter().any(|p| p == MANIFEST_ENTRY) {
        return Err(RestoreError::ManifestMissing);
    }
    let manifest_contents = std::fs::read_to_string(staging.join(MANIFEST_ENTRY)).map_err(|_| RestoreError::ManifestInvalid)?;
    let manifest: BackupManifest = serde_json::from_str(&manifest_contents).map_err(|_| RestoreError::ManifestInvalid)?;

    if manifest.backup_format_version != BACKUP_FORMAT_VERSION {
        return Err(RestoreError::UnsupportedBackupFormatVersion(manifest.backup_format_version));
    }
    for required in [VAULT_DB_ENTRY, VAULT_META_ENTRY] {
        if manifest.find_entry(required).is_none() {
            return Err(RestoreError::MissingRequiredFile(required.to_string()));
        }
    }
    for entry in &manifest.files {
        let path = staging.join(&entry.path);
        let meta = std::fs::metadata(&path).map_err(|_| RestoreError::MissingRequiredFile(entry.path.clone()))?;
        if meta.len() != entry.size_bytes {
            return Err(RestoreError::FileSizeMismatch(entry.path.clone()));
        }
        let hash = archive::sha256_file(&path).map_err(|_| RestoreError::FileHashMismatch(entry.path.clone()))?;
        if hash != entry.sha256 {
            return Err(RestoreError::FileHashMismatch(entry.path.clone()));
        }
    }

    let staged_paths = VaultPaths::new(&staging);
    let mut staged_conn: Connection = match &credential {
        RestoreCredential::Password(pw) => security::unlock_vault(&staged_paths, pw).map(|(conn, _dek)| conn).map_err(|e| match e {
            security::UnlockError::CorruptDatabase => RestoreError::CorruptStagedDatabase,
            _ => RestoreError::IncorrectCredential,
        })?,
        RestoreCredential::RecoveryCode { code, new_password } => {
            security::recover_access(&staged_paths, code, new_password).map(|(conn, _dek)| conn).map_err(|e| match e {
                security::RecoveryError::CorruptDatabase => RestoreError::CorruptStagedDatabase,
                _ => RestoreError::IncorrectCredential,
            })?
        }
    };

    let backup_schema_version: i64 = staged_conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|_| RestoreError::CorruptStagedDatabase)?;
    let supported_schema_version = current_app_schema_version().map_err(|_| RestoreError::CorruptStagedDatabase)?;
    if backup_schema_version > supported_schema_version {
        return Err(RestoreError::SchemaTooNew { backup_schema_version, supported_schema_version });
    }
    db::run_migrations(&mut staged_conn).map_err(|_| RestoreError::MigrationFailed)?;

    let fk_violations: i64 = staged_conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0))
        .map_err(|_| RestoreError::IntegrityCheckFailed)?;
    if fk_violations != 0 {
        return Err(RestoreError::IntegrityCheckFailed);
    }
    let table_count: i64 = staged_conn
        .query_row("SELECT count(*) FROM sqlite_master WHERE type='table'", [], |r| r.get(0))
        .map_err(|_| RestoreError::IntegrityCheckFailed)?;
    if table_count == 0 {
        return Err(RestoreError::IntegrityCheckFailed);
    }
    drop(staged_conn);

    // A partir de aquí, todo pasó validación: recién ahora se toca el
    // vault activo. `session.lock()` cierra la conexión viva y zeroiza el
    // DEK (reutilizando exactamente el mismo mecanismo de `VaultSession` —
    // ver §36 de la aprobación).
    session.lock();

    let rescue_dir = rescue_dir_for(vault_dir)
        .ok_or_else(|| RestoreError::Io(io::Error::other("sin directorio padre")))?;
    let _ = std::fs::remove_dir_all(&rescue_dir);
    let had_previous_vault = vault_dir.exists();
    if had_previous_vault {
        std::fs::rename(vault_dir, &rescue_dir).map_err(RestoreError::SwapFailed)?;
    }
    // A partir de aquí `guard` ya no debe borrar `staging`: se está
    // moviendo a su ubicación definitiva. `std::mem::forget` es seguro
    // porque el directorio deja de existir en la ruta original tras el
    // `rename` exitoso.
    match std::fs::rename(&staging, vault_dir) {
        Ok(()) => std::mem::forget(guard),
        Err(e) => {
            if had_previous_vault {
                let _ = std::fs::rename(&rescue_dir, vault_dir);
            }
            return Err(RestoreError::SwapFailed(e));
        }
    }

    let post_restore_ok = {
        let promoted_paths = VaultPaths::new(vault_dir);
        let reopen = match &credential {
            RestoreCredential::Password(pw) => security::unlock_vault(&promoted_paths, pw).map(|_| ()),
            RestoreCredential::RecoveryCode { new_password, .. } => security::unlock_vault(&promoted_paths, new_password).map(|_| ()),
        };
        reopen.is_ok()
    };

    if !post_restore_ok {
        let failed_dir = vault_dir
            .parent()
            .map(|p| p.join(format!("vault-restore-failed-{}", Uuid::new_v4())))
            .unwrap_or_else(|| vault_dir.with_extension("failed"));
        let _ = std::fs::rename(vault_dir, &failed_dir);
        if had_previous_vault {
            let _ = std::fs::rename(&rescue_dir, vault_dir);
        }
        session.refresh_from_disk();
        return Err(RestoreError::PostRestoreValidationFailed);
    }

    if had_previous_vault {
        let _ = std::fs::remove_dir_all(&rescue_dir);
    }
    session.refresh_from_disk();

    Ok(RestoreSummary { restored_at: now_iso8601() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::VaultStatus;
    use crate::services::patients::{self, PatientInput};

    fn temp_app_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cc-backup-svc-test-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn minimal_patient(name: &str) -> PatientInput {
        PatientInput {
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
        }
    }

    /// Crea un vault real (vía `VaultSession`, igual que la aplicación) en
    /// `app_dir/vault`, ya desbloqueado. Devuelve la sesión, el `vault_dir`,
    /// la contraseña y el código de recuperación usados.
    fn new_unlocked_vault(app_dir: &Path, password: &str) -> (VaultSession, PathBuf, String) {
        let vault_dir = app_dir.join("vault");
        std::fs::create_dir_all(&vault_dir).unwrap();
        let session = VaultSession::new(&vault_dir);
        let recovery_code = session.begin_creation(password).unwrap();
        session.confirm_creation().unwrap();
        (session, vault_dir, recovery_code)
    }

    fn create_test_patient(session: &VaultSession, name: &str) -> String {
        session.with_connection(|conn| patients::create_patient(conn, minimal_patient(name))).unwrap().unwrap().id
    }

    fn patient_count(session: &VaultSession) -> i64 {
        session
            .with_connection(|conn| conn.query_row::<i64, _, _>("SELECT count(*) FROM patients", [], |r| r.get(0)))
            .unwrap()
            .unwrap()
    }

    // -----------------------------------------------------------------
    // Backup (§44 de la aprobación)
    // -----------------------------------------------------------------

    #[test]
    fn create_backup_of_a_valid_vault_produces_a_readable_manifest() {
        let app_dir = temp_app_dir("create-manifest-ok");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Uno");

        let dest = app_dir.join("respaldo.cclinbackup");
        let summary = create_backup(&session, &vault_dir, &dest).unwrap();
        assert!(!summary.backup_id.is_empty());

        let manifest = inspect_backup(&dest).unwrap();
        assert_eq!(manifest.backup_format_version, BACKUP_FORMAT_VERSION);
        assert_eq!(manifest.schema_version, 5);
        assert_eq!(manifest.backup_id, summary.backup_id);
    }

    #[test]
    fn backup_manifest_hashes_match_the_packaged_files() {
        let app_dir = temp_app_dir("manifest-hashes-match");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Dos");

        let dest = app_dir.join("respaldo.cclinbackup");
        create_backup(&session, &vault_dir, &dest).unwrap();
        let manifest = inspect_backup(&dest).unwrap();

        let extract_dir = app_dir.join("verify-extract");
        std::fs::create_dir_all(&extract_dir).unwrap();
        archive::extract_container(&dest, &extract_dir).unwrap();

        for entry in &manifest.files {
            let real_hash = archive::sha256_file(&extract_dir.join(&entry.path)).unwrap();
            assert_eq!(real_hash, entry.sha256, "hash no coincide para {}", entry.path);
            let real_size = std::fs::metadata(extract_dir.join(&entry.path)).unwrap().len();
            assert_eq!(real_size, entry.size_bytes);
        }
    }

    /// El test más importante de esta fase: confirma empíricamente (no solo
    /// en teoría) que `VACUUM INTO`, ejecutado sobre la conexión SQLCipher
    /// ya desbloqueada, produce un archivo que sigue siendo un SQLCipher
    /// válido con la MISMA clave, y que los datos reales quedaron intactos.
    #[test]
    fn backup_db_entry_opens_with_the_same_key_and_preserves_data() {
        let app_dir = temp_app_dir("vacuum-into-preserves-data");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Tres");
        create_test_patient(&session, "Paciente Cuatro");

        let dest = app_dir.join("respaldo.cclinbackup");
        create_backup(&session, &vault_dir, &dest).unwrap();

        let extract_dir = app_dir.join("verify-extract");
        std::fs::create_dir_all(&extract_dir).unwrap();
        archive::extract_container(&dest, &extract_dir).unwrap();

        // Desbloquear el snapshot extraído con la MISMA contraseña, de forma
        // completamente independiente del vault activo.
        let (conn, _dek) = security::unlock_vault(&VaultPaths::new(&extract_dir), "ContrasenaSegura2026!").unwrap();
        let count: i64 = conn.query_row("SELECT count(*) FROM patients", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);

        // Y el archivo en disco sigue sin el encabezado plano de SQLite —
        // el snapshot no es "menos cifrado" que el vault original.
        let bytes = std::fs::read(extract_dir.join(VAULT_DB_ENTRY)).unwrap();
        assert_ne!(&bytes[..15], b"SQLite format 3");
    }

    #[test]
    fn manifest_never_contains_patient_data() {
        let app_dir = temp_app_dir("manifest-no-patient-data");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "XYZFASE10BACKUP Nombre De Prueba Muy Distintivo");

        let dest = app_dir.join("respaldo.cclinbackup");
        create_backup(&session, &vault_dir, &dest).unwrap();
        let manifest = inspect_backup(&dest).unwrap();

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(!json.contains("XYZFASE10BACKUP"));
        assert!(!json.to_lowercase().contains("paciente"));
    }

    #[test]
    fn backup_contains_only_the_three_expected_entries() {
        let app_dir = temp_app_dir("only-expected-entries");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Cinco");

        let dest = app_dir.join("respaldo.cclinbackup");
        create_backup(&session, &vault_dir, &dest).unwrap();

        let extract_dir = app_dir.join("verify-extract");
        std::fs::create_dir_all(&extract_dir).unwrap();
        let mut entries = archive::extract_container(&dest, &extract_dir).unwrap();
        entries.sort();
        assert_eq!(entries, vec![MANIFEST_ENTRY.to_string(), VAULT_DB_ENTRY.to_string(), VAULT_META_ENTRY.to_string()]);
    }

    #[test]
    fn creating_a_backup_does_not_modify_the_live_vault() {
        let app_dir = temp_app_dir("backup-does-not-modify-live-vault");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Seis");
        let before = patient_count(&session);

        let dest = app_dir.join("respaldo.cclinbackup");
        create_backup(&session, &vault_dir, &dest).unwrap();

        let after = patient_count(&session);
        assert_eq!(before, after);
        assert_eq!(session.status(), VaultStatus::Unlocked, "crear un backup no debe bloquear el vault activo");
    }

    #[test]
    fn two_consecutive_backups_are_both_valid() {
        let app_dir = temp_app_dir("two-consecutive-backups");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Siete");

        let dest_a = app_dir.join("a.cclinbackup");
        let dest_b = app_dir.join("b.cclinbackup");
        let summary_a = create_backup(&session, &vault_dir, &dest_a).unwrap();
        let summary_b = create_backup(&session, &vault_dir, &dest_b).unwrap();

        assert_ne!(summary_a.backup_id, summary_b.backup_id);
        assert!(inspect_backup(&dest_a).is_ok());
        assert!(inspect_backup(&dest_b).is_ok());
    }

    #[test]
    fn a_second_backup_reflects_changes_made_after_the_first() {
        let app_dir = temp_app_dir("second-backup-reflects-changes");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Ocho");

        let dest_a = app_dir.join("a.cclinbackup");
        create_backup(&session, &vault_dir, &dest_a).unwrap();

        create_test_patient(&session, "Paciente Nueve");
        let dest_b = app_dir.join("b.cclinbackup");
        create_backup(&session, &vault_dir, &dest_b).unwrap();

        let extract_a = app_dir.join("extract-a");
        let extract_b = app_dir.join("extract-b");
        std::fs::create_dir_all(&extract_a).unwrap();
        std::fs::create_dir_all(&extract_b).unwrap();
        archive::extract_container(&dest_a, &extract_a).unwrap();
        archive::extract_container(&dest_b, &extract_b).unwrap();

        let (conn_a, _) = security::unlock_vault(&VaultPaths::new(&extract_a), "ContrasenaSegura2026!").unwrap();
        let (conn_b, _) = security::unlock_vault(&VaultPaths::new(&extract_b), "ContrasenaSegura2026!").unwrap();
        let count_a: i64 = conn_a.query_row("SELECT count(*) FROM patients", [], |r| r.get(0)).unwrap();
        let count_b: i64 = conn_b.query_row("SELECT count(*) FROM patients", [], |r| r.get(0)).unwrap();
        assert_eq!(count_a, 1);
        assert_eq!(count_b, 2);
    }

    #[test]
    fn create_backup_is_rejected_while_vault_is_locked() {
        let app_dir = temp_app_dir("backup-rejected-while-locked");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Diez");
        session.lock();

        let dest = app_dir.join("respaldo.cclinbackup");
        let err = create_backup(&session, &vault_dir, &dest).unwrap_err();
        assert!(matches!(err, BackupError::VaultLocked));
        assert!(!dest.exists(), "no debe quedar ningún archivo parcial si el backup se rechaza");
    }

    #[test]
    fn create_backup_rejects_an_existing_destination() {
        let app_dir = temp_app_dir("backup-rejects-existing-destination");
        let (session, vault_dir, _rc) = new_unlocked_vault(&app_dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Once");

        let dest = app_dir.join("ya-existe.cclinbackup");
        std::fs::write(&dest, b"contenido previo").unwrap();

        let err = create_backup(&session, &vault_dir, &dest).unwrap_err();
        assert!(matches!(err, BackupError::DestinationAlreadyExists));
        assert_eq!(std::fs::read(&dest).unwrap(), b"contenido previo", "el destino previo no debe tocarse");
    }

    // -----------------------------------------------------------------
    // Restore (§45 de la aprobación)
    // -----------------------------------------------------------------

    #[test]
    fn restore_onto_a_fresh_installation_with_no_existing_vault() {
        let source_dir = temp_app_dir("restore-fresh-source");
        let (source_session, source_vault_dir, _rc) = new_unlocked_vault(&source_dir, "ContrasenaSegura2026!");
        create_test_patient(&source_session, "Paciente Fresco");
        let dest = source_dir.join("respaldo.cclinbackup");
        create_backup(&source_session, &source_vault_dir, &dest).unwrap();

        // Instalación nueva: ni siquiera existe el directorio del vault.
        let fresh_dir = temp_app_dir("restore-fresh-target");
        let fresh_vault_dir = fresh_dir.join("vault");
        assert!(!fresh_vault_dir.exists());
        let fresh_session = VaultSession::new(&fresh_vault_dir);
        assert_eq!(fresh_session.status(), VaultStatus::NoVault);

        let summary = restore_backup(
            &fresh_session,
            &fresh_vault_dir,
            &dest,
            RestoreCredential::Password("ContrasenaSegura2026!".to_string()),
        )
        .unwrap();
        assert!(!summary.restored_at.is_empty());
        assert_eq!(fresh_session.status(), VaultStatus::Locked);

        fresh_session.unlock("ContrasenaSegura2026!").unwrap();
        assert_eq!(patient_count(&fresh_session), 1);
    }

    #[test]
    fn restore_over_an_existing_vault_replaces_it_exactly() {
        let source_dir = temp_app_dir("restore-replace-source");
        let (source_session, source_vault_dir, _rc) = new_unlocked_vault(&source_dir, "ContrasenaSegura2026!");
        create_test_patient(&source_session, "Del Respaldo A");
        let dest = source_dir.join("a.cclinbackup");
        create_backup(&source_session, &source_vault_dir, &dest).unwrap();

        let target_dir = temp_app_dir("restore-replace-target");
        let (target_session, target_vault_dir, _rc2) = new_unlocked_vault(&target_dir, "OtraContrasenaDelTarget2026!");
        create_test_patient(&target_session, "Solo En El Target, Debe Desaparecer");
        create_test_patient(&target_session, "Tambien Debe Desaparecer");
        assert_eq!(patient_count(&target_session), 2);

        restore_backup(&target_session, &target_vault_dir, &dest, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap();
        assert_eq!(target_session.status(), VaultStatus::Locked);

        target_session.unlock("ContrasenaSegura2026!").unwrap();
        assert_eq!(patient_count(&target_session), 1);
        let only = target_session.with_connection(|conn| patients::list_patients(conn, None)).unwrap().unwrap();
        assert_eq!(only[0].full_name, "Del Respaldo A");

        // La contraseña del target original ya no debe funcionar — el
        // vault fue reemplazado, no fusionado.
        target_session.lock();
        let old_err = target_session.unlock("OtraContrasenaDelTarget2026!").unwrap_err();
        assert!(matches!(old_err, security::UnlockError::IncorrectPassword));
    }

    #[test]
    fn restore_discards_changes_made_after_the_backup() {
        let dir = temp_app_dir("restore-discards-later-changes");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Antes Del Backup");
        let dest = dir.join("respaldo.cclinbackup");
        create_backup(&session, &vault_dir, &dest).unwrap();

        create_test_patient(&session, "Despues Del Backup");
        assert_eq!(patient_count(&session), 2);

        restore_backup(&session, &vault_dir, &dest, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap();
        session.unlock("ContrasenaSegura2026!").unwrap();
        assert_eq!(patient_count(&session), 1);
    }

    fn assert_vault_untouched(session: &VaultSession, vault_dir: &Path, expected_password: &str, expected_count: i64) {
        // El vault debe seguir siendo exactamente el mismo: mismo estado de
        // sesión posible (bloqueado o desbloqueado según se dejó), y al
        // desbloquear con la contraseña original, los mismos datos.
        assert!(vault_dir.join("vault.db").exists());
        assert!(vault_dir.join("vault.meta.json").exists());
        if session.status() != VaultStatus::Unlocked {
            session.unlock(expected_password).unwrap();
        }
        assert_eq!(patient_count(session), expected_count);
    }

    #[test]
    fn a_failed_restore_leaves_the_previous_vault_intact() {
        let dir = temp_app_dir("failed-restore-leaves-vault-intact");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Que Debe Sobrevivir");

        // Un archivo que ni siquiera es un ZIP válido.
        let bogus = dir.join("bogus.cclinbackup");
        std::fs::write(&bogus, b"esto no es un contenedor de respaldo valido").unwrap();

        let err = restore_backup(&session, &vault_dir, &bogus, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap_err();
        assert!(matches!(err, RestoreError::ArchiveUnreadable));
        assert_vault_untouched(&session, &vault_dir, "ContrasenaSegura2026!", 1);
    }

    #[test]
    fn restore_with_wrong_password_does_not_touch_the_current_vault() {
        let dir = temp_app_dir("restore-wrong-password");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Protegido");
        let dest = dir.join("respaldo.cclinbackup");
        create_backup(&session, &vault_dir, &dest).unwrap();

        let err = restore_backup(&session, &vault_dir, &dest, RestoreCredential::Password("ContrasenaCompletamenteIncorrecta!".to_string())).unwrap_err();
        assert!(matches!(err, RestoreError::IncorrectCredential));
        assert_vault_untouched(&session, &vault_dir, "ContrasenaSegura2026!", 1);
    }

    #[test]
    fn restore_with_invalid_manifest_does_not_touch_the_current_vault() {
        let dir = temp_app_dir("restore-invalid-manifest");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Manifest");
        let good = dir.join("bueno.cclinbackup");
        create_backup(&session, &vault_dir, &good).unwrap();

        // Reempaquetar con un manifest corrupto (JSON inválido).
        let extract_dir = dir.join("extract-for-tamper");
        std::fs::create_dir_all(&extract_dir).unwrap();
        archive::extract_container(&good, &extract_dir).unwrap();
        std::fs::write(extract_dir.join(MANIFEST_ENTRY), b"esto no es json valido").unwrap();

        let tampered = dir.join("tampered.cclinbackup");
        archive::write_container(
            &tampered,
            &[
                (MANIFEST_ENTRY, &extract_dir.join(MANIFEST_ENTRY)),
                (VAULT_DB_ENTRY, &extract_dir.join(VAULT_DB_ENTRY)),
                (VAULT_META_ENTRY, &extract_dir.join(VAULT_META_ENTRY)),
            ],
        )
        .unwrap();

        let err = restore_backup(&session, &vault_dir, &tampered, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap_err();
        assert!(matches!(err, RestoreError::ManifestInvalid));
        assert_vault_untouched(&session, &vault_dir, "ContrasenaSegura2026!", 1);
    }

    #[test]
    fn restore_with_tampered_file_hash_does_not_touch_the_current_vault() {
        let dir = temp_app_dir("restore-tampered-hash");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Hash");
        let good = dir.join("bueno.cclinbackup");
        create_backup(&session, &vault_dir, &good).unwrap();

        let extract_dir = dir.join("extract-for-tamper");
        std::fs::create_dir_all(&extract_dir).unwrap();
        archive::extract_container(&good, &extract_dir).unwrap();
        // Modificar vault.db sin recalcular su hash en el manifest.
        let mut db_bytes = std::fs::read(extract_dir.join(VAULT_DB_ENTRY)).unwrap();
        let last = db_bytes.len() - 1;
        db_bytes[last] ^= 0xFF;
        std::fs::write(extract_dir.join(VAULT_DB_ENTRY), &db_bytes).unwrap();

        let tampered = dir.join("tampered.cclinbackup");
        archive::write_container(
            &tampered,
            &[
                (MANIFEST_ENTRY, &extract_dir.join(MANIFEST_ENTRY)),
                (VAULT_DB_ENTRY, &extract_dir.join(VAULT_DB_ENTRY)),
                (VAULT_META_ENTRY, &extract_dir.join(VAULT_META_ENTRY)),
            ],
        )
        .unwrap();

        let err = restore_backup(&session, &vault_dir, &tampered, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap_err();
        assert!(matches!(err, RestoreError::FileHashMismatch(p) if p == VAULT_DB_ENTRY));
        assert_vault_untouched(&session, &vault_dir, "ContrasenaSegura2026!", 1);
    }

    #[test]
    fn restore_with_missing_required_file_does_not_touch_the_current_vault() {
        let dir = temp_app_dir("restore-missing-file");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Faltante");
        let good = dir.join("bueno.cclinbackup");
        create_backup(&session, &vault_dir, &good).unwrap();

        let extract_dir = dir.join("extract-for-tamper");
        std::fs::create_dir_all(&extract_dir).unwrap();
        archive::extract_container(&good, &extract_dir).unwrap();

        // Reempaquetar sin vault.meta.json.
        let incomplete = dir.join("incompleto.cclinbackup");
        archive::write_container(&incomplete, &[(MANIFEST_ENTRY, &extract_dir.join(MANIFEST_ENTRY)), (VAULT_DB_ENTRY, &extract_dir.join(VAULT_DB_ENTRY))]).unwrap();

        let err = restore_backup(&session, &vault_dir, &incomplete, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap_err();
        assert!(matches!(err, RestoreError::MissingRequiredFile(p) if p == VAULT_META_ENTRY));
        assert_vault_untouched(&session, &vault_dir, "ContrasenaSegura2026!", 1);
    }

    /// Prueba de corrupción real pedida en §47/§28: una base de datos cuyo
    /// contenido ya no es un SQLCipher válido, pero cuyo tamaño y hash SÍ
    /// coinciden con el manifest (construido a propósito así, para probar
    /// específicamente la capa de apertura/credencial — no la capa de
    /// hashes, que ya tiene su propio test dedicado arriba).
    #[test]
    fn restore_with_corrupt_database_inside_an_otherwise_valid_container_is_rejected() {
        let dir = temp_app_dir("restore-corrupt-db");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Corrupcion");
        let good = dir.join("bueno.cclinbackup");
        create_backup(&session, &vault_dir, &good).unwrap();

        let extract_dir = dir.join("extract-for-corruption");
        std::fs::create_dir_all(&extract_dir).unwrap();
        archive::extract_container(&good, &extract_dir).unwrap();

        // Reemplazar vault.db por basura del MISMO tamaño, y recalcular su
        // hash correctamente para esa basura — así se aísla el test a la
        // apertura del archivo, no a la verificación de integridad previa.
        let original_len = std::fs::metadata(extract_dir.join(VAULT_DB_ENTRY)).unwrap().len();
        let garbage = vec![0x42u8; original_len as usize];
        std::fs::write(extract_dir.join(VAULT_DB_ENTRY), &garbage).unwrap();

        let mut manifest: BackupManifest = serde_json::from_str(&std::fs::read_to_string(extract_dir.join(MANIFEST_ENTRY)).unwrap()).unwrap();
        for f in &mut manifest.files {
            if f.path == VAULT_DB_ENTRY {
                f.sha256 = archive::sha256_file(&extract_dir.join(VAULT_DB_ENTRY)).unwrap();
                f.size_bytes = original_len;
            }
        }
        std::fs::write(extract_dir.join(MANIFEST_ENTRY), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        let corrupt = dir.join("corrupto.cclinbackup");
        archive::write_container(
            &corrupt,
            &[
                (MANIFEST_ENTRY, &extract_dir.join(MANIFEST_ENTRY)),
                (VAULT_DB_ENTRY, &extract_dir.join(VAULT_DB_ENTRY)),
                (VAULT_META_ENTRY, &extract_dir.join(VAULT_META_ENTRY)),
            ],
        )
        .unwrap();

        let err = restore_backup(&session, &vault_dir, &corrupt, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap_err();
        assert!(matches!(err, RestoreError::CorruptStagedDatabase));
        assert_vault_untouched(&session, &vault_dir, "ContrasenaSegura2026!", 1);
    }

    #[test]
    fn restore_rejects_a_backup_from_a_newer_schema_version() {
        let dir = temp_app_dir("restore-schema-too-new");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Futuro");
        let good = dir.join("bueno.cclinbackup");
        create_backup(&session, &vault_dir, &good).unwrap();

        let extract_dir = dir.join("extract-for-future-schema");
        std::fs::create_dir_all(&extract_dir).unwrap();
        archive::extract_container(&good, &extract_dir).unwrap();

        // Adelantar el user_version REAL del snapshot extraído (no solo el
        // del manifest, que restore ni siquiera consulta para esta
        // decisión) más allá de lo que esta build soporta.
        {
            let (conn, _dek) = security::unlock_vault(&VaultPaths::new(&extract_dir), "ContrasenaSegura2026!").unwrap();
            conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        }
        // Recalcular tamaño/hash tras la modificación, para que la prueba
        // ejercite específicamente el rechazo por versión — no un rechazo
        // (también válido, pero distinto) por hash inconsistente.
        let mut manifest: BackupManifest = serde_json::from_str(&std::fs::read_to_string(extract_dir.join(MANIFEST_ENTRY)).unwrap()).unwrap();
        for f in &mut manifest.files {
            if f.path == VAULT_DB_ENTRY {
                f.sha256 = archive::sha256_file(&extract_dir.join(VAULT_DB_ENTRY)).unwrap();
                f.size_bytes = std::fs::metadata(extract_dir.join(VAULT_DB_ENTRY)).unwrap().len();
            }
        }
        std::fs::write(extract_dir.join(MANIFEST_ENTRY), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        let future = dir.join("futuro.cclinbackup");
        archive::write_container(
            &future,
            &[
                (MANIFEST_ENTRY, &extract_dir.join(MANIFEST_ENTRY)),
                (VAULT_DB_ENTRY, &extract_dir.join(VAULT_DB_ENTRY)),
                (VAULT_META_ENTRY, &extract_dir.join(VAULT_META_ENTRY)),
            ],
        )
        .unwrap();

        let err = restore_backup(&session, &vault_dir, &future, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap_err();
        assert!(matches!(err, RestoreError::SchemaTooNew { backup_schema_version: 99, supported_schema_version: 5 }));
        assert_vault_untouched(&session, &vault_dir, "ContrasenaSegura2026!", 1);
    }

    #[test]
    fn restore_runs_foreign_key_check_and_rejects_a_violation() {
        let dir = temp_app_dir("restore-fk-violation");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente FK");
        let good = dir.join("bueno.cclinbackup");
        create_backup(&session, &vault_dir, &good).unwrap();

        let extract_dir = dir.join("extract-for-fk");
        std::fs::create_dir_all(&extract_dir).unwrap();
        archive::extract_container(&good, &extract_dir).unwrap();

        {
            // Insertar una violación de integridad referencial directamente
            // (sin pasar por el servicio, que la impediría) para probar que
            // `restore_backup` la detecta antes de promover el staging.
            let (conn, _dek) = security::unlock_vault(&VaultPaths::new(&extract_dir), "ContrasenaSegura2026!").unwrap();
            conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
            conn.execute(
                "INSERT INTO sessions (id, patient_id, session_date) VALUES ('huerfana', 'no-existe-este-paciente', '2026-01-01')",
                [],
            )
            .unwrap();
        }
        let mut manifest: BackupManifest = serde_json::from_str(&std::fs::read_to_string(extract_dir.join(MANIFEST_ENTRY)).unwrap()).unwrap();
        for f in &mut manifest.files {
            if f.path == VAULT_DB_ENTRY {
                f.sha256 = archive::sha256_file(&extract_dir.join(VAULT_DB_ENTRY)).unwrap();
                f.size_bytes = std::fs::metadata(extract_dir.join(VAULT_DB_ENTRY)).unwrap().len();
            }
        }
        std::fs::write(extract_dir.join(MANIFEST_ENTRY), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        let broken = dir.join("roto.cclinbackup");
        archive::write_container(
            &broken,
            &[
                (MANIFEST_ENTRY, &extract_dir.join(MANIFEST_ENTRY)),
                (VAULT_DB_ENTRY, &extract_dir.join(VAULT_DB_ENTRY)),
                (VAULT_META_ENTRY, &extract_dir.join(VAULT_META_ENTRY)),
            ],
        )
        .unwrap();

        let err = restore_backup(&session, &vault_dir, &broken, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap_err();
        assert!(matches!(err, RestoreError::IntegrityCheckFailed));
        assert_vault_untouched(&session, &vault_dir, "ContrasenaSegura2026!", 1);
    }

    #[test]
    fn restore_with_recovery_code_sets_a_new_password_and_works() {
        let dir = temp_app_dir("restore-recovery-code");
        let (session, vault_dir, recovery_code) = new_unlocked_vault(&dir, "ContrasenaOriginal2026!");
        create_test_patient(&session, "Paciente Recuperado");
        let dest = dir.join("respaldo.cclinbackup");
        create_backup(&session, &vault_dir, &dest).unwrap();

        let target_dir = temp_app_dir("restore-recovery-code-target");
        let target_vault_dir = target_dir.join("vault");
        let target_session = VaultSession::new(&target_vault_dir);

        restore_backup(
            &target_session,
            &target_vault_dir,
            &dest,
            RestoreCredential::RecoveryCode { code: recovery_code, new_password: "NuevaContrasenaTrasRestore2026!".to_string() },
        )
        .unwrap();

        target_session.unlock("NuevaContrasenaTrasRestore2026!").unwrap();
        assert_eq!(patient_count(&target_session), 1);
    }

    #[test]
    fn restore_leaves_no_staging_or_rescue_directories_after_success() {
        let dir = temp_app_dir("restore-no-leftovers-on-success");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Limpieza");
        let dest = dir.join("respaldo.cclinbackup");
        create_backup(&session, &vault_dir, &dest).unwrap();

        restore_backup(&session, &vault_dir, &dest, RestoreCredential::Password("ContrasenaSegura2026!".to_string())).unwrap();

        assert!(!rescue_dir_for(&vault_dir).unwrap().exists());
        let staging_root = staging_root_for(&vault_dir).unwrap();
        if staging_root.exists() {
            assert_eq!(std::fs::read_dir(&staging_root).unwrap().count(), 0, "no debe quedar ningún staging sin limpiar tras un éxito");
        }
    }

    #[test]
    fn run_startup_recovery_restores_the_previous_vault_if_interrupted_between_rescue_and_promote() {
        let dir = temp_app_dir("startup-recovery-mid-swap");
        let vault_dir = dir.join("vault");
        std::fs::create_dir_all(&vault_dir).unwrap();
        std::fs::write(vault_dir.join("vault.db"), b"contenido original").unwrap();
        std::fs::write(vault_dir.join("vault.meta.json"), b"{}").unwrap();

        // Simula el estado exacto de una interrupción: el vault anterior ya
        // se movió a rescue, pero el staging nunca llegó a promoverse (por
        // eso `vault_dir` no existe en este punto del escenario real — se
        // recrea aquí solo para poder moverlo).
        let rescue_dir = rescue_dir_for(&vault_dir).unwrap();
        std::fs::rename(&vault_dir, &rescue_dir).unwrap();
        assert!(!vault_dir.exists());

        run_startup_recovery(&vault_dir);

        assert!(vault_dir.exists());
        assert!(!rescue_dir.exists());
        assert_eq!(std::fs::read(vault_dir.join("vault.db")).unwrap(), b"contenido original");
    }

    #[test]
    fn run_startup_recovery_cleans_up_an_orphaned_rescue_after_a_completed_restore() {
        let dir = temp_app_dir("startup-recovery-orphaned-rescue");
        let vault_dir = dir.join("vault");
        std::fs::create_dir_all(&vault_dir).unwrap();
        std::fs::write(vault_dir.join("vault.db"), b"vault restaurado, ya promovido").unwrap();
        std::fs::write(vault_dir.join("vault.meta.json"), b"{}").unwrap();

        // El vault YA fue promovido (existe), pero el rescue del anterior
        // quedó sin borrar por una interrupción justo después del swap.
        let rescue_dir = rescue_dir_for(&vault_dir).unwrap();
        std::fs::create_dir_all(&rescue_dir).unwrap();
        std::fs::write(rescue_dir.join("vault.db"), b"vault anterior, ya reemplazado").unwrap();

        run_startup_recovery(&vault_dir);

        assert!(vault_dir.exists());
        assert_eq!(std::fs::read(vault_dir.join("vault.db")).unwrap(), b"vault restaurado, ya promovido");
        assert!(!rescue_dir.exists(), "el rescue huérfano debe limpiarse, nunca sobrescribir el vault ya promovido");
    }

    #[test]
    fn run_startup_recovery_does_nothing_in_the_normal_case() {
        let dir = temp_app_dir("startup-recovery-normal-case");
        let (session, vault_dir, _rc) = new_unlocked_vault(&dir, "ContrasenaSegura2026!");
        create_test_patient(&session, "Paciente Normal");

        run_startup_recovery(&vault_dir);

        assert_eq!(session.status(), VaultStatus::Unlocked);
        assert_eq!(patient_count(&session), 1);
    }
}
