//! Orquestación de crear / desbloquear / recuperar / cambiar contraseña.
//! Funciones puras sobre rutas de archivo y secretos en memoria — sin
//! estado global. El estado de sesión ("la app está desbloqueada ahora
//! mismo") vive en `session.rs`.

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::db::{self, VaultError, VaultKey};

use super::envelope::{generate_dek, unwrap_dek, wrap_dek, EnvelopeError};
use super::kdf::{derive_kek, KdfError, KdfParams};
use super::password_policy::{self, PasswordPolicyError};
use super::recovery_code::{RecoveryCode, RecoveryCodeError};
use super::vault_meta::{VaultMetaError, VaultMetaFile, WrapRecord, FORMAT_VERSION};

/// Dónde vive un vault: el archivo cifrado y su archivo de metadatos, en el
/// mismo directorio.
#[derive(Debug, Clone)]
pub struct VaultPaths {
    pub db_path: PathBuf,
    pub meta_path: PathBuf,
}

impl VaultPaths {
    pub fn new(vault_dir: &Path) -> Self {
        Self {
            db_path: vault_dir.join("vault.db"),
            meta_path: vault_dir.join("vault.meta.json"),
        }
    }

    /// Existe un vault utilizable (ambos archivos presentes) en esta ubicación.
    pub fn exists(&self) -> bool {
        self.db_path.exists() && self.meta_path.exists()
    }
}

fn now_iso8601() -> String {
    // Se reutiliza `strftime` de SQLite (en memoria, sin relación con el
    // vault) para el formato de fecha, en vez de implementar cálculo de
    // calendario a mano: es el mismo formato que ya usan todas las columnas
    // `created_at`/`updated_at` del esquema (Fase 1.3).
    let conn = Connection::open_in_memory().expect("SQLite en memoria siempre se puede abrir");
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| r.get(0))
        .expect("strftime siempre responde")
}

// ---------------------------------------------------------------------
// Errores
// ---------------------------------------------------------------------

#[derive(Debug)]
pub enum CreateVaultError {
    WeakPassword(PasswordPolicyError),
    Crypto,
}
impl fmt::Display for CreateVaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CreateVaultError::WeakPassword(e) => write!(f, "{e}"),
            CreateVaultError::Crypto => write!(f, "no se pudo generar el material criptográfico del vault"),
        }
    }
}
impl std::error::Error for CreateVaultError {}
impl From<PasswordPolicyError> for CreateVaultError {
    fn from(e: PasswordPolicyError) -> Self {
        CreateVaultError::WeakPassword(e)
    }
}
impl From<KdfError> for CreateVaultError {
    fn from(_: KdfError) -> Self {
        CreateVaultError::Crypto
    }
}
impl From<EnvelopeError> for CreateVaultError {
    fn from(_: EnvelopeError) -> Self {
        CreateVaultError::Crypto
    }
}
impl From<RecoveryCodeError> for CreateVaultError {
    fn from(_: RecoveryCodeError) -> Self {
        CreateVaultError::Crypto
    }
}

#[derive(Debug)]
pub enum FinalizeCreationError {
    Meta(VaultMetaError),
    Database(VaultError),
    MigrationFailed,
}
impl fmt::Display for FinalizeCreationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FinalizeCreationError::Meta(e) => write!(f, "no se pudo escribir vault.meta.json: {e}"),
            FinalizeCreationError::Database(e) => write!(f, "no se pudo crear la base cifrada: {e}"),
            FinalizeCreationError::MigrationFailed => write!(f, "no se pudo crear el esquema inicial"),
        }
    }
}
impl std::error::Error for FinalizeCreationError {}

