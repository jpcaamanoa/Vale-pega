//! Llamadas HTTP a Google: intercambio/renovación/revocación de tokens,
//! listado de calendarios, y CRUD de eventos. Ningún dato clínico entra
//! nunca a este archivo — las funciones de creación/actualización de
//! eventos reciben únicamente `starts_at`/`ends_at` (nunca un paciente, un
//! título real, ni ningún otro campo de `appointments`), y el texto del
//! evento (`"Sesión clínica"`) está fijo dentro de este módulo, no se
//! recibe como parámetro — es estructuralmente imposible que un llamador
//! le pase un texto distinto.

use serde::{Deserialize, Serialize};

use super::oauth::{GOOGLE_REVOKE_ENDPOINT, GOOGLE_TOKEN_ENDPOINT};

const CALENDAR_LIST_ENDPOINT: &str = "https://www.googleapis.com/calendar/v3/users/me/calendarList";
const EVENT_SUMMARY: &str = "Sesión clínica";

#[derive(Debug)]
pub enum GoogleApiError {
    Network(String),
    /// Respuesta de error del token endpoint indicando que el
    /// `refresh_token` ya no es válido (revocado por la usuaria desde su
    /// cuenta de Google, o expirado). Se distingue de `Network` porque el
    /// tratamiento es distinto: esto sí implica limpiar el token guardado y
    /// marcar la integración como desconectada; un error de red no.
    TokenRevoked,
    /// Cualquier otro error HTTP de la API (permisos, cuota, calendario
    /// inexistente, etc.), con el código de estado para diagnóstico.
    ApiError { status: u16, body: String },
}
impl std::fmt::Display for GoogleApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoogleApiError::Network(e) => write!(f, "error de red contactando a Google: {e}"),
            GoogleApiError::TokenRevoked => write!(f, "el acceso a Google Calendar fue revocado o expiró"),
            GoogleApiError::ApiError { status, body } => write!(f, "Google respondió {status}: {body}"),
        }
    }
}
impl std::error::Error for GoogleApiError {}

impl From<reqwest::Error> for GoogleApiError {
    fn from(e: reqwest::Error) -> Self {
        GoogleApiError::Network(e.to_string())
    }
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    /// Presente en el intercambio inicial; en una renovación, Google a
    /// veces (no siempre) entrega uno nuevo — cuando viene, reemplaza al
    /// guardado.
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: String,
}

fn is_revoked_error(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::BAD_REQUEST
        && serde_json::from_str::<TokenErrorResponse>(body)
            .map(|e| e.error == "invalid_grant")
            .unwrap_or(false)
}

pub async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, GoogleApiError> {
    let http = reqwest::Client::new();
    let response = http
        .post(GOOGLE_TOKEN_ENDPOINT)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await?;
    parse_token_response(response).await
}

