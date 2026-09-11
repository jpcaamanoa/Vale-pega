//! Estado de sesión del vault: si está desbloqueado ahora mismo, y el
//! bloqueo real (manual y automático por inactividad).
//!
//! Esto es lo único en todo el módulo de seguridad que mantiene estado
//! mutable compartido (protegido por un `Mutex`, pensado para vivir como
//! `tauri::State`). Todo lo demás en `security::` son funciones puras.

use std::fmt;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use hkdf::Hkdf;
use rusqlite::Connection;
use sha2::Sha256;
use zeroize::Zeroize;

use crate::db::VaultKey;

use super::random;
use super::vault_manager::{
    self, ChangePasswordError, CreateVaultError, FinalizeCreationError, PendingVaultCreation,
    RecoveryError, UnlockError, VaultPaths,
};

/// 15 minutos por defecto. Configurable más adelante desde `app_settings`
/// (Fase 2+); aquí solo se define el mecanismo.
pub const DEFAULT_AUTO_LOCK_TIMEOUT: Duration = Duration::from_secs(15 * 60);

struct AutoLockTracker {
    last_activity: Instant,
}

impl AutoLockTracker {
    fn new() -> Self {
        Self { last_activity: Instant::now() }
    }
    fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
    fn should_lock(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() >= timeout
    }
}

struct UnlockedSession {
    conn: Connection,
    /// Se retiene mientras la app está desbloqueada (no solo durante el
    /// `PRAGMA key` inicial) porque es lo que hay que zeroizar al bloquear
    /// — ver el límite ya aceptado en `docs/ARCHITECTURE.md` sección 5
    /// ("mientras la app está desbloqueada, el DEK vive en RAM").
    #[allow(dead_code)] // se usará para abrir conexiones adicionales en fases futuras
    dek: VaultKey,
    tracker: AutoLockTracker,
}

enum State {
    NoVault,
    Locked,
    PendingCreation(PendingVaultCreation),
    Unlocked(UnlockedSession),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultStatus {
    NoVault,
    Locked,
    PendingCreation,
    Unlocked,
}

#[derive(Debug)]
pub enum BeginCreationError {
    VaultAlreadyExists,
    Crypto(CreateVaultError),
}
impl fmt::Display for BeginCreationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BeginCreationError::VaultAlreadyExists => write!(f, "ya existe un vault en esta ubicación"),
            BeginCreationError::Crypto(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for BeginCreationError {}

#[derive(Debug)]
pub enum ConfirmCreationError {
    NoPendingCreation,
    Finalize(FinalizeCreationError),
}
impl fmt::Display for ConfirmCreationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfirmCreationError::NoPendingCreation => {
                write!(f, "no hay una creación de vault pendiente de confirmar")
            }
            ConfirmCreationError::Finalize(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for ConfirmCreationError {}

/// El vault está bloqueado, o nunca se desbloqueó: no hay conexión
/// disponible. Es el único error posible de `VaultSession::with_connection`,
/// y existe precisamente para que sea estructuralmente imposible tocar la
/// base de datos sin pasar por un desbloqueo real primero.
#[derive(Debug)]
pub struct VaultLockedError;
impl fmt::Display for VaultLockedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "el vault está bloqueado")
    }
}
impl std::error::Error for VaultLockedError {}

// ---------------------------------------------------------------------
// Fase 16 (Documentos cifrados) — capacidad criptográfica mínima expuesta
// a `services::document_crypto`, aprobada explícitamente en
// `Plan-Fase-16-Documentos-cifrados-pendiente-de-aprobacion.md` (Bloque 3).
//
// Principio de diseño: `services::document_crypto` nunca ve la DEK del
// vault — solo puede pedir "envuelve esta clave de archivo" o "desenvuelve
// este envoltorio", y solo mientras la sesión esté desbloqueada. La DEK del
// vault (`UnlockedSession::dek`) nunca cruza el límite de este módulo.
//
// El envoltorio reutiliza exactamente el mismo mecanismo ya auditado de
// `security::envelope` (AES-256-GCM, nonce aleatorio de 12 bytes nunca
// reutilizado) — deliberadamente reimplementado aquí, en vez de generalizar
// `envelope.rs`, porque la aprobación de Fase 16 acotó la modificación
// exclusivamente a este archivo (`security/session.rs`), no a todo
// `security/*`.
//
// CRYPTO-1 (hardening pre-RC, Fase 17) — aprobado en
// `Plan-Fase-17-Hardening-Pre-RC-pendiente-de-aprobacion.md`: hasta aquí, la MISMA DEK del vault
// se usaba tanto como clave raw de SQLCipher (`PRAGMA key`, `db::connection::open_vault`) como
// clave AES-256-GCM para envolver la DEK de cada archivo — sin ninguna separación de dominio
// criptográfica entre ambos usos. Se agrega `KeyWrapVersion` para distinguir dos esquemas de
// envoltura, versionados en la columna `documents.key_wrap_version` (`SCHEMA_V10`):
//
// - `Legacy` (`key_wrap_version = 1`): el algoritmo exacto de Fase 16, sin cambios — la DEK del
//   vault directamente como clave AES-256-GCM. Se sigue soportando indefinidamente para poder
//   leer los documentos que ya existen; `wrap_file_key` nunca vuelve a producirlo.
// - `DomainSeparated` (`key_wrap_version = 2`): la clave de envoltura ya no es la DEK cruda del
//   vault, sino una subclave derivada vía HKDF-SHA256 (`derive_file_wrap_key`) — separación de
//   dominio real. Es el único esquema que `wrap_file_key` produce desde esta fase.
//
// No hay ninguna migración automática de `Legacy` a `DomainSeparated`: un documento legacy
// permanece en `Legacy` para siempre, salvo que una fase futura explícita decida remigrarlo. Ver
// `docs/documents.md` para el riesgo residual documentado.
// ---------------------------------------------------------------------

