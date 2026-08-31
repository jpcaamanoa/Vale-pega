//! Comandos Tauri de la integración con Google Calendar: configurar
//! credenciales, conectar (flujo OAuth completo), listar calendarios
//! existentes, seleccionar uno, desconectar, y reintentar manualmente la
//! sincronización de una cita puntual.
//!
//! Igual que `commands::appointments`: capa fina, sin SQL ni reglas de
//! negocio propias. La orquestación OAuth (generar PKCE/state, levantar el
//! listener de loopback, abrir el navegador, esperar el callback,
//! intercambiar el código) vive aquí porque es la única capa con acceso
//! simultáneo al runtime async de Tauri y al vault — `calendar::oauth` y
//! `calendar::client` son deliberadamente puros/sin estado.

use std::sync::Arc;
use std::time::Duration;

use tauri::State;

use crate::calendar::client::{self, GoogleCalendarListItem};
use crate::calendar::{oauth, sync, tokens};
use crate::security::VaultSession;

use super::appointments::{finish, AppointmentWithSync};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

/// Cuánto se espera a que la usuaria complete el consentimiento en el
/// navegador antes de abandonar el intento de conexión.
const AUTH_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleConnectionStatus {
    /// Hay Client ID/Client Secret guardados en Ajustes (`app_settings`,
    /// dentro del vault cifrado).
    pub credentials_configured: bool,
    /// Hay un `refresh_token` válido guardado en el keychain del sistema
    /// operativo — es decir, el flujo OAuth se completó al menos una vez y
    /// no se ha desconectado ni revocado desde entonces.
    pub connected: bool,
    pub calendar_id: Option<String>,
}

#[tauri::command]
pub fn google_connection_status(state: State<'_, SharedVaultSession>) -> Result<GoogleConnectionStatus, String> {
    let (credentials, calendar_id) = state
        .with_connection(|conn| (sync::get_credentials(conn), sync::get_selected_calendar_id(conn)))
        .map_err(|_| LOCKED_MESSAGE.to_string())?;
    let connected = matches!(tokens::load(), Ok(Some(_)));
    Ok(GoogleConnectionStatus { credentials_configured: credentials.is_some(), connected, calendar_id })
}

/// Guarda el Client ID/Client Secret del cliente OAuth "Desktop app" de
/// Google Cloud Console. No son secretos confidenciales para este tipo de
/// cliente (ver `docs/google-calendar.md`), así que se guardan en
/// `app_settings` (dentro del vault SQLCipher), no en el keychain.
#[tauri::command]
pub fn save_google_credentials(
    client_id: String,
    client_secret: String,
    state: State<'_, SharedVaultSession>,
) -> Result<(), String> {
    state
        .with_connection(|conn| sync::save_credentials(conn, &client_id, &client_secret))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Flujo completo OAuth 2.0 Authorization Code + PKCE: genera
/// `code_verifier`/`code_challenge`/`state`, levanta el listener de loopback
/// en `127.0.0.1`, abre el navegador con la URL de consentimiento de Google,
/// espera el callback validando `state`, e intercambia el código por
/// tokens. El `refresh_token` resultante se guarda exclusivamente en el
/// keychain del sistema operativo.
#[tauri::command]
pub async fn begin_google_auth(state: State<'_, SharedVaultSession>) -> Result<(), String> {
    let (client_id, client_secret) = state
        .with_connection(sync::get_credentials)
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .ok_or_else(|| "primero configura el Client ID y el Client Secret de Google en Ajustes".to_string())?;

    let verifier = oauth::generate_verifier();
    let challenge = oauth::code_challenge(&verifier);
    let expected_state = oauth::generate_state();

    let (listener, port) = oauth::bind_loopback_listener().map_err(|e| e.to_string())?;
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let auth_url = oauth::build_auth_url(&client_id, &redirect_uri, &challenge, &expected_state);

    open::that(&auth_url).map_err(|e| format!("no se pudo abrir el navegador: {e}"))?;

    let code = tokio::task::spawn_blocking(move || oauth::wait_for_redirect(listener, &expected_state, AUTH_TIMEOUT))
        .await
        .map_err(|e| format!("error interno esperando la autorización de Google: {e}"))?
        .map_err(|e| e.to_string())?;

    let token_response = client::exchange_code(&client_id, &client_secret, &code, &verifier, &redirect_uri)
        .await
        .map_err(|e| e.to_string())?;

    let refresh_token = token_response
        .refresh_token
        .ok_or_else(|| "Google no entregó un refresh token — vuelve a intentar la conexión".to_string())?;
    tokens::save(&tokens::RefreshToken::new(refresh_token)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_google_calendars(state: State<'_, SharedVaultSession>) -> Result<Vec<GoogleCalendarListItem>, String> {
    let (client_id, client_secret) = state
        .with_connection(sync::get_credentials)
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .ok_or_else(|| "Google no está configurado".to_string())?;

    let access_token = sync::get_valid_access_token(&client_id, &client_secret)
        .await
        .map_err(|_| "no se pudo obtener un token de acceso válido de Google — revisa la conexión".to_string())?;

    client::list_calendars(&access_token).await.map_err(|e| e.to_string())
}

/// Selecciona un calendario **existente** de la cuenta de Google conectada.
/// Nunca crea uno nuevo — la lista viene de `list_google_calendars`.
#[tauri::command]
pub fn select_google_calendar(calendar_id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state
        .with_connection(|conn| sync::set_selected_calendar_id(conn, &calendar_id))
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Desconecta la integración: revoca el token contra Google (best-effort —
/// si falla, igual se limpia localmente) y borra el `refresh_token` del
/// keychain y el calendario seleccionado. El Client ID/Client Secret
/// configurados en Ajustes **no** se borran, para no obligar a
/// reconfigurarlos si la usuaria vuelve a conectar.
#[tauri::command]
pub async fn disconnect_google_calendar(state: State<'_, SharedVaultSession>) -> Result<(), String> {
    if let Ok(Some(refresh_token)) = tokens::load() {
        let _ = client::revoke_token(refresh_token.expose_secret()).await;
    }
    let _ = tokens::clear();

    state
        .with_connection(sync::clear_selected_calendar_id)
        .map_err(|_| LOCKED_MESSAGE.to_string())?
        .map_err(|e| e.to_string())
}

/// Reintento manual de sincronización de una cita puntual — misma
/// reconciliación que se dispara automáticamente tras cada mutación.
#[tauri::command]
pub async fn retry_appointment_sync(id: String, state: State<'_, SharedVaultSession>) -> Result<AppointmentWithSync, String> {
    finish(&state, &id).await
}