pub async fn refresh_access_token(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<TokenResponse, GoogleApiError> {
    let http = reqwest::Client::new();
    let response = http
        .post(GOOGLE_TOKEN_ENDPOINT)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?;
    parse_token_response(response).await
}

async fn parse_token_response(response: reqwest::Response) -> Result<TokenResponse, GoogleApiError> {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status.is_success() {
        serde_json::from_str(&body).map_err(|e| GoogleApiError::ApiError { status: status.as_u16(), body: e.to_string() })
    } else if is_revoked_error(status, &body) {
        Err(GoogleApiError::TokenRevoked)
    } else {
        Err(GoogleApiError::ApiError { status: status.as_u16(), body })
    }
}

/// Revoca el token (access o refresh) contra Google. Se llama al
/// desconectar. Un fallo aquí no impide limpiar el keychain local — Google
/// puede tardar o estar inaccesible, pero la desconexión local siempre
/// debe completarse (ver `commands::calendar::disconnect_google_calendar`).
pub async fn revoke_token(token: &str) -> Result<(), GoogleApiError> {
    let http = reqwest::Client::new();
    let response = http.post(GOOGLE_REVOKE_ENDPOINT).form(&[("token", token)]).send().await?;
    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(GoogleApiError::ApiError { status: status.as_u16(), body })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCalendarListItem {
    pub id: String,
    pub summary: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Deserialize)]
struct CalendarListResponse {
    items: Vec<GoogleCalendarListItem>,
}

pub async fn list_calendars(access_token: &str) -> Result<Vec<GoogleCalendarListItem>, GoogleApiError> {
    let http = reqwest::Client::new();
    let response = http.get(CALENDAR_LIST_ENDPOINT).bearer_auth(access_token).send().await?;
    let response = check_status(response).await?;
    let parsed: CalendarListResponse = response.json().await?;
    Ok(parsed.items)
}

/// El único lugar donde se construye el cuerpo JSON que sale hacia Google
/// para representar una cita. Función pura y testeable sin red: recibe
/// exclusivamente horarios, nunca un `Appointment` completo — así es
/// estructuralmente imposible que termine incluyendo un campo clínico
/// aunque `Appointment` gane campos nuevos en el futuro.
fn event_payload(starts_at: &str, ends_at: &str) -> serde_json::Value {
    serde_json::json!({
        "summary": EVENT_SUMMARY,
        "start": { "dateTime": starts_at },
        "end": { "dateTime": ends_at },
    })
}

fn events_url(calendar_id: &str) -> String {
    format!("https://www.googleapis.com/calendar/v3/calendars/{}/events", url_path_encode(calendar_id))
}

fn event_url(calendar_id: &str, event_id: &str) -> String {
    format!(
        "https://www.googleapis.com/calendar/v3/calendars/{}/events/{}",
        url_path_encode(calendar_id),
        url_path_encode(event_id)
    )
}

fn url_path_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct EventResponse {
    id: String,
}

pub async fn create_event(access_token: &str, calendar_id: &str, starts_at: &str, ends_at: &str) -> Result<String, GoogleApiError> {
    let http = reqwest::Client::new();
    let response = http
        .post(events_url(calendar_id))
        .bearer_auth(access_token)
        .json(&event_payload(starts_at, ends_at))
        .send()
        .await?;
    let response = check_status(response).await?;
    let parsed: EventResponse = response.json().await?;
    Ok(parsed.id)
}

pub async fn update_event(
    access_token: &str,
    calendar_id: &str,
    event_id: &str,
    starts_at: &str,
    ends_at: &str,
) -> Result<(), GoogleApiError> {
    let http = reqwest::Client::new();
    let response = http
        .patch(event_url(calendar_id, event_id))
        .bearer_auth(access_token)
        .json(&event_payload(starts_at, ends_at))
        .send()
        .await?;
    check_status(response).await?;
    Ok(())
}

/// `Ok(())` tanto si el borrado tuvo éxito como si el evento ya no existía
/// en Google (404/410) — para nuestros efectos, ambos casos significan "ya
/// no hay evento espejo", que es el resultado que se buscaba.
pub async fn delete_event(access_token: &str, calendar_id: &str, event_id: &str) -> Result<(), GoogleApiError> {
    let http = reqwest::Client::new();
    let response = http.delete(event_url(calendar_id, event_id)).bearer_auth(access_token).send().await?;
    let status = response.status();
    if status.is_success() || status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        Ok(())
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(GoogleApiError::ApiError { status: status.as_u16(), body })
    }
}

async fn check_status(response: reqwest::Response) -> Result<reqwest::Response, GoogleApiError> {
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(GoogleApiError::TokenRevoked);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(GoogleApiError::ApiError { status: status.as_u16(), body });
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_payload_never_contains_anything_beyond_the_generic_summary_and_the_two_timestamps() {
        let payload = event_payload("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z");
        let obj = payload.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["end", "start", "summary"]);
        assert_eq!(payload["summary"], "Sesión clínica");
        assert_eq!(payload["start"]["dateTime"], "2026-09-01T15:00:00Z");
        assert_eq!(payload["end"]["dateTime"], "2026-09-01T16:00:00Z");
    }

    #[test]
    fn event_payload_is_identical_regardless_of_what_the_timestamps_look_like() {
        // No hay ninguna rama de código en `event_payload` que dependa de
        // nada más que start/end — este test documenta esa garantía
        // estructural: no existe ningún parámetro por el que un nombre de
        // paciente, modalidad o motivo de consulta pudiera colarse.
        let payload = event_payload("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z");
        assert_eq!(payload.as_object().unwrap().len(), 3);
    }

    #[test]
    fn calendar_url_percent_encodes_special_characters() {
        assert_eq!(
            events_url("mi.correo+tag@gmail.com"),
            "https://www.googleapis.com/calendar/v3/calendars/mi.correo%2Btag%40gmail.com/events"
        );
    }
}