pub const FILE_KEY_LEN: usize = 32;
pub const FILE_KEY_WRAP_NONCE_LEN: usize = 12;

/// Esquema usado para envolver la DEK de un archivo — corresponde 1:1 a la columna
/// `documents.key_wrap_version` (`SCHEMA_V10`), pero como tipo validado en vez de un entero mágico
/// propagándose por las capas de servicio/repositorio. Deliberadamente distinto, en concepto y en
/// numeración, de `documents.format_version` (que versiona el formato físico del ciphertext CCD1
/// en sí, no el esquema de envoltura de la DEK).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyWrapVersion {
    /// Fase 16: la DEK cruda del vault, usada directamente como clave AES-256-GCM.
    Legacy,
    /// Fase 17 (CRYPTO-1): subclave derivada vía HKDF-SHA256 con separación de dominio.
    DomainSeparated,
}

impl KeyWrapVersion {
    pub fn as_i64(self) -> i64 {
        match self {
            KeyWrapVersion::Legacy => 1,
            KeyWrapVersion::DomainSeparated => 2,
        }
    }
}

/// La columna `documents.key_wrap_version` trae un valor fuera de `{1, 2}` — no debería ocurrir
/// nunca en la práctica gracias al `CHECK` de `SCHEMA_V10`, pero `TryFrom` deja la conversión
/// explícita y sin pánico de todas formas, en vez de asumir silenciosamente un esquema por
/// defecto ante un dato inesperado.
#[derive(Debug)]
pub struct UnknownKeyWrapVersion(pub i64);
impl fmt::Display for UnknownKeyWrapVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "esquema de envoltura de clave desconocido: {}", self.0)
    }
}
impl std::error::Error for UnknownKeyWrapVersion {}

impl TryFrom<i64> for KeyWrapVersion {
    type Error = UnknownKeyWrapVersion;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(KeyWrapVersion::Legacy),
            2 => Ok(KeyWrapVersion::DomainSeparated),
            other => Err(UnknownKeyWrapVersion(other)),
        }
    }
}

/// Contexto HKDF de la derivación de la subclave de envoltura de archivos. El "v1" de esta cadena
/// versiona la DERIVACIÓN HKDF en sí (su algoritmo/contexto) — un concepto distinto de
/// `KeyWrapVersion::DomainSeparated` (`key_wrap_version = 2`), que versiona el ESQUEMA DE
/// ENVOLTURA completo del documento. Son dos numeraciones independientes que coinciden en "1"/"2"
/// por coincidencia de dónde está cada fase, no por relación entre sí — no cambiar este literal
/// una vez que existan documentos `DomainSeparated` reales: dejarían de poder desenvolverse con la
/// subclave correcta.
const FILE_KEY_WRAP_HKDF_INFO: &[u8] = b"cuaderno-clinico:file-key-wrap:v1";

/// Deriva la subclave de envoltura de archivos (CRYPTO-1) a partir de la DEK cruda del vault, vía
/// HKDF-SHA256 (RFC 5869) — separación de dominio real frente al uso de esa misma DEK como clave
/// raw de SQLCipher. `salt = None`: la entrada (la DEK del vault) ya es material de alta entropía
/// generado por un CSPRNG (`security::random`), nunca una contraseña de baja entropía — un salt
/// aleatorio no aportaría seguridad adicional aquí (RFC 5869 §3.1, "if the input key material is
/// already cryptographically strong ... the salt value is not needed"). La subclave derivada
/// recibe la misma higiene de memoria que el resto del material secreto del proyecto: se zeroiza
/// explícitamente en cuanto termina de usarse (ver los dos llamadores).
fn derive_file_wrap_key(vault_dek: &[u8; FILE_KEY_LEN]) -> [u8; FILE_KEY_LEN] {
    let hk = Hkdf::<Sha256>::new(None, vault_dek);
    let mut subkey = [0u8; FILE_KEY_LEN];
    hk.expand(FILE_KEY_WRAP_HKDF_INFO, &mut subkey)
        .expect("32 bytes de salida está muy por debajo del límite de HKDF-SHA256 (255 * 32 bytes, RFC 5869 §2.3)");
    subkey
}

/// Envuelve `file_key` con `dek` (la DEK cruda del vault) usando el esquema `DomainSeparated`
/// (`KeyWrapVersion::DomainSeparated`) — el único que se produce para documentos nuevos desde
/// Fase 17. Función libre, parametrizada por la DEK en vez de método de `VaultSession`, para poder
/// probarla con valores completamente conocidos sin necesitar un vault real desbloqueado.
fn wrap_file_key_with_dek(dek: &[u8; FILE_KEY_LEN], file_key: &[u8; FILE_KEY_LEN]) -> Result<WrappedFileKey, WrapFileKeyError> {
    let mut subkey = derive_file_wrap_key(dek);
    let nonce_bytes = random::bytes::<FILE_KEY_WRAP_NONCE_LEN>().map_err(WrapFileKeyError::Random)?;
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(subkey));
    subkey.zeroize();
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, file_key.as_slice())
        .map_err(|_| WrapFileKeyError::EncryptionFailed)?;
    Ok(WrappedFileKey { nonce: nonce_bytes, ciphertext })
}

