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

use rusqlite::Connection;

use crate::db::VaultKey;

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
    // Todavía no hay ningún comando Tauri de datos clínicos que la lea
    // (llega en la Fase 1.5); por ahora solo la usan los tests de
    // `with_connection` de este mismo archivo.
    #[allow(dead_code)]
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
// Sin uso en producción todavía (llega con los primeros comandos de datos
// clínicos en la Fase 1.5); lo ejercitan los tests de este archivo.
#[allow(dead_code)]
#[derive(Debug)]
pub struct VaultLockedError;
impl fmt::Display for VaultLockedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "el vault está bloqueado")
    }
}
impl std::error::Error for VaultLockedError {}

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
    // Sin comando Tauri todavía (llega con el primer repositorio de datos
    // clínicos en la Fase 1.5); lo ejercitan los tests de este archivo.
    #[allow(dead_code)]
    pub fn with_connection<T>(&self, f: impl FnOnce(&Connection) -> T) -> Result<T, VaultLockedError> {
        let state = self.state.lock().unwrap();
        match &*state {
            State::Unlocked(session) => Ok(f(&session.conn)),
            _ => Err(VaultLockedError),
        }
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