/// Mensajes deliberadamente genéricos donde una diferenciación ayudaría a un
/// atacante; ver `docs/security.md` para la justificación de cada variante.
#[derive(Debug)]
pub enum UnlockError {
    /// No hay vault.db y/o vault.meta.json en esta ubicación.
    NoVault,
    /// vault.meta.json existe pero no se puede interpretar (JSON inválido,
    /// esquema desconocido, campos corruptos). Distinto de "contraseña
    /// incorrecta": aquí ni siquiera se llegó a intentar una clave.
    MetaFileUnreadable,
    /// Contraseña incorrecta, o el registro cifrado de la contraseña en
    /// vault.meta.json está dañado/manipulado — indistinguibles por diseño
    /// (autenticación de AES-GCM), igual que en la Fase 1.2 con SQLCipher.
    IncorrectPassword,
    /// La contraseña demostrablemente correcta desenvolvió el DEK, pero el
    /// archivo `.db` no abre. Solo se revela después de probar la
    /// contraseña correcta, así que no ayuda a un atacante sin ella.
    CorruptDatabase,
    /// La contraseña fue correcta y la base abrió, pero llevarla al esquema
    /// más reciente falló (ver `causa raíz WIN-DB-01/02` en
    /// `docs/safety-plan.md`/`docs/library.md`). Nunca se sigue adelante con
    /// un `Connection` a medio migrar.
    MigrationFailed,
}
impl fmt::Display for UnlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnlockError::NoVault => write!(f, "no existe un cuaderno clínico en esta ubicación"),
            UnlockError::MetaFileUnreadable => {
                write!(f, "el archivo de configuración de seguridad del vault está dañado")
            }
            UnlockError::IncorrectPassword => write!(f, "contraseña incorrecta"),
            UnlockError::CorruptDatabase => write!(f, "la base de datos parece estar dañada"),
            UnlockError::MigrationFailed => write!(f, "no se pudo actualizar el esquema de la base de datos"),
        }
    }
}
impl std::error::Error for UnlockError {}

#[derive(Debug)]
pub enum ChangePasswordError {
    WeakPassword(PasswordPolicyError),
    /// La contraseña actual no coincide, o el registro está dañado —
    /// indistinguibles por el mismo motivo que en `UnlockError`.
    IncorrectCurrentPassword,
    MetaFileUnreadable,
    Io(VaultMetaError),
}
impl fmt::Display for ChangePasswordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChangePasswordError::WeakPassword(e) => write!(f, "{e}"),
            ChangePasswordError::IncorrectCurrentPassword => write!(f, "la contraseña actual es incorrecta"),
            ChangePasswordError::MetaFileUnreadable => {
                write!(f, "el archivo de configuración de seguridad del vault está dañado")
            }
            ChangePasswordError::Io(e) => write!(f, "no se pudo guardar el cambio de contraseña: {e}"),
        }
    }
}
impl std::error::Error for ChangePasswordError {}

#[derive(Debug)]
pub enum RecoveryError {
    WeakPassword(PasswordPolicyError),
    InvalidCodeFormat,
    /// Código incorrecto, o el registro de recuperación está dañado —
    /// mismo principio de indistinguibilidad.
    IncorrectRecoveryCode,
    MetaFileUnreadable,
    Io(VaultMetaError),
    CorruptDatabase,
    /// Mismo caso que `UnlockError::MigrationFailed`, alcanzado desde el
    /// flujo de recuperación en vez del de desbloqueo normal.
    MigrationFailed,
}
impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecoveryError::WeakPassword(e) => write!(f, "{e}"),
            RecoveryError::InvalidCodeFormat => write!(f, "el código de recuperación no tiene el formato esperado"),
            RecoveryError::IncorrectRecoveryCode => write!(f, "código de recuperación incorrecto"),
            RecoveryError::MetaFileUnreadable => {
                write!(f, "el archivo de configuración de seguridad del vault está dañado")
            }
            RecoveryError::Io(e) => write!(f, "no se pudo guardar la nueva contraseña: {e}"),
            RecoveryError::CorruptDatabase => write!(f, "la base de datos parece estar dañada"),
            RecoveryError::MigrationFailed => write!(f, "no se pudo actualizar el esquema de la base de datos"),
        }
    }
}
impl std::error::Error for RecoveryError {}

// ---------------------------------------------------------------------
// Creación (en dos pasos: begin/finalize — ver docs/security.md)
// ---------------------------------------------------------------------

/// Todo lo que hace falta para crear un vault, generado en memoria y sin
/// escribir nada en disco todavía. Existe para que la usuaria pueda ver y
/// confirmar el código de recuperación *antes* de que se cree ningún
/// archivo: si cierra la app en ese punto, no queda ningún vault a medio
/// crear en el disco.
#[derive(Debug)]
pub struct PendingVaultCreation {
    dek: VaultKey,
    recovery_code: RecoveryCode,
    meta: VaultMetaFile,
}

impl PendingVaultCreation {
    pub fn begin(password: &str) -> Result<Self, CreateVaultError> {
        password_policy::validate(password)?;

        let dek = generate_dek()?;
        let recovery_code = RecoveryCode::generate()?;
        let params = KdfParams::recommended();

        let password_salt = super::kdf::Salt::generate()?;
        let password_kek = derive_kek(password.as_bytes(), &password_salt, &params)?;
        let password_wrapped = wrap_dek(&dek, &password_kek)?;

        let recovery_salt = super::kdf::Salt::generate()?;
        let recovery_kek = derive_kek(recovery_code.as_bytes(), &recovery_salt, &params)?;
        let recovery_wrapped = wrap_dek(&dek, &recovery_kek)?;

        let meta = VaultMetaFile {
            format_version: FORMAT_VERSION,
            created_at: now_iso8601(),
            password_wrap: WrapRecord::new(&password_salt, params.clone(), &password_wrapped),
            recovery_wrap: WrapRecord::new(&recovery_salt, params, &recovery_wrapped),
        };

        Ok(Self { dek, recovery_code, meta })
    }

