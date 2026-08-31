//! Flujo OAuth 2.0 Authorization Code + PKCE para el cliente tipo "Desktop
//! app" de Google, siguiendo RFC 8252 (apps nativas/instaladas). Ver
//! `docs/google-calendar.md` para la justificación completa de cada
//! decisión de este archivo.
//!
//! Separación deliberada: las funciones puras (generación de
//! `code_verifier`/`state`, cálculo del `code_challenge`, construcción de
//! la URL de autorización, parseo del callback) no tocan la red ni el
//! sistema de archivos — se pueden probar sin ningún backend externo. Solo
//! `wait_for_redirect` toca un socket real (el listener de loopback).

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::time::Duration;

use base64ct::{Base64UrlUnpadded, Encoding};
use sha2::{Digest, Sha256};

/// RFC 7636 exige 43-128 caracteres de salida base64url para el
/// `code_verifier`. 32 bytes aleatorios codifican a 43 caracteres exactos
/// (el mínimo permitido, y suficiente entropía: 256 bits).
const VERIFIER_RANDOM_BYTES: usize = 32;
const STATE_RANDOM_BYTES: usize = 32;

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    // No se reutiliza `security::random` a propósito: ese módulo es privado
    // a `security::` y este es un dominio de secretos completamente
    // separado (credenciales de Google, no el vault clínico) — llamar a
    // `getrandom` directamente aquí evita tocar el módulo de seguridad ya
    // aprobado. Es la misma fuente de aleatoriedad del sistema operativo
    // que usa `security::random` internamente.
    getrandom::fill(&mut buf).expect("el generador de aleatoriedad del sistema operativo debe estar disponible");
    buf
}

/// Cadena aleatoria apta para usarse como `code_verifier` PKCE o como
/// `state` — ambas son, en esencia, el mismo tipo de valor: alta entropía,
/// solo caracteres base64url. Se genera nueva en cada intento de
/// autorización y vive únicamente en memoria (nunca se persiste).
pub fn generate_verifier() -> String {
    Base64UrlUnpadded::encode_string(&random_bytes::<VERIFIER_RANDOM_BYTES>())
}

pub fn generate_state() -> String {
    Base64UrlUnpadded::encode_string(&random_bytes::<STATE_RANDOM_BYTES>())
}

/// `code_challenge = BASE64URL(SHA256(code_verifier))`, exactamente la
/// fórmula de RFC 7636 §4.2, método `S256`.
pub fn code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    Base64UrlUnpadded::encode_string(&digest)
}

pub const GOOGLE_AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_REVOKE_ENDPOINT: &str = "https://oauth2.googleapis.com/revoke";

/// Scopes mínimos aprobados — ver `docs/google-calendar.md`. Nunca
/// `calendar` (acceso completo) ni ningún scope de configuración/ACL.
pub const SCOPES: &str = "https://www.googleapis.com/auth/calendar.calendarlist.readonly https://www.googleapis.com/auth/calendar.events";

fn url_encode(value: &str) -> String {
    // Suficiente para los valores que este módulo necesita codificar
    // (scopes con espacios, code_challenge/state base64url, redirect_uri
    // con solo dígitos/puntos/dos puntos) sin agregar una dependencia de
    // codificación de URLs completa.
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Construye la URL de consentimiento de Google. `access_type=offline` +
/// `prompt=consent` garantizan que Google entregue un `refresh_token`
/// incluso si la usuaria ya había autorizado la app antes (Google solo lo
/// entrega la primera vez a menos que se fuerce `prompt=consent`).
pub fn build_auth_url(client_id: &str, redirect_uri: &str, code_challenge: &str, state: &str) -> String {
    format!(
        "{GOOGLE_AUTH_ENDPOINT}?client_id={}&redirect_uri={}&response_type=code&scope={}\
         &code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        url_encode(client_id),
        url_encode(redirect_uri),
        url_encode(SCOPES),
        url_encode(code_challenge),
        url_encode(state),
    )
}

#[derive(Debug)]
pub enum OAuthError {
    /// El listener de loopback no pudo levantarse.
    ListenerFailed(String),
    /// Se agotó el tiempo de espera del callback (la usuaria no completó
    /// el consentimiento, o cerró el navegador).
    Timeout,
    /// El callback llegó pero sin los parámetros esperados, o con un error
    /// reportado por Google (p. ej. la usuaria negó el consentimiento).
    CallbackError(String),
    /// El `state` recibido no coincide con el generado para este intento —
    /// se aborta sin intercambiar nada. Es la protección contra CSRF.
    StateMismatch,
}
impl fmt::Display for OAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OAuthError::ListenerFailed(e) => write!(f, "no se pudo levantar el listener local: {e}"),
            OAuthError::Timeout => write!(f, "tiempo de espera agotado esperando la autorización"),
            OAuthError::CallbackError(e) => write!(f, "la autorización de Google falló: {e}"),
            OAuthError::StateMismatch => write!(f, "el parámetro state no coincide — posible intento de CSRF, se abortó"),
        }
    }
}
impl std::error::Error for OAuthError {}

