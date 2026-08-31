//! Almacenamiento de credenciales de Google en el keychain del sistema
//! operativo (crate `keyring`) — nunca en SQLite, nunca en
//! `vault.meta.json`, nunca en un archivo plano.
//!
//! Solo se guarda el `refresh_token`. Deliberadamente **no** se cachea el
//! `access_token`: es de vida corta (~1 hora) y el costo de pedir uno nuevo
//! antes de cada llamada a la API de Google es bajo para el volumen de uso
//! esperado (una psicóloga, sincronización ocasional de citas) — así se
//! evita tener que rastrear expiración en ningún lado.
//!
//! El Client ID/Client Secret de la app de escritorio **no** pasan por
//! aquí — no son credenciales confidenciales para este tipo de cliente OAuth
//! (ver `docs/google-calendar.md`), así que viven en `app_settings` (dentro
//! del vault SQLCipher, ya cifrado en reposo), gestionados por
//! `commands::calendar`, no por este módulo.

use std::fmt;

use keyring::Entry;
use zeroize::Zeroize;

const SERVICE: &str = "com.jpcaamano.cuadernoclinico.google_calendar";
const ACCOUNT: &str = "refresh_token";

/// El refresh token, envuelto para que nunca se imprima por accidente y se
/// zeroice al soltarse — mismo patrón que `db::VaultKey` /
/// `security::kdf::Kek`.
pub struct RefreshToken(String);

impl RefreshToken {
    pub fn new(value: String) -> Self {
        Self(value)
    }
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RefreshToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshToken").field("value", &"<redacted>").finish()
    }
}

impl Drop for RefreshToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug)]
pub enum TokenStoreError {
    /// El backend de keychain del sistema operativo no está disponible o
    /// falló (p. ej. sin daemon de Secret Service en Linux). No es un error
    /// de credenciales — es una limitación del entorno, y se reporta como
    /// tal en vez de degradar a un almacenamiento inseguro.
    BackendUnavailable(String),
}
impl fmt::Display for TokenStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenStoreError::BackendUnavailable(msg) => {
                write!(f, "no se pudo acceder al almacén de credenciales del sistema operativo: {msg}")
            }
        }
    }
}
impl std::error::Error for TokenStoreError {}

fn entry() -> Result<Entry, TokenStoreError> {
    Entry::new(SERVICE, ACCOUNT).map_err(|e| TokenStoreError::BackendUnavailable(e.to_string()))
}

pub fn save(token: &RefreshToken) -> Result<(), TokenStoreError> {
    entry()?
        .set_password(token.expose_secret())
        .map_err(|e| TokenStoreError::BackendUnavailable(e.to_string()))
}

/// `Ok(None)` si nunca se guardó nada (no conectado) o si el sistema
/// operativo indica explícitamente "no existe" — se distingue de un fallo
/// real del backend, que sí se propaga como error.
pub fn load() -> Result<Option<RefreshToken>, TokenStoreError> {
    match entry()?.get_password() {
        Ok(value) => Ok(Some(RefreshToken::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(TokenStoreError::BackendUnavailable(e.to_string())),
    }
}

/// Borra el refresh token del keychain. Se usa al desconectar y cuando
/// Google rechaza explícitamente el refresh token (revocado/expirado).
pub fn clear() -> Result<(), TokenStoreError> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(TokenStoreError::BackendUnavailable(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_the_token_value() {
        let t = RefreshToken::new("secreto-de-prueba-no-real".to_string());
        let debug_output = format!("{t:?}");
        assert!(!debug_output.contains("secreto-de-prueba-no-real"));
        assert_eq!(debug_output, "RefreshToken { value: \"<redacted>\" }");
    }

    // No hay un test de round-trip contra el keychain real aquí a
    // propósito: requiere un backend de Secret Service/Keychain/Credential
    // Manager real disponible, que no existe en un entorno de CI headless.
    // La verificación manual de esta fase documenta explícitamente el
    // resultado de probarlo contra el entorno real disponible.
}