/// Desenvuelve `wrapped` con `dek` (la DEK cruda del vault), despachando exclusivamente según
/// `version` — la columna `documents.key_wrap_version` es la única fuente de verdad, nunca hay
/// autodetección ni un intento de "probar con la otra versión si esta falla": una versión
/// desconocida o un desenvolvimiento fallido son siempre un error explícito, nunca un fallback
/// silencioso. Función libre por el mismo motivo que `wrap_file_key_with_dek`: testeable con
/// valores completamente conocidos.
fn unwrap_file_key_with_dek(dek: &[u8; FILE_KEY_LEN], wrapped: &WrappedFileKey, version: KeyWrapVersion) -> Result<FileKey, UnwrapFileKeyError> {
    let nonce = Nonce::from(wrapped.nonce);
    let mut plaintext = match version {
        KeyWrapVersion::Legacy => {
            let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*dek));
            cipher.decrypt(&nonce, wrapped.ciphertext.as_slice()).map_err(|_| UnwrapFileKeyError::UnwrapFailed)?
        }
        KeyWrapVersion::DomainSeparated => {
            let mut subkey = derive_file_wrap_key(dek);
            let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(subkey));
            subkey.zeroize();
            cipher.decrypt(&nonce, wrapped.ciphertext.as_slice()).map_err(|_| UnwrapFileKeyError::UnwrapFailed)?
        }
    };
    let result = <[u8; FILE_KEY_LEN]>::try_from(plaintext.as_slice()).map_err(|_| UnwrapFileKeyError::InvalidLength);
    plaintext.zeroize();
    result.map(FileKey)
}

/// Una DEK de archivo (Fase 16) ya desenvuelta — nunca la DEK del vault.
/// Mismo criterio de higiene que `db::VaultKey`/`security::kdf::Kek`:
/// `Debug` redactado, sin `Clone` (cada desenvoltura produce una instancia
/// nueva, de vida lo más corta posible), zeroizada al soltarse.
pub struct FileKey([u8; FILE_KEY_LEN]);

impl FileKey {
    /// Solo para consumo dentro de este mismo crate — `services::
    /// document_crypto` la usa para cifrar/descifrar el contenido de un
    /// archivo, nunca se expone fuera del binario.
    pub(crate) fn expose_secret(&self) -> &[u8; FILE_KEY_LEN] {
        &self.0
    }
}

impl fmt::Debug for FileKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileKey").field("bytes", &"<redacted>").finish()
    }
}

impl Drop for FileKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// DEK de archivo envuelta con la DEK del vault — lo único que se persiste
/// en la fila de `documents` (columnas `wrapped_file_dek`/`wrap_nonce`,
/// `SCHEMA_V9`). Ninguno de los dos campos es secreto por sí solo sin la
/// DEK del vault, que a su vez exige la sesión desbloqueada — mismo
/// principio que `security::envelope::WrappedKey`.
#[derive(Debug, Clone)]
pub struct WrappedFileKey {
    pub nonce: [u8; FILE_KEY_WRAP_NONCE_LEN],
    pub ciphertext: Vec<u8>,
}

#[derive(Debug)]
pub enum WrapFileKeyError {
    /// El vault está bloqueado — no hay ninguna DEK con la cual envolver.
    Locked,
    Random(getrandom::Error),
    /// No debería ocurrir con una clave de tamaño fijo válido.
    EncryptionFailed,
}
impl fmt::Display for WrapFileKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WrapFileKeyError::Locked => write!(f, "el vault está bloqueado"),
            WrapFileKeyError::Random(_) => write!(f, "no se pudo generar un nonce aleatorio"),
            WrapFileKeyError::EncryptionFailed => write!(f, "no se pudo envolver la clave del archivo"),
        }
    }
}
impl std::error::Error for WrapFileKeyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WrapFileKeyError::Random(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum UnwrapFileKeyError {
    /// El vault está bloqueado — no hay ninguna DEK con la cual desenvolver.
    Locked,
    /// DEK del vault incorrecta (no debería ocurrir con la sesión ya
    /// desbloqueada) o el envoltorio está dañado/manipulado — indistinguibles
    /// por diseño, mismo criterio que `EnvelopeError::UnwrapFailed`.
    UnwrapFailed,
    /// El contenido desenvuelto no mide exactamente `FILE_KEY_LEN` bytes.
    InvalidLength,
}
impl fmt::Display for UnwrapFileKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnwrapFileKeyError::Locked => write!(f, "el vault está bloqueado"),
            UnwrapFileKeyError::UnwrapFailed => write!(f, "no se pudo desenvolver la clave del archivo"),
            UnwrapFileKeyError::InvalidLength => write!(f, "la clave de archivo desenvuelta tiene un tamaño inválido"),
        }
    }
}
impl std::error::Error for UnwrapFileKeyError {}

/// Maneja el ciclo de vida completo de autenticación de un vault. Pensado
/// para vivir como estado compartido de Tauri (`tauri::Manager::manage`).
pub struct VaultSession {
    paths: VaultPaths,
    state: Mutex<State>,
    auto_lock_timeout: Mutex<Duration>,
}

impl VaultSession {
    pub fn new(vault_dir: &Path) -> Self {
        let paths = VaultPaths::new(vault_dir);
        let initial = if paths.exists() { State::Locked } else { State::NoVault };
        Self {
            paths,
            state: Mutex::new(initial),
            auto_lock_timeout: Mutex::new(DEFAULT_AUTO_LOCK_TIMEOUT),
        }
    }

    pub fn status(&self) -> VaultStatus {
        match &*self.state.lock().unwrap() {
            State::NoVault => VaultStatus::NoVault,
            State::Locked => VaultStatus::Locked,
            State::PendingCreation(_) => VaultStatus::PendingCreation,
            State::Unlocked(_) => VaultStatus::Unlocked,
        }
    }

    // -------------------------------------------------------------
    // Creación
    // -------------------------------------------------------------