/// Levanta un listener en `127.0.0.1` con puerto efímero (asignado por el
/// sistema operativo) — nunca `0.0.0.0`, nunca el hostname ambiguo
/// `localhost`. Devuelve el listener ya enlazado y el puerto real, para
/// construir el `redirect_uri` exacto antes de abrir el navegador.
pub fn bind_loopback_listener() -> Result<(TcpListener, u16), OAuthError> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .map_err(|e| OAuthError::ListenerFailed(e.to_string()))?;
    let port = listener.local_addr().map_err(|e| OAuthError::ListenerFailed(e.to_string()))?.port();
    Ok((listener, port))
}

/// Extrae `code`, `state` y (si Google reportó un error) `error` de la
/// primera línea de una petición HTTP GET cruda, del estilo
/// `GET /?code=...&state=...&error=... HTTP/1.1`. No usa ninguna librería
/// de parseo HTTP — es intencionalmente mínimo, porque solo necesita leer
/// exactamente una petición controlada por el propio flujo que la
/// desencadenó.
fn parse_callback_query(request_line: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let path = request_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;

    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let decoded = percent_decode(value);
        match key {
            "code" => code = Some(decoded),
            "state" => state = Some(decoded),
            "error" => error = Some(decoded),
            _ => {}
        }
    }
    Some((code, state, error))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const CALLBACK_PAGE: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
    <html><body style=\"font-family: sans-serif; padding: 2rem;\">\
    Puedes cerrar esta pestaña y volver a Cuaderno Clínico.</body></html>";

