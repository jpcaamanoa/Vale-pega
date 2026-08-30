//! Comandos Tauri para el ciclo de vida de autenticación del vault (Fase
//! 1.4). Cada uno es una capa fina: valida nada por sí mismo, solo traduce
//! la llamada IPC a una llamada al `VaultSession` compartido y convierte el
//! error a un mensaje de texto seguro para mostrar en la UI (los mensajes ya
//! están diseñados para no revelar más de lo necesario — ver
//! `security::vault_manager` y `docs/security.md`).

use std::sync::Arc;

use tauri::State;

use crate::security::{PasswordStrength, VaultSession, VaultStatus};

type SharedVaultSession = Arc<VaultSession>;

#[tauri::command]
pub fn vault_status(state: State<'_, SharedVaultSession>) -> VaultStatus {
    state.status()
}

#[tauri::command]
pub fn evaluate_password_strength(password: String) -> PasswordStrength {
    crate::security::evaluate_password_strength(&password)
}

#[tauri::command]
pub fn begin_vault_creation(password: String, state: State<'_, SharedVaultSession>) -> Result<String, String> {
    state.begin_creation(&password).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn confirm_vault_creation(state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.confirm_creation().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_vault_creation(state: State<'_, SharedVaultSession>) {
    state.cancel_creation();
}

#[tauri::command]
pub fn unlock_vault(password: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.unlock(&password).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn recover_vault_access(
    recovery_code: String,
    new_password: String,
    state: State<'_, SharedVaultSession>,
) -> Result<(), String> {
    state
        .recover_access(&recovery_code, &new_password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn change_vault_password(
    current_password: String,
    new_password: String,
    state: State<'_, SharedVaultSession>,
) -> Result<(), String> {
    state
        .change_password(&current_password, &new_password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn lock_vault(state: State<'_, SharedVaultSession>) {
    state.lock();
}

#[tauri::command]
pub fn record_vault_activity(state: State<'_, SharedVaultSession>) {
    state.record_activity();
}

/// Configura el período de inactividad tras el cual la app se bloquea sola.
/// Todavía no hay una pantalla de configuración que lo exponga (eso es de
/// una fase posterior), pero el mecanismo ya es real y configurable, tal
/// como se pidió.
#[tauri::command]
pub fn set_auto_lock_timeout_seconds(seconds: u64, state: State<'_, SharedVaultSession>) {
    state.set_auto_lock_timeout(std::time::Duration::from_secs(seconds));
}