    /// El código de recuperación en su formato de despliegue
    /// (`XXXX-XXXX-XXXX-XXXX-XXXX-XXXX`). Se puede llamar más de una vez
    /// mientras la creación esté pendiente, pero una vez que `finalize()`
    /// consume este valor, no vuelve a estar disponible en ningún lado —
    /// no se guarda el texto plano del código en ninguna parte.
    pub fn recovery_code_display(&self) -> String {
        self.recovery_code.to_display_string()
    }

    /// Escribe el vault en disco: guarda `vault.meta.json`, crea la base
    /// SQLCipher, corre las migraciones, y vuelve a abrirla desde cero para
    /// comprobar de punta a punta que un arranque futuro funcionaría igual
    /// (no se reutiliza la conexión recién usada para crear el esquema).
    pub fn finalize(self, paths: &VaultPaths) -> Result<(Connection, VaultKey), FinalizeCreationError> {
        self.meta.save(&paths.meta_path).map_err(FinalizeCreationError::Meta)?;

        {
            let mut conn = db::open_vault(&paths.db_path, &self.dek).map_err(FinalizeCreationError::Database)?;
            db::run_migrations(&mut conn).map_err(|_| FinalizeCreationError::MigrationFailed)?;
        }

        let conn = db::open_vault(&paths.db_path, &self.dek).map_err(FinalizeCreationError::Database)?;
        // `recovery_code` y `meta` (que ya no contiene nada más útil) se
        // sueltan aquí; el DEK sigue haciendo falta mientras la app esté
        // desbloqueada, así que se devuelve en vez de dejarlo zeroizarse.
        Ok((conn, self.dek))
    }
}

// ---------------------------------------------------------------------
// Desbloqueo
// ---------------------------------------------------------------------

/// Autentica y abre la conexión, **sin migrar el esquema**. Uso interno:
/// `unlock_vault` (más abajo) es el punto de entrada real de la aplicación y
/// siempre migra a continuación; `backup::service::restore_backup` es la
/// única otra llamadora, y usa deliberadamente esta variante sin migrar
/// sobre la base **staged** de un backup — necesita inspeccionar el
/// `PRAGMA user_version` crudo (sin tocar el esquema todavía) para decidir
/// si el backup viene de una versión de la app más nueva que esta
/// instalación (`RestoreError::SchemaTooNew`) **antes** de arriesgarse a
/// migrar algo que no sabe interpretar — luego migra explícitamente ella
/// misma, solo si la versión resultó soportada.
pub(crate) fn unlock_vault_without_migrating(paths: &VaultPaths, password: &str) -> Result<(Connection, VaultKey), UnlockError> {
    if !paths.exists() {
        return Err(UnlockError::NoVault);
    }
    let meta = VaultMetaFile::load(&paths.meta_path).map_err(|_| UnlockError::MetaFileUnreadable)?;

    let salt = meta.password_wrap.salt().map_err(|_| UnlockError::MetaFileUnreadable)?;
    let wrapped = meta.password_wrap.wrapped_key().map_err(|_| UnlockError::MetaFileUnreadable)?;
    let kek = derive_kek(password.as_bytes(), &salt, &meta.password_wrap.kdf)
        .map_err(|_| UnlockError::MetaFileUnreadable)?;

    let dek = unwrap_dek(&wrapped, &kek).map_err(|_| UnlockError::IncorrectPassword)?;

    // Si llegamos aquí, la contraseña quedó probada correcta (la
    // autenticación de AES-GCM no puede falsificarse). Cualquier fallo de
    // aquí en adelante es del archivo `.db`, no de la contraseña.
    let conn = db::open_vault(&paths.db_path, &dek).map_err(|_| UnlockError::CorruptDatabase)?;
    Ok((conn, dek))
}