    /// Genera el DEK, el código de recuperación y los envoltorios en
    /// memoria — no escribe nada en disco todavía. Devuelve el código de
    /// recuperación para mostrarlo en la UI.
    pub fn begin_creation(&self, password: &str) -> Result<String, BeginCreationError> {
        let mut state = self.state.lock().unwrap();
        if !matches!(*state, State::NoVault) {
            return Err(BeginCreationError::VaultAlreadyExists);
        }
        let pending = PendingVaultCreation::begin(password).map_err(BeginCreationError::Crypto)?;
        let display = pending.recovery_code_display();
        *state = State::PendingCreation(pending);
        Ok(display)
    }

    /// La usuaria decidió no continuar (p. ej. cerró la pantalla de
    /// creación): se descarta todo lo generado en memoria sin haber escrito
    /// nada en disco.
    pub fn cancel_creation(&self) {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, State::PendingCreation(_)) {
            *state = State::NoVault;
        }
    }

    /// Solo se llama después de que la usuaria confirmó explícitamente que
    /// guardó el código de recuperación. Recién aquí se escribe algo en
    /// disco: `vault.meta.json`, la base SQLCipher, y las migraciones.
    pub fn confirm_creation(&self) -> Result<(), ConfirmCreationError> {
        let mut state = self.state.lock().unwrap();
        let pending = match std::mem::replace(&mut *state, State::Locked) {
            State::PendingCreation(p) => p,
            other => {
                *state = other;
                return Err(ConfirmCreationError::NoPendingCreation);
            }
        };
        match pending.finalize(&self.paths) {
            Ok((conn, dek)) => {
                *state = State::Unlocked(UnlockedSession { conn, dek, tracker: AutoLockTracker::new() });
                Ok(())
            }
            Err(e) => {
                *state = State::NoVault;
                Err(ConfirmCreationError::Finalize(e))
            }
        }
    }

    // -------------------------------------------------------------
    // Desbloqueo / bloqueo
    // -------------------------------------------------------------

    pub fn unlock(&self, password: &str) -> Result<(), UnlockError> {
        let (conn, dek) = vault_manager::unlock_vault(&self.paths, password)?;
        let mut state = self.state.lock().unwrap();
        *state = State::Unlocked(UnlockedSession { conn, dek, tracker: AutoLockTracker::new() });
        Ok(())
    }

    pub fn recover_access(&self, recovery_code: &str, new_password: &str) -> Result<(), RecoveryError> {
        let (conn, dek) = vault_manager::recover_access(&self.paths, recovery_code, new_password)?;
        let mut state = self.state.lock().unwrap();
        *state = State::Unlocked(UnlockedSession { conn, dek, tracker: AutoLockTracker::new() });
        Ok(())
    }

    /// Cambiar la contraseña exige volver a probarla (no basta con que la
    /// sesión ya esté desbloqueada) — ver `docs/security.md`.
    pub fn change_password(&self, current_password: &str, new_password: &str) -> Result<(), ChangePasswordError> {
        vault_manager::change_password(&self.paths, current_password, new_password)
    }

    /// Bloqueo manual: cierra la conexión (se suelta aquí mismo) y zeroiza
    /// el DEK de memoria (vía el `Drop` de `VaultKey`).
    pub fn lock(&self) {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, State::Unlocked(_)) {
            *state = State::Locked;
            // `UnlockedSession` (con `conn` y `dek`) se suelta aquí al
            // reemplazar el estado: la conexión se cierra y el DEK se
            // zeroiza como parte del `Drop` de sus campos.
        }
    }

    /// Vuelve a comprobar en disco si hay un vault utilizable en `paths` y
    /// ajusta el estado en memoria en consecuencia — exactamente el mismo
    /// criterio que `VaultSession::new`. Pensado exclusivamente para
    /// `backup::service::restore_backup` (Fase 10): tras reemplazar los
    /// archivos del vault en disco (una instalación sin vault previo que
    /// recibe uno restaurado, o un vault existente reemplazado), el estado
    /// en memoria de esta sesión no se entera solo — nadie más vuelve a
    /// crear el proceso para que `new()` lo detecte de nuevo.
    ///
    /// No hace nada si el estado actual es `Unlocked` o `PendingCreation`:
    /// nunca se llama en esos casos (`restore_backup` siempre bloquea
    /// primero), y tocar esos estados aquí sería fuera del propósito único
    /// de este método.
    pub fn refresh_from_disk(&self) {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, State::Unlocked(_) | State::PendingCreation(_)) {
            return;
        }
        *state = if self.paths.exists() { State::Locked } else { State::NoVault };
    }

    /// Registra actividad de la usuaria (para el bloqueo automático). No
    /// hace nada si el vault no está desbloqueado.
    pub fn record_activity(&self) {
        let mut state = self.state.lock().unwrap();
        if let State::Unlocked(session) = &mut *state {
            session.tracker.touch();
        }
    }

    pub fn set_auto_lock_timeout(&self, timeout: Duration) {
        *self.auto_lock_timeout.lock().unwrap() = timeout;
    }

    /// Si corresponde, bloquea por inactividad. Pensado para llamarse
    /// periódicamente desde una tarea en segundo plano (ver `commands`).
    /// Devuelve `true` si efectivamente bloqueó en esta llamada.
    ///
    /// **Lo que NO hace todavía** (delimitado a propósito, no simulado):
    /// no reacciona a que el sistema operativo se suspenda o se bloquee la
    /// pantalla — eso requiere integración nativa por plataforma
    /// (NSWorkspace en macOS, mensajes de sesión de Windows, señales de
    /// login1 por D-Bus en Linux) que no se implementa en esta fase. Solo
    /// cubre inactividad medida por tiempo transcurrido desde la última
    /// `record_activity()`.
    pub fn tick_auto_lock(&self) -> bool {
        let timeout = *self.auto_lock_timeout.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        let should_lock = matches!(&*state, State::Unlocked(s) if s.tracker.should_lock(timeout));
        if should_lock {
            *state = State::Locked;
        }
        should_lock
    }

    /// Único punto de acceso a la conexión de base de datos. Si el vault
    /// está bloqueado (o nunca se desbloqueó), devuelve
    /// `Err(VaultLockedError)` — no hay ninguna otra vía para llegar a la
    /// conexión, así que es estructuralmente imposible leer datos clínicos
    /// mientras la app está bloqueada.
    pub fn with_connection<T>(&self, f: impl FnOnce(&Connection) -> T) -> Result<T, VaultLockedError> {
        let state = self.state.lock().unwrap();
        match &*state {
            State::Unlocked(session) => Ok(f(&session.conn)),
            _ => Err(VaultLockedError),
        }
    }

    /// Envuelve una DEK de archivo con la DEK del vault de esta sesión — la DEK del vault nunca
    /// sale de este método. Nonce aleatorio nuevo en cada llamada, nunca reutilizado, mismo
    /// criterio que `security::envelope::wrap_dek`. Desde Fase 17 (CRYPTO-1) produce
    /// exclusivamente el esquema `KeyWrapVersion::DomainSeparated` — quien llama debe persistir
    /// `KeyWrapVersion::DomainSeparated.as_i64()` (2) en `documents.key_wrap_version` junto al
    /// resultado; este método no toca esa columna, solo produce el envoltorio.
    pub fn wrap_file_key(&self, file_key: &[u8; FILE_KEY_LEN]) -> Result<WrappedFileKey, WrapFileKeyError> {
        let state = self.state.lock().unwrap();
        let dek = match &*state {
            State::Unlocked(session) => session.dek.expose_secret(),
            _ => return Err(WrapFileKeyError::Locked),
        };
        wrap_file_key_with_dek(dek, file_key)
    }

    /// Desenvuelve una DEK de archivo con la DEK del vault de esta sesión, según el esquema
    /// indicado por `version` — la columna `documents.key_wrap_version` de la fila en cuestión es
    /// la única fuente de verdad sobre qué `KeyWrapVersion` corresponde; nunca hay autodetección
    /// ni un intento silencioso con la otra versión. Devuelve la `FileKey` ya lista para usar —
    /// nunca la DEK del vault en sí.
    pub fn unwrap_file_key(&self, wrapped: &WrappedFileKey, version: KeyWrapVersion) -> Result<FileKey, UnwrapFileKeyError> {
        let state = self.state.lock().unwrap();
        let dek = match &*state {
            State::Unlocked(session) => session.dek.expose_secret(),
            _ => return Err(UnwrapFileKeyError::Locked),
        };
        unwrap_file_key_with_dek(dek, wrapped, version)
    }

    /// Solo para tests de integración de otras capas (`services::documents`) que necesitan
    /// construir, con la DEK real de un vault de prueba ya desbloqueado, un envoltorio `Legacy`
    /// (`key_wrap_version = 1`) realista — para verificar que los documentos que ya existían
    /// antes de Fase 17 siguen abriendo correctamente. Es literalmente el cuerpo íntegro, sin
    /// cambios, de lo que era `wrap_file_key` antes de esta fase. `#[cfg(test)]`: no se compila en
    /// ningún build de release, y nunca expone la DEK del vault en sí fuera de este módulo — igual
    /// que `wrap_file_key`, solo devuelve el envoltorio ya cifrado. No existe ningún camino de
    /// producción (fuera de tests) que pueda producir un envoltorio `Legacy` nuevo: `wrap_file_key`
    /// sin `cfg(test)` siempre produce `DomainSeparated`.
    #[cfg(test)]
    pub(crate) fn wrap_file_key_as_legacy_for_tests(&self, file_key: &[u8; FILE_KEY_LEN]) -> Result<WrappedFileKey, WrapFileKeyError> {
        let state = self.state.lock().unwrap();
        let dek = match &*state {
            State::Unlocked(session) => session.dek.expose_secret(),
            _ => return Err(WrapFileKeyError::Locked),
        };
        let nonce_bytes = random::bytes::<FILE_KEY_WRAP_NONCE_LEN>().map_err(WrapFileKeyError::Random)?;
        let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*dek));
        let nonce = Nonce::from(nonce_bytes);
        let ciphertext = cipher.encrypt(&nonce, file_key.as_slice()).map_err(|_| WrapFileKeyError::EncryptionFailed)?;
        Ok(WrappedFileKey { nonce: nonce_bytes, ciphertext })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cc-session-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fresh_directory_reports_no_vault() {
        let dir = temp_vault_dir("fresh-no-vault");
        let session = VaultSession::new(&dir);
        assert_eq!(session.status(), VaultStatus::NoVault);
    }

    #[test]
    fn full_creation_flow_ends_unlocked() {
        let dir = temp_vault_dir("full-creation-flow");
        let session = VaultSession::new(&dir);

        let recovery_code = session.begin_creation("ContrasenaSegura2026!").unwrap();
        assert_eq!(session.status(), VaultStatus::PendingCreation);
        assert!(!recovery_code.is_empty());

        session.confirm_creation().unwrap();
        assert_eq!(session.status(), VaultStatus::Unlocked);
    }

    #[test]
    fn cancelling_creation_leaves_no_vault_on_disk() {
        let dir = temp_vault_dir("cancel-creation");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.cancel_creation();

        assert_eq!(session.status(), VaultStatus::NoVault);
        assert!(!VaultPaths::new(&dir).exists());
    }

    #[test]
    fn reopening_an_existing_vault_starts_locked() {
        let dir = temp_vault_dir("reopen-starts-locked");
        {
            let session = VaultSession::new(&dir);
            session.begin_creation("ContrasenaSegura2026!").unwrap();
            session.confirm_creation().unwrap();
        }
        let session2 = VaultSession::new(&dir);
        assert_eq!(session2.status(), VaultStatus::Locked);
    }

    #[test]
    fn unlock_and_lock_roundtrip() {
        let dir = temp_vault_dir("unlock-lock-roundtrip");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();

        session.lock();
        assert_eq!(session.status(), VaultStatus::Locked);

        session.unlock("ContrasenaSegura2026!").unwrap();
        assert_eq!(session.status(), VaultStatus::Unlocked);
    }

    #[test]
    fn cannot_access_the_connection_while_locked() {
        let dir = temp_vault_dir("no-access-while-locked");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();
        session.lock();

        let result = session.with_connection(|conn| {
            conn.query_row::<i64, _, _>("SELECT count(*) FROM patients", [], |r| r.get(0))
        });
        assert!(matches!(result, Err(VaultLockedError)));
    }

    #[test]
    fn patient_operations_are_rejected_at_the_backend_while_locked() {
        use crate::services::patients::{self, PatientInput};

        fn blank_input(name: &str) -> PatientInput {
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

        let dir = temp_vault_dir("patient-ops-rejected-while-locked");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();

        // Con el vault desbloqueado, crear un paciente funciona y persiste.
        let created_id = session
            .with_connection(|conn| patients::create_patient(conn, blank_input("Ana Pérez")))
            .unwrap()
            .unwrap()
            .id;

        session.lock();

        // Bloqueado: ninguna operación de pacientes puede ni siquiera
        // intentarse, porque no hay forma de obtener una `&Connection`.
        let create_result =
            session.with_connection(|conn| patients::create_patient(conn, blank_input("Bruno Soto")));
        assert!(matches!(create_result, Err(VaultLockedError)));

        let list_result = session.with_connection(|conn| patients::list_patients(conn, None));
        assert!(matches!(list_result, Err(VaultLockedError)));

        let get_result = session.with_connection(|conn| patients::get_patient(conn, &created_id));
        assert!(matches!(get_result, Err(VaultLockedError)));

        let update_result =
            session.with_connection(|conn| patients::update_patient(conn, &created_id, blank_input("Otro Nombre")));
        assert!(matches!(update_result, Err(VaultLockedError)));

        let archive_result = session.with_connection(|conn| patients::archive_patient(conn, &created_id));
        assert!(matches!(archive_result, Err(VaultLockedError)));

        // Y al desbloquear de nuevo, el paciente sigue exactamente como
        // se dejó (no se perdió ni se corrompió nada durante el bloqueo).
        session.unlock("ContrasenaSegura2026!").unwrap();
        let still_there = session
            .with_connection(|conn| patients::get_patient(conn, &created_id))
            .unwrap()
            .unwrap();
        assert_eq!(still_there.full_name, "Ana Pérez");
    }

    /// Simula el ciclo completo pedido en el criterio de terminado de la
    /// Fase 1.5: crear vault → crear paciente → "cerrar la app" (soltar
    /// `VaultSession`, lo que cierra la conexión) → "reabrir la app" (una
    /// `VaultSession` nueva sobre el mismo directorio) → desbloquear →
    /// encontrar el paciente exactamente como quedó.
    #[test]
    fn patient_survives_a_full_close_and_reopen_of_the_app() {
        use crate::services::patients::{self, PatientInput};

        let dir = temp_vault_dir("patient-survives-close-reopen");
        let patient_id;
        {
            let session = VaultSession::new(&dir);
            session.begin_creation("ContrasenaSegura2026!").unwrap();
            session.confirm_creation().unwrap();

            let input = PatientInput {
                full_name: "Constanza Rivas".to_string(),
                preferred_name: Some("Coni".to_string()),
                rut: Some("12.345.678-5".to_string()),
                birth_date: Some("1990-05-12".to_string()),
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
            patient_id = session
                .with_connection(|conn| patients::create_patient(conn, input))
                .unwrap()
                .unwrap()
                .id;
        } // `session` se suelta aquí: simula cerrar la aplicación.

        // "Reabrir la aplicación": una VaultSession completamente nueva
        // apuntando al mismo directorio, que no hereda nada en memoria de
        // la anterior.
        let reopened = VaultSession::new(&dir);
        assert_eq!(reopened.status(), VaultStatus::Locked);
        reopened.unlock("ContrasenaSegura2026!").unwrap();

        let found = reopened
            .with_connection(|conn| patients::get_patient(conn, &patient_id))
            .unwrap()
            .unwrap();
        assert_eq!(found.full_name, "Constanza Rivas");
        assert_eq!(found.preferred_name.as_deref(), Some("Coni"));
        assert_eq!(found.rut.as_deref(), Some("12345678-5"));
    }

    #[test]
    fn can_access_the_connection_and_read_real_data_while_unlocked() {
        let dir = temp_vault_dir("access-while-unlocked");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();

        session
            .with_connection(|conn| {
                conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", [])
            })
            .unwrap()
            .unwrap();

        let count = session
            .with_connection(|conn| conn.query_row::<i64, _, _>("SELECT count(*) FROM patients", [], |r| r.get(0)))
            .unwrap()
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn auto_lock_ticks_do_nothing_before_the_timeout() {
        let dir = temp_vault_dir("auto-lock-not-yet");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();
        session.set_auto_lock_timeout(Duration::from_secs(60));

        assert!(!session.tick_auto_lock());
        assert_eq!(session.status(), VaultStatus::Unlocked);
    }

    #[test]
    fn auto_lock_locks_after_the_configured_timeout_of_inactivity() {
        let dir = temp_vault_dir("auto-lock-fires");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();
        session.set_auto_lock_timeout(Duration::from_millis(20));

        std::thread::sleep(Duration::from_millis(60));

        assert!(session.tick_auto_lock());
        assert_eq!(session.status(), VaultStatus::Locked);
    }

    #[test]
    fn recording_activity_resets_the_auto_lock_timer() {
        let dir = temp_vault_dir("auto-lock-activity-resets");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();
        session.set_auto_lock_timeout(Duration::from_millis(50));

        std::thread::sleep(Duration::from_millis(30));
        session.record_activity();
        std::thread::sleep(Duration::from_millis(30));

        // 60ms transcurridos en total, pero solo 30ms desde la última
        // actividad registrada — no debería haber bloqueado todavía.
        assert!(!session.tick_auto_lock());
        assert_eq!(session.status(), VaultStatus::Unlocked);
    }

    // -----------------------------------------------------------------
    // CRYPTO-1 (hardening pre-RC, Fase 17): HKDF-SHA256 domain-separated wrapping (v2) y
    // compatibilidad indefinida con el algoritmo legacy (v1).
    // -----------------------------------------------------------------

    #[test]
    fn wrap_then_unwrap_domain_separated_roundtrips() {
        let dir = temp_vault_dir("crypto1-roundtrip-v2");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();

        let file_key = [0x77u8; FILE_KEY_LEN];
        let wrapped = session.wrap_file_key(&file_key).unwrap();
        let unwrapped = session.unwrap_file_key(&wrapped, KeyWrapVersion::DomainSeparated).unwrap();
        assert_eq!(unwrapped.expose_secret(), &file_key);
    }

    #[test]
    fn wrap_file_key_always_produces_the_domain_separated_scheme() {
        // No hay ninguna forma pública de producir un envoltorio v1 nuevo desde esta fase —
        // wrap_file_key es, en sí mismo, la garantía de que ningún documento nuevo puede terminar
        // en key_wrap_version = 1. Se comprueba indirectamente: lo que wrap_file_key produce solo
        // puede desenvolverse correctamente con KeyWrapVersion::DomainSeparated.
        let dir = temp_vault_dir("crypto1-wrap-always-v2");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();

        let file_key = [0x55u8; FILE_KEY_LEN];
        let wrapped = session.wrap_file_key(&file_key).unwrap();
        assert!(session.unwrap_file_key(&wrapped, KeyWrapVersion::DomainSeparated).is_ok());
    }

    #[test]
    fn a_domain_separated_wrapped_key_cannot_be_unwrapped_as_legacy() {
        // Demuestra que la separación de dominio es real y no cosmética: la clave usada para
        // envolver bajo v2 (la subclave derivada por HKDF) es genuinamente distinta de la DEK
        // cruda del vault que usaría v1 — intentar desenvolver un envoltorio v2 como si fuera v1
        // falla la autenticación de AES-256-GCM (clave incorrecta), nunca produce un resultado
        // incorrecto en silencio.
        let dir = temp_vault_dir("crypto1-v2-rejected-as-v1");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();

        let file_key = [0x33u8; FILE_KEY_LEN];
        let wrapped = session.wrap_file_key(&file_key).unwrap();
        let err = session.unwrap_file_key(&wrapped, KeyWrapVersion::Legacy).unwrap_err();
        assert!(matches!(err, UnwrapFileKeyError::UnwrapFailed));
    }

    /// Fase 18 (validación pre-RC, escenario H): un `wrapped_file_dek` manipulado (un solo byte
    /// del ciphertext alterado) debe fallar la autenticación AES-256-GCM al desenvolver — nunca
    /// producir una `FileKey` incorrecta en silencio. Mismo principio ya probado a nivel de
    /// contenido del archivo (`document_crypto::tampered_ciphertext_is_rejected`), verificado aquí
    /// explícitamente también a nivel de la propia envoltura de la DEK.
    #[test]
    fn a_tampered_wrapped_ciphertext_is_rejected() {
        let dir = temp_vault_dir("crypto1-tampered-wrapped-ciphertext");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();

        let file_key = [0x44u8; FILE_KEY_LEN];
        let mut wrapped = session.wrap_file_key(&file_key).unwrap();
        let last = wrapped.ciphertext.len() - 1;
        wrapped.ciphertext[last] ^= 0xFF;

        let err = session.unwrap_file_key(&wrapped, KeyWrapVersion::DomainSeparated).unwrap_err();
        assert!(matches!(err, UnwrapFileKeyError::UnwrapFailed));
    }

    /// Fase 18 (validación pre-RC, escenario H): un `wrap_nonce` manipulado también debe fallar la
    /// autenticación — el nonce es parte de la entrada autenticada de AES-256-GCM, no un dato
    /// suelto que pueda cambiarse sin invalidar el ciphertext.
    #[test]
    fn a_tampered_wrap_nonce_is_rejected() {
        let dir = temp_vault_dir("crypto1-tampered-wrap-nonce");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();

        let file_key = [0x55u8; FILE_KEY_LEN];
        let mut wrapped = session.wrap_file_key(&file_key).unwrap();
        wrapped.nonce[0] ^= 0xFF;

        let err = session.unwrap_file_key(&wrapped, KeyWrapVersion::DomainSeparated).unwrap_err();
        assert!(matches!(err, UnwrapFileKeyError::UnwrapFailed));
    }

    /// Fixture congelado del algoritmo EXACTO de Fase 16 (`key_wrap_version = 1`): DEK del vault
    /// usada directamente como clave AES-256-GCM, sin ninguna derivación. Los bytes de
    /// `LEGACY_FIXTURE_CIPHERTEXT` se calcularon UNA SOLA VEZ, de forma independiente, con un
    /// programa Rust desechable que usa directamente el crate `aes-gcm` (misma versión que este
    /// proyecto) — nunca invocando `wrap_file_key`/`unwrap_file_key_with_dek` de este archivo.
    /// Deliberado: si se generara dinámicamente en este mismo test usando el código de este
    /// módulo, un cambio futuro que alterara silenciosamente el algoritmo "legacy" (por ejemplo,
    /// si alguien agregara sin querer separación de dominio también a `Legacy`) podría cambiar
    /// productor y consumidor a la vez sin que el test lo detectara. Con bytes fijos, ese cambio
    /// rompe este test de inmediato.
    const LEGACY_FIXTURE_VAULT_DEK: [u8; FILE_KEY_LEN] = [0x42; FILE_KEY_LEN];
    const LEGACY_FIXTURE_FILE_KEY_PLAINTEXT: [u8; FILE_KEY_LEN] = [0x99; FILE_KEY_LEN];
    const LEGACY_FIXTURE_NONCE: [u8; FILE_KEY_WRAP_NONCE_LEN] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    const LEGACY_FIXTURE_CIPHERTEXT: [u8; 48] = [
        44, 99, 191, 213, 212, 234, 208, 154, 21, 67, 40, 81, 211, 16, 247, 85, 9, 38, 90, 95, 158, 6, 174, 235, 23, 75, 69, 128, 6, 215, 151, 103, 168, 252,
        217, 223, 208, 23, 39, 156, 179, 100, 18, 197, 82, 61, 209, 241,
    ];

    #[test]
    fn unwrap_legacy_frozen_fixture_still_decrypts() {
        let wrapped = WrappedFileKey { nonce: LEGACY_FIXTURE_NONCE, ciphertext: LEGACY_FIXTURE_CIPHERTEXT.to_vec() };
        let unwrapped = unwrap_file_key_with_dek(&LEGACY_FIXTURE_VAULT_DEK, &wrapped, KeyWrapVersion::Legacy)
            .expect("el algoritmo legacy (v1) debe seguir siendo legible indefinidamente, sin ninguna migración");
        assert_eq!(unwrapped.expose_secret(), &LEGACY_FIXTURE_FILE_KEY_PLAINTEXT);
    }

    #[test]
    fn legacy_frozen_fixture_cannot_be_unwrapped_as_domain_separated() {
        let wrapped = WrappedFileKey { nonce: LEGACY_FIXTURE_NONCE, ciphertext: LEGACY_FIXTURE_CIPHERTEXT.to_vec() };
        let err = unwrap_file_key_with_dek(&LEGACY_FIXTURE_VAULT_DEK, &wrapped, KeyWrapVersion::DomainSeparated).unwrap_err();
        assert!(matches!(err, UnwrapFileKeyError::UnwrapFailed));
    }

    #[test]
    fn derive_file_wrap_key_is_deterministic_for_the_same_vault_dek() {
        let a = derive_file_wrap_key(&LEGACY_FIXTURE_VAULT_DEK);
        let b = derive_file_wrap_key(&LEGACY_FIXTURE_VAULT_DEK);
        assert_eq!(a, b, "HKDF debe ser determinista: la misma DEK del vault siempre deriva la misma subclave");
    }

    #[test]
    fn derive_file_wrap_key_differs_from_the_vault_dek_itself() {
        let derived = derive_file_wrap_key(&LEGACY_FIXTURE_VAULT_DEK);
        assert_ne!(derived, LEGACY_FIXTURE_VAULT_DEK, "la subclave derivada nunca debe coincidir con la DEK cruda del vault");
    }

    #[test]
    fn derive_file_wrap_key_differs_between_different_vault_deks() {
        let dek_a = [0x11u8; FILE_KEY_LEN];
        let dek_b = [0x22u8; FILE_KEY_LEN];
        assert_ne!(derive_file_wrap_key(&dek_a), derive_file_wrap_key(&dek_b));
    }

    #[test]
    fn key_wrap_version_round_trips_through_its_integer_representation() {
        assert_eq!(KeyWrapVersion::Legacy.as_i64(), 1);
        assert_eq!(KeyWrapVersion::DomainSeparated.as_i64(), 2);
        assert_eq!(KeyWrapVersion::try_from(1).unwrap(), KeyWrapVersion::Legacy);
        assert_eq!(KeyWrapVersion::try_from(2).unwrap(), KeyWrapVersion::DomainSeparated);
    }

    #[test]
    fn key_wrap_version_rejects_any_value_outside_one_or_two() {
        assert!(KeyWrapVersion::try_from(0).is_err());
        assert!(KeyWrapVersion::try_from(3).is_err());
        assert!(KeyWrapVersion::try_from(-1).is_err());
    }

    #[test]
    fn wrap_and_unwrap_are_rejected_while_locked_regardless_of_version() {
        let dir = temp_vault_dir("crypto1-locked");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();
        let file_key = [0x66u8; FILE_KEY_LEN];
        let wrapped = session.wrap_file_key(&file_key).unwrap();

        session.lock();

        assert!(matches!(session.wrap_file_key(&file_key).unwrap_err(), WrapFileKeyError::Locked));
        assert!(matches!(session.unwrap_file_key(&wrapped, KeyWrapVersion::DomainSeparated).unwrap_err(), UnwrapFileKeyError::Locked));
        assert!(matches!(session.unwrap_file_key(&wrapped, KeyWrapVersion::Legacy).unwrap_err(), UnwrapFileKeyError::Locked));
    }

    #[test]
    fn cannot_begin_creation_when_a_vault_already_exists() {
        let dir = temp_vault_dir("no-double-creation");
        let session = VaultSession::new(&dir);
        session.begin_creation("ContrasenaSegura2026!").unwrap();
        session.confirm_creation().unwrap();
        session.lock();

        let err = session.begin_creation("OtraContrasenaSegura2026!").unwrap_err();
        assert!(matches!(err, BeginCreationError::VaultAlreadyExists));
    }
}
