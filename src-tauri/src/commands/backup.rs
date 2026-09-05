//! Comandos Tauri para Backup y Restore (Fase 10). Capa fina: obtiene
//! `vault_dir` exactamente como lo hace `lib.rs` al arrancar, delega toda
//! la lógica real a `backup::service`, y traduce errores a texto para la
//! UI. Nunca decide nada por sí sola — ni siquiera si el vault está
//! desbloqueado (eso lo exige `VaultSession::with_connection` dentro del
//! propio servicio).

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::backup::service::{self, BackupSummary, RestoreCredential, RestoreSummary};
use crate::backup::BackupManifest;
use crate::security::VaultSession;

type SharedVaultSession = Arc<VaultSession>;

fn vault_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("vault"))
        .map_err(|e| format!("no se pudo determinar el directorio de datos de la aplicación: {e}"))
}

#[tauri::command]
pub fn create_backup(app: AppHandle, destination_path: String, state: State<'_, SharedVaultSession>) -> Result<BackupSummary, String> {
    let dir = vault_dir(&app)?;
    service::create_backup(&state, &dir, std::path::Path::new(&destination_path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn inspect_backup(archive_path: String) -> Result<BackupManifest, String> {
    service::inspect_backup(std::path::Path::new(&archive_path)).map_err(|e| match e {
        service::InspectError::ArchiveUnreadable => "el archivo de respaldo no se pudo leer".to_string(),
        service::InspectError::ManifestMissing => "el respaldo no contiene manifest.json".to_string(),
        service::InspectError::ManifestInvalid => "el manifest del respaldo no es válido".to_string(),
    })
}

/// Credencial recibida desde React — se traduce a `RestoreCredential` en
/// esta misma capa fina, nunca en el servicio (que no sabe nada de JSON ni
/// de Tauri).
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RestoreCredentialInput {
    Password { password: String },
    RecoveryCode { code: String, new_password: String },
}

impl From<RestoreCredentialInput> for RestoreCredential {
    fn from(value: RestoreCredentialInput) -> Self {
        match value {
            RestoreCredentialInput::Password { password } => RestoreCredential::Password(password),
            RestoreCredentialInput::RecoveryCode { code, new_password } => RestoreCredential::RecoveryCode { code, new_password },
        }
    }
}

#[tauri::command]
pub fn restore_backup(
    app: AppHandle,
    archive_path: String,
    credential: RestoreCredentialInput,
    state: State<'_, SharedVaultSession>,
) -> Result<RestoreSummary, String> {
    let dir = vault_dir(&app)?;
    service::restore_backup(&state, &dir, std::path::Path::new(&archive_path), credential.into()).map_err(|e| e.to_string())
}