/// Punto de entrada real de la aplicación para reabrir un vault existente
/// (usado por `commands::vault::unlock_vault`, único camino real de
/// desbloqueo). CAUSA RAÍZ WIN-DB-01/WIN-DB-02 (ver
/// `docs/safety-plan.md`/`docs/library.md`): `PendingVaultCreation::finalize`
/// corre las migraciones al crear un vault nuevo, pero un vault YA
/// EXISTENTE solo pasaba antes por `open_vault` en este punto — nunca por
/// `run_migrations`. Un vault creado bajo un esquema anterior (p. ej. V10)
/// quedaba congelado en ese `PRAGMA user_version` para siempre en cada
/// desbloqueo normal, aunque el binario ya conociera esquemas más nuevos
/// (V11/V12…): las tablas y columnas nuevas nunca llegaban a crearse.
/// `to_latest` es un no-op barato cuando el esquema ya está al día (la
/// mayoría de los desbloqueos reales), y aditivo/preservador de datos
/// cuando no — mismo contrato que ya usa `finalize` para un vault nuevo.
pub fn unlock_vault(paths: &VaultPaths, password: &str) -> Result<(Connection, VaultKey), UnlockError> {
    let (mut conn, dek) = unlock_vault_without_migrating(paths, password)?;
    db::run_migrations(&mut conn).map_err(|_| UnlockError::MigrationFailed)?;
    Ok((conn, dek))
}

// ---------------------------------------------------------------------
// Cambio de contraseña — nunca vuelve a cifrar la base
// ---------------------------------------------------------------------

pub fn change_password(
    paths: &VaultPaths,
    current_password: &str,
    new_password: &str,
) -> Result<(), ChangePasswordError> {
    password_policy::validate(new_password).map_err(ChangePasswordError::WeakPassword)?;

    let mut meta = VaultMetaFile::load(&paths.meta_path).map_err(|_| ChangePasswordError::MetaFileUnreadable)?;

    let salt = meta.password_wrap.salt().map_err(|_| ChangePasswordError::MetaFileUnreadable)?;
    let wrapped = meta.password_wrap.wrapped_key().map_err(|_| ChangePasswordError::MetaFileUnreadable)?;
    let kek = derive_kek(current_password.as_bytes(), &salt, &meta.password_wrap.kdf)
        .map_err(|_| ChangePasswordError::MetaFileUnreadable)?;

    // DEK -> desenvolver (prueba que la contraseña actual es correcta).
    let dek = unwrap_dek(&wrapped, &kek).map_err(|_| ChangePasswordError::IncorrectCurrentPassword)?;

    // -> derivar nueva KEK -> volver a envolver el MISMO DEK. La base
    // SQLCipher nunca se toca: sigue cifrada con el mismo DEK de siempre.
    let new_params = KdfParams::recommended();
    let new_salt = super::kdf::Salt::generate().map_err(|_| ChangePasswordError::MetaFileUnreadable)?;
    let new_kek = derive_kek(new_password.as_bytes(), &new_salt, &new_params)
        .map_err(|_| ChangePasswordError::MetaFileUnreadable)?;
    let new_wrapped = wrap_dek(&dek, &new_kek).map_err(|_| ChangePasswordError::MetaFileUnreadable)?;

    meta.password_wrap = WrapRecord::new(&new_salt, new_params, &new_wrapped);
    // meta.recovery_wrap queda intacto: el código de recuperación sigue
    // funcionando después de cambiar la contraseña.
    meta.save(&paths.meta_path).map_err(ChangePasswordError::Io)
}

// ---------------------------------------------------------------------
// Recuperación de acceso mediante el código de recuperación
// ---------------------------------------------------------------------

/// Mismo motivo que `unlock_vault_without_migrating` — variante sin migrar,
/// de uso interno exclusivo de `backup::service::restore_backup` para poder
/// inspeccionar el `PRAGMA user_version` crudo del backup **staged** antes
/// de decidir si migrarlo.
pub(crate) fn recover_access_without_migrating(
    paths: &VaultPaths,
    recovery_code_input: &str,
    new_password: &str,
) -> Result<(Connection, VaultKey), RecoveryError> {
    password_policy::validate(new_password).map_err(RecoveryError::WeakPassword)?;
    let code = RecoveryCode::parse(recovery_code_input).map_err(|_| RecoveryError::InvalidCodeFormat)?;

    let mut meta = VaultMetaFile::load(&paths.meta_path).map_err(|_| RecoveryError::MetaFileUnreadable)?;

    let salt = meta.recovery_wrap.salt().map_err(|_| RecoveryError::MetaFileUnreadable)?;
    let wrapped = meta.recovery_wrap.wrapped_key().map_err(|_| RecoveryError::MetaFileUnreadable)?;
    let kek = derive_kek(code.as_bytes(), &salt, &meta.recovery_wrap.kdf)
        .map_err(|_| RecoveryError::MetaFileUnreadable)?;

    let dek = unwrap_dek(&wrapped, &kek).map_err(|_| RecoveryError::IncorrectRecoveryCode)?;

    // Se estableció una nueva contraseña: se envuelve el mismo DEK con una
    // KEK nueva. El envoltorio de recuperación (meta.recovery_wrap) queda
    // intacto — el mismo código de recuperación sigue sirviendo después.
    let new_params = KdfParams::recommended();
    let new_salt = super::kdf::Salt::generate().map_err(|_| RecoveryError::MetaFileUnreadable)?;
    let new_kek = derive_kek(new_password.as_bytes(), &new_salt, &new_params)
        .map_err(|_| RecoveryError::MetaFileUnreadable)?;
    let new_wrapped = wrap_dek(&dek, &new_kek).map_err(|_| RecoveryError::MetaFileUnreadable)?;

    meta.password_wrap = WrapRecord::new(&new_salt, new_params, &new_wrapped);
    meta.save(&paths.meta_path).map_err(RecoveryError::Io)?;

    let conn = db::open_vault(&paths.db_path, &dek).map_err(|_| RecoveryError::CorruptDatabase)?;
    Ok((conn, dek))
}