/// Bloqueante a propósito — se invoca desde el comando Tauri vía
/// `spawn_blocking` para no bloquear el runtime async. Acepta exactamente
/// una conexión, valida `state` **antes** de devolver el `code`, y cierra
/// el listener al salir (se suelta con la función).
pub fn wait_for_redirect(listener: TcpListener, expected_state: &str, timeout: Duration) -> Result<String, OAuthError> {
    // `accept()` no tiene timeout nativo en `std`, así que el listener se
    // pone en modo no bloqueante y se sondea con una espera corta — es lo
    // que permite abandonar el intento si la usuaria nunca completa el
    // consentimiento en el navegador, sin dejar un hilo bloqueado para
    // siempre.
    listener.set_nonblocking(true).map_err(|e| OAuthError::ListenerFailed(e.to_string()))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => {
                stream.set_nonblocking(false).map_err(|e| OAuthError::ListenerFailed(e.to_string()))?;
                if let Some(result) = handle_one_connection(stream, expected_state) {
                    return result;
                }
                // Petición que no era el callback esperado (p. ej. el
                // navegador pidiendo /favicon.ico) — se ignora y se sigue
                // esperando, siempre que no se haya vencido el plazo.
                if std::time::Instant::now() >= deadline {
                    return Err(OAuthError::Timeout);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(OAuthError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(OAuthError::ListenerFailed(e.to_string())),
        }
    }
}

fn handle_one_connection(mut stream: TcpStream, expected_state: &str) -> Option<Result<String, OAuthError>> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;

    let (code, state, error) = parse_callback_query(&request_line)?;
    let _ = stream.write_all(CALLBACK_PAGE.as_bytes());
    let _ = stream.flush();

    if let Some(err) = error {
        return Some(Err(OAuthError::CallbackError(err)));
    }
    let code = code?;
    let state = state?;
    if state != expected_state {
        return Some(Err(OAuthError::StateMismatch));
    }
    Some(Ok(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_and_state_are_high_entropy_and_url_safe() {
        let v = generate_verifier();
        assert!(v.len() >= 43, "RFC 7636 exige al menos 43 caracteres");
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));

        let s = generate_state();
        assert_ne!(v, s);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn two_calls_never_produce_the_same_verifier() {
        assert_ne!(generate_verifier(), generate_verifier());
        assert_ne!(generate_state(), generate_state());
    }

    #[test]
    fn code_challenge_matches_a_known_rfc7636_test_vector() {
        // Vector de ejemplo del propio RFC 7636, Apéndice B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let expected_challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(code_challenge(verifier), expected_challenge);
    }

    #[test]
    fn auth_url_includes_pkce_and_state_and_minimal_scopes() {
        let url = build_auth_url("client-123", "http://127.0.0.1:54321", "challenge-abc", "state-xyz");
        assert!(url.starts_with(GOOGLE_AUTH_ENDPOINT));
        assert!(url.contains("code_challenge=challenge-abc"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=state-xyz"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("calendar.events"));
        assert!(url.contains("calendar.calendarlist.readonly"));
        // Nunca el scope amplio.
        assert!(!url.contains("auth/calendar&") && !url.contains("auth/calendar%20"));
    }

    #[test]
    fn parses_a_successful_callback_request_line() {
        let (code, state, error) = parse_callback_query("GET /?code=4/abc123&state=xyz789 HTTP/1.1").unwrap();
        assert_eq!(code.as_deref(), Some("4/abc123"));
        assert_eq!(state.as_deref(), Some("xyz789"));
        assert!(error.is_none());
    }

    #[test]
    fn parses_a_denied_consent_callback() {
        let (code, _state, error) = parse_callback_query("GET /?error=access_denied&state=xyz789 HTTP/1.1").unwrap();
        assert!(code.is_none());
        assert_eq!(error.as_deref(), Some("access_denied"));
    }

    #[test]
    fn decodes_percent_encoded_values() {
        let (code, _state, _error) = parse_callback_query("GET /?code=4%2Fabc%3D%3D&state=s HTTP/1.1").unwrap();
        assert_eq!(code.as_deref(), Some("4/abc=="));
    }

    #[test]
    fn ignores_requests_without_a_query_string() {
        assert!(parse_callback_query("GET /favicon.ico HTTP/1.1").is_none());
    }

    #[test]
    fn wait_for_redirect_rejects_a_mismatched_state() {
        let (listener, port) = bind_loopback_listener().unwrap();
        let handle = std::thread::spawn(move || wait_for_redirect(listener, "expected-state", Duration::from_secs(5)));

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(b"GET /?code=abc&state=wrong-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();

        let result = handle.join().unwrap();
        assert!(matches!(result, Err(OAuthError::StateMismatch)));
    }

    #[test]
    fn wait_for_redirect_accepts_a_matching_state_and_returns_the_code() {
        let (listener, port) = bind_loopback_listener().unwrap();
        let handle = std::thread::spawn(move || wait_for_redirect(listener, "correct-state", Duration::from_secs(5)));

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(b"GET /?code=the-code&state=correct-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();

        let result = handle.join().unwrap();
        assert_eq!(result.unwrap(), "the-code");
    }

    #[test]
    fn wait_for_redirect_times_out_when_nothing_connects() {
        let (listener, _port) = bind_loopback_listener().unwrap();
        let result = wait_for_redirect(listener, "state", Duration::from_millis(100));
        assert!(matches!(result, Err(OAuthError::Timeout)));
    }

    #[test]
    fn loopback_listener_binds_only_to_127_0_0_1() {
        let (listener, _port) = bind_loopback_listener().unwrap();
        assert_eq!(listener.local_addr().unwrap().ip(), std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
}
