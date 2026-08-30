//! Derivación de la KEK (Key Encryption Key) con Argon2id.
//!
//! La contraseña maestra (o el código de recuperación) nunca se usa
//! directamente como clave: siempre se pasa por Argon2id junto con una sal
//! aleatoria de 16 bytes, produciendo una KEK de 32 bytes que a su vez
//! envuelve el DEK (ver `envelope.rs`). Esto es justamente lo que evita usar
//! el KDF interno (más débil, PBKDF2) que SQLCipher aplicaría si se le
//! pasara la contraseña directamente — ver `docs/ARCHITECTURE.md` sección 5
//! y `docs/sqlcipher.md`.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::random;

pub const SALT_LEN: usize = 16;
pub const KEK_LEN: usize = 32;

/// Parámetros de Argon2id recomendados por RFC 9106 (§4, "second recommended
/// option", pensado para entornos con restricciones de memoria — aun así muy
/// por encima de lo que necesita un ataque de fuerza bruta interactivo desde
/// un equipo de escritorio). Se aplican igual a la KEK de contraseña y a la
/// de código de recuperación.
///
/// Con esto, derivar una KEK toma del orden de cientos de milisegundos en
/// hardware de escritorio típico — perceptible pero aceptable para un
/// desbloqueo que ocurre pocas veces por día, y deliberadamente costoso para
/// quien intente probar contraseñas por fuerza bruta contra una copia
/// robada del vault.
pub const ARGON2ID_M_COST_KIB: u32 = 65536; // 64 MiB
pub const ARGON2ID_T_COST: u32 = 3;
pub const ARGON2ID_P_COST: u32 = 4;

/// Parámetros de Argon2id usados para una derivación concreta, tal como se
/// guardan (en claro — no son secretos) en `vault.meta.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    pub fn recommended() -> Self {
        Self {
            m_cost_kib: ARGON2ID_M_COST_KIB,
            t_cost: ARGON2ID_T_COST,
            p_cost: ARGON2ID_P_COST,
        }
    }

    fn to_argon2_params(&self) -> Result<Params, KdfError> {
        Params::new(self.m_cost_kib, self.t_cost, self.p_cost, Some(KEK_LEN))
            .map_err(|_| KdfError::InvalidParams)
    }
}

#[derive(Debug)]
pub enum KdfError {
    InvalidParams,
    /// Fallo interno de Argon2id (p. ej. entrada demasiado larga). No debería
    /// ocurrir con nuestros parámetros fijos.
    HashingFailed,
    Random(getrandom::Error),
}

impl fmt::Display for KdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KdfError::InvalidParams => write!(f, "parámetros de Argon2id inválidos"),
            KdfError::HashingFailed => write!(f, "no se pudo derivar la clave con Argon2id"),
            KdfError::Random(_) => write!(f, "no se pudo generar una sal aleatoria"),
        }
    }
}
impl std::error::Error for KdfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KdfError::Random(e) => Some(e),
            _ => None,
        }
    }
}
impl From<getrandom::Error> for KdfError {
    fn from(e: getrandom::Error) -> Self {
        KdfError::Random(e)
    }
}

/// KEK derivada: 32 bytes que nunca se guardan en disco, se imprimen en
/// logs, ni sobreviven más tiempo del necesario en memoria. Igual que
/// `db::VaultKey`, redacta su `Debug` y se zeroiza al soltarse.
pub struct Kek([u8; KEK_LEN]);

impl Kek {
    pub(crate) fn expose_secret(&self) -> &[u8; KEK_LEN] {
        &self.0
    }
}

impl fmt::Debug for Kek {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kek").field("bytes", &"<redacted>").finish()
    }
}

impl Drop for Kek {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Sal aleatoria de 16 bytes para una derivación de Argon2id.
#[derive(Clone)]
pub struct Salt([u8; SALT_LEN]);

impl Salt {
    pub fn generate() -> Result<Self, KdfError> {
        Ok(Self(random::bytes::<SALT_LEN>()?))
    }

    pub fn to_base64(&self) -> String {
        super::b64::encode(&self.0)
    }

    pub fn from_base64(s: &str) -> Result<Self, super::b64::DecodeError> {
        let bytes = super::b64::decode(s)?;
        let arr: [u8; SALT_LEN] = bytes
            .try_into()
            .map_err(|_| super::b64::DecodeError::WrongLength)?;
        Ok(Self(arr))
    }
}

/// Deriva una KEK a partir de un secreto (contraseña maestra, o los bytes
/// decodificados del código de recuperación) y una sal, usando Argon2id con
/// los parámetros indicados. Determinístico: el mismo secreto + sal +
/// parámetros siempre produce la misma KEK — es lo que permite volver a
/// desenvolver el DEK en cada desbloqueo sin guardar la KEK en ningún lado.
pub fn derive_kek(secret: &[u8], salt: &Salt, params: &KdfParams) -> Result<Kek, KdfError> {
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params.to_argon2_params()?);
    let mut out = [0u8; KEK_LEN];
    argon2
        .hash_password_into(secret, &salt.0, &mut out)
        .map_err(|_| KdfError::HashingFailed)?;
    Ok(Kek(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_secret_and_salt_produce_the_same_kek() {
        let salt = Salt::generate().unwrap();
        let params = KdfParams::recommended();
        let a = derive_kek(b"clave de prueba", &salt, &params).unwrap();
        let b = derive_kek(b"clave de prueba", &salt, &params).unwrap();
        assert_eq!(a.expose_secret(), b.expose_secret());
    }

    #[test]
    fn different_secrets_produce_different_keks() {
        let salt = Salt::generate().unwrap();
        let params = KdfParams::recommended();
        let a = derive_kek(b"clave-A", &salt, &params).unwrap();
        let b = derive_kek(b"clave-B", &salt, &params).unwrap();
        assert_ne!(a.expose_secret(), b.expose_secret());
    }

    #[test]
    fn different_salts_produce_different_keks_for_the_same_secret() {
        let params = KdfParams::recommended();
        let a = derive_kek(b"misma-clave", &Salt::generate().unwrap(), &params).unwrap();
        let b = derive_kek(b"misma-clave", &Salt::generate().unwrap(), &params).unwrap();
        assert_ne!(a.expose_secret(), b.expose_secret());
    }

    #[test]
    fn debug_never_prints_the_kek_bytes() {
        let salt = Salt::generate().unwrap();
        let kek = derive_kek(b"x", &salt, &KdfParams::recommended()).unwrap();
        assert_eq!(format!("{kek:?}"), "Kek { bytes: \"<redacted>\" }");
    }

    #[test]
    fn salt_roundtrips_through_base64() {
        let salt = Salt::generate().unwrap();
        let encoded = salt.to_base64();
        let decoded = Salt::from_base64(&encoded).unwrap();
        assert_eq!(salt.0, decoded.0);
    }
}