/// Punto de entrada real de la aplicación para recuperar acceso mediante el
/// código de recuperación. Mismo motivo que `unlock_vault` para migrar
/// siempre — ver el comentario ahí.
pub fn recover_access(
    paths: &VaultPaths,
    recovery_code_input: &str,
    new_password: &str,
) -> Result<(Connection, VaultKey), RecoveryError> {
    let (mut conn, dek) = recover_access_without_migrating(paths, recovery_code_input, new_password)?;
    db::run_migrations(&mut conn).map_err(|_| RecoveryError::MigrationFailed)?;
    Ok((conn, dek))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cc-vault-manager-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn begin_does_not_write_anything_to_disk() {
        let dir = temp_vault_dir("begin-no-disk-writes");
        let paths = VaultPaths::new(&dir);
        let pending = PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap();
        let _ = pending.recovery_code_display();
        assert!(!paths.exists(), "begin() no debe crear ningún archivo todavía");
    }

    #[test]
    fn finalize_creates_a_working_vault() {
        let dir = temp_vault_dir("finalize-creates-vault");
        let paths = VaultPaths::new(&dir);
        let pending = PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap();
        let (conn, _dek) = pending.finalize(&paths).unwrap();
        assert!(paths.exists());
        let count: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE type='table'", [], |r| r.get(0))
            .unwrap();
        assert!(count > 0);
    }

    #[test]
    fn unlock_with_correct_password_succeeds() {
        let dir = temp_vault_dir("unlock-correct");
        let paths = VaultPaths::new(&dir);
        PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap().finalize(&paths).unwrap();

        let (_conn, _dek) = unlock_vault(&paths, "ContrasenaSegura2026!").unwrap();
    }

    #[test]
    fn unlock_with_wrong_password_is_rejected() {
        let dir = temp_vault_dir("unlock-wrong");
        let paths = VaultPaths::new(&dir);
        PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap().finalize(&paths).unwrap();

        let err = unlock_vault(&paths, "OtraContrasenaCompletamenteDistinta!").unwrap_err();
        assert!(matches!(err, UnlockError::IncorrectPassword));
    }

    #[test]
    fn unlock_without_a_vault_reports_no_vault() {
        let dir = temp_vault_dir("unlock-no-vault");
        let paths = VaultPaths::new(&dir);
        let err = unlock_vault(&paths, "cualquiera").unwrap_err();
        assert!(matches!(err, UnlockError::NoVault));
    }

    /// Regresión WIN-DB-01/WIN-DB-02 (validación real de Windows,
    /// continuación post-Fase 19): reproduce el flujo real completo — un
    /// vault creado bajo un esquema anterior (V10, antes de que existieran
    /// `safety_plan_list_items`/las columnas nuevas de `safety_plan_contacts`/
    /// `library_resource_patients`), con datos reales ya guardados, abierto
    /// después por la función de desbloqueo REAL (`unlock_vault`, la misma
    /// que usa `commands::vault::unlock_vault` en la app) — nunca llamando a
    /// `run_migrations` directamente, como sí hacían (y siguen haciendo)
    /// todos los demás tests de este proyecto. Antes del fix, esta prueba
    /// fallaba exactamente como en Windows: el esquema se quedaba en V10 y
    /// `list_items`/`list_contacts`/`link_resource_to_patient` fallaban con
    /// "no such table"/"no such column".
    #[test]
    fn unlock_vault_upgrades_an_existing_v10_vault_and_makes_its_data_reachable() {
        let dir = temp_vault_dir("unlock-migrates-v10");
        let paths = VaultPaths::new(&dir);
        let password = "ContrasenaSegura2026!";

        // 1. Construir un vault "real" con el mismo mecanismo de creación
        //    (`PendingVaultCreation` + `vault.meta.json` real), pero
        //    deteniendo el esquema en V10 en vez de dejar que `finalize()`
        //    lo lleve a la última versión — imita exactamente un vault
        //    creado bajo un binario anterior a V11/V12.
        let pending = PendingVaultCreation::begin(password).unwrap();
        pending.meta.save(&paths.meta_path).unwrap();
        {
            let mut conn = db::open_vault(&paths.db_path, &pending.dek).unwrap();
            crate::db::migrate_to_v10_for_tests(&mut conn);

            conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'Paciente Legacy')", [])
                .unwrap();
            conn.execute(
                "INSERT INTO safety_plans (id, patient_id, version, status, confirmed_at) \
                 VALUES ('sp1', 'p1', 1, 'vigente', '2026-01-01T00:00:00.000Z')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO safety_plan_contacts (id, safety_plan_id, contact_type, name) \
                 VALUES ('c1', 'sp1', 'support_person', 'Amiga Legacy')",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO library_resources (id, title) VALUES ('r1', 'Guía ficticia')", [])
                .unwrap();

            let schema_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
            assert_eq!(schema_version, 10, "el vault de prueba debe quedar exactamente en V10 antes del desbloqueo real");
        } // la conexión se cierra aquí — imita cerrar y reabrir la app.

        // 2. El flujo real: desbloquear con la contraseña, exactamente como
        //    hace la UI en cada arranque — nunca `run_migrations` a mano.
        let (conn, _dek) = unlock_vault(&paths, password).expect("el desbloqueo real debe migrar automáticamente a la última versión");

        let schema_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(schema_version, 12, "unlock_vault debe dejar un vault V10 real en el esquema más reciente");

        // 3. Las funciones que Windows reportó rotas deben funcionar ahora
        //    sobre esta MISMA conexión — la que unlock_vault realmente
        //    devuelve, no una conexión de test aparte.
        use crate::services::{library, safety_plans};

        let items = safety_plans::list_items(&conn, "sp1").expect("Paso 1/2/3-lugares no debe fallar tras el desbloqueo real (WIN-DB-01)");
        assert!(items.is_empty(), "un plan V10 no tenía ítems de lista — deben leerse vacíos, nunca fallar");

        let contacts = safety_plans::list_contacts(&conn, "sp1").expect("Paso 3-personas/4/5 no debe fallar tras el desbloqueo real (WIN-DB-01)");
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].contact_type, "support_person", "un contacto V10 conserva su contact_type original");
        assert_eq!(contacts[0].name, "Amiga Legacy");

        library::link_resource_to_patient(&conn, "r1", "p1").expect("asociar Biblioteca<->paciente no debe fallar tras el desbloqueo real (WIN-DB-02)");
        let linked = library::list_resources_for_patient(&conn, "p1").expect("listar recursos del paciente no debe fallar tras el desbloqueo real (WIN-DB-02)");
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].title, "Guía ficticia");
    }

    /// Mismo escenario que la anterior, pero pasando por el flujo real de
    /// recuperación por código en vez del desbloqueo por contraseña — el
    /// otro punto de entrada que `security::session` expone a un vault
    /// existente.
    #[test]
    fn recover_access_also_upgrades_an_existing_v10_vault() {
        let dir = temp_vault_dir("recover-migrates-v10");
        let paths = VaultPaths::new(&dir);
        let pending = PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap();
        let recovery_code = pending.recovery_code_display();
        pending.meta.save(&paths.meta_path).unwrap();
        {
            let mut conn = db::open_vault(&paths.db_path, &pending.dek).unwrap();
            crate::db::migrate_to_v10_for_tests(&mut conn);
            conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
            conn.execute("INSERT INTO safety_plans (id, patient_id, version, status) VALUES ('sp1', 'p1', 1, 'borrador')", [])
                .unwrap();
        }

        let (conn, _dek) = recover_access(&paths, &recovery_code, "NuevaContrasenaSegura2026!").expect("recover_access debe migrar automáticamente igual que unlock_vault");
        let schema_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(schema_version, 12);

        use crate::services::safety_plans;
        safety_plans::list_items(&conn, "sp1").expect("no debe fallar tras recover_access sobre un vault V10 real");
    }

    #[test]
    fn recovery_code_unlocks_the_vault() {
        let dir = temp_vault_dir("recovery-unlocks");
        let paths = VaultPaths::new(&dir);
        let pending = PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap();
        let recovery_code = pending.recovery_code_display();
        pending.finalize(&paths).unwrap();

        let (_conn, _dek) = recover_access(&paths, &recovery_code, "NuevaContrasenaSegura2026!").unwrap();
    }

    #[test]
    fn wrong_recovery_code_is_rejected() {
        let dir = temp_vault_dir("recovery-wrong-code");
        let paths = VaultPaths::new(&dir);
        PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap().finalize(&paths).unwrap();

        let fake_code = "AAAA-AAAA-AAAA-AAAA-AAAA-AAAA";
        let err = recover_access(&paths, fake_code, "NuevaContrasenaSegura2026!").unwrap_err();
        assert!(matches!(err, RecoveryError::IncorrectRecoveryCode));
    }

    #[test]
    fn change_password_then_old_password_stops_working_and_new_one_works() {
        let dir = temp_vault_dir("change-password-basic");
        let paths = VaultPaths::new(&dir);
        PendingVaultCreation::begin("ContrasenaVieja2026!").unwrap().finalize(&paths).unwrap();

        change_password(&paths, "ContrasenaVieja2026!", "ContrasenaNueva2026!").unwrap();

        let old_err = unlock_vault(&paths, "ContrasenaVieja2026!").unwrap_err();
        assert!(matches!(old_err, UnlockError::IncorrectPassword));

        let (_conn, _dek) = unlock_vault(&paths, "ContrasenaNueva2026!").unwrap();
    }

    #[test]
    fn change_password_with_incorrect_current_password_is_rejected() {
        let dir = temp_vault_dir("change-password-wrong-current");
        let paths = VaultPaths::new(&dir);
        PendingVaultCreation::begin("ContrasenaVieja2026!").unwrap().finalize(&paths).unwrap();

        let err = change_password(&paths, "NoEsLaContrasenaActual!", "ContrasenaNueva2026!").unwrap_err();
        assert!(matches!(err, ChangePasswordError::IncorrectCurrentPassword));

        // La contraseña original debe seguir funcionando: el cambio no se aplicó.
        let (_conn, _dek) = unlock_vault(&paths, "ContrasenaVieja2026!").unwrap();
    }

    #[test]
    fn recovery_code_keeps_working_after_a_password_change() {
        let dir = temp_vault_dir("recovery-after-change");
        let paths = VaultPaths::new(&dir);
        let pending = PendingVaultCreation::begin("ContrasenaVieja2026!").unwrap();
        let recovery_code = pending.recovery_code_display();
        pending.finalize(&paths).unwrap();

        change_password(&paths, "ContrasenaVieja2026!", "ContrasenaNueva2026!").unwrap();

        let (_conn, _dek) = recover_access(&paths, &recovery_code, "TrasRecuperarNueva2026!").unwrap();
    }

    #[test]
    fn dek_is_unchanged_by_a_password_change() {
        let dir = temp_vault_dir("dek-unchanged-by-password-change");
        let paths = VaultPaths::new(&dir);
        PendingVaultCreation::begin("ContrasenaVieja2026!").unwrap().finalize(&paths).unwrap();

        let (_conn_before, dek_before) = unlock_vault(&paths, "ContrasenaVieja2026!").unwrap();
        change_password(&paths, "ContrasenaVieja2026!", "ContrasenaNueva2026!").unwrap();
        let (_conn_after, dek_after) = unlock_vault(&paths, "ContrasenaNueva2026!").unwrap();

        assert_eq!(dek_before.expose_secret(), dek_after.expose_secret());
    }

    #[test]
    fn weak_password_is_rejected_at_creation() {
        let err = PendingVaultCreation::begin("corta").unwrap_err();
        assert!(matches!(err, CreateVaultError::WeakPassword(_)));
    }

    #[test]
    fn corrupt_database_file_is_reported_distinctly_from_wrong_password() {
        let dir = temp_vault_dir("corrupt-db-distinct");
        let paths = VaultPaths::new(&dir);
        PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap().finalize(&paths).unwrap();

        // Se corrompe solo el archivo .db, dejando vault.meta.json intacto,
        // para simular un archivo dañado (no una contraseña incorrecta).
        std::fs::write(&paths.db_path, b"esto ya no es una base de datos valida").unwrap();

        let err = unlock_vault(&paths, "ContrasenaSegura2026!").unwrap_err();
        assert!(matches!(err, UnlockError::CorruptDatabase));
    }

    #[test]
    fn corrupt_meta_file_is_reported_distinctly() {
        let dir = temp_vault_dir("corrupt-meta-distinct");
        let paths = VaultPaths::new(&dir);
        PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap().finalize(&paths).unwrap();

        std::fs::write(&paths.meta_path, b"no es json valido").unwrap();

        let err = unlock_vault(&paths, "ContrasenaSegura2026!").unwrap_err();
        assert!(matches!(err, UnlockError::MetaFileUnreadable));
    }

    #[test]
    fn created_vault_is_actually_encrypted_on_disk() {
        let dir = temp_vault_dir("created-vault-is-encrypted");
        let paths = VaultPaths::new(&dir);
        let pending = PendingVaultCreation::begin("ContrasenaSegura2026!").unwrap();
        let (conn, _dek) = pending.finalize(&paths).unwrap();
        conn.execute(
            "INSERT INTO patients (id, full_name) VALUES ('p1', 'Nombre Clinico De Prueba')",
            [],
        )
        .unwrap();
        drop(conn);

        let bytes = std::fs::read(&paths.db_path).unwrap();
        const SQLITE_PLAINTEXT_HEADER: &[u8] = b"SQLite format 3\0";
        assert_ne!(&bytes[..SQLITE_PLAINTEXT_HEADER.len()], SQLITE_PLAINTEXT_HEADER);
        assert!(!bytes.windows(b"Nombre Clinico De Prueba".len()).any(|w| w == b"Nombre Clinico De Prueba"));
    }

    struct CapturingLogger {
        buf: std::sync::Mutex<Vec<String>>,
    }
    impl log::Log for CapturingLogger {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            self.buf.lock().unwrap().push(format!("{}", record.args()));
        }
        fn flush(&self) {}
    }

    static CAPTURING_LOGGER: std::sync::OnceLock<CapturingLogger> = std::sync::OnceLock::new();

    fn install_capturing_logger() -> &'static CapturingLogger {
        let logger = CAPTURING_LOGGER.get_or_init(|| CapturingLogger {
            buf: std::sync::Mutex::new(Vec::new()),
        });
        // Puede fallar si otro test ya lo instaló antes: es el mismo logger
        // (viene del mismo `OnceLock`), así que no importa.
        let _ = log::set_logger(logger);
        log::set_max_level(log::LevelFilter::Trace);
        logger
    }

    #[test]
    fn no_secret_material_appears_in_log_output() {
        let logger = install_capturing_logger();
        logger.buf.lock().unwrap().clear();

        let dir = temp_vault_dir("no-secrets-in-logs");
        let paths = VaultPaths::new(&dir);
        let unique_password = "InequivocamenteUnicaClave2026-XJ9";
        let unique_new_password = "OtraClaveTotalmenteNuevaUnica2026-QZ7";
        let unique_recovered_password = "TrasRecuperarClaveUnica2026-MK3";

        let pending = PendingVaultCreation::begin(unique_password).unwrap();
        let recovery_code = pending.recovery_code_display();
        let (conn, _dek) = pending.finalize(&paths).unwrap();
        drop(conn);

        let (conn2, dek) = unlock_vault(&paths, unique_password).unwrap();
        let dek_hex: String = dek.expose_secret().iter().map(|b| format!("{b:02x}")).collect();
        drop(conn2);

        change_password(&paths, unique_password, unique_new_password).unwrap();
        let (conn3, _dek2) = recover_access(&paths, &recovery_code, unique_recovered_password).unwrap();
        drop(conn3);

        let logs = logger.buf.lock().unwrap();
        let joined = logs.join("\n");
        assert!(!joined.contains(unique_password), "la contraseña original apareció en un log");
        assert!(!joined.contains(unique_new_password), "la nueva contraseña apareció en un log");
        assert!(!joined.contains(unique_recovered_password), "la contraseña post-recuperación apareció en un log");
        assert!(!joined.contains(&recovery_code), "el código de recuperación apareció en un log");
        assert!(!joined.contains(&dek_hex), "el DEK en hexadecimal apareció en un log");
    }

    #[test]
    fn vault_meta_file_never_contains_the_password_or_recovery_code_in_plain_text() {
        let dir = temp_vault_dir("no-plaintext-secrets-in-meta");
        let paths = VaultPaths::new(&dir);
        let pending = PendingVaultCreation::begin("MiContrasenaTotalmenteUnica2026!").unwrap();
        let recovery_code = pending.recovery_code_display();
        pending.finalize(&paths).unwrap();

        let contents = std::fs::read_to_string(&paths.meta_path).unwrap();
        assert!(!contents.contains("MiContrasenaTotalmenteUnica2026!"));
        assert!(!contents.contains(&recovery_code));
        // Tampoco cada grupo de 4 caracteres del código por separado.
        for group in recovery_code.split('-') {
            assert!(!contents.contains(group));
        }
    }
}
