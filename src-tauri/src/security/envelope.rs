//! Cifrado por sobres (envelope encryption) del DEK con AES-256-GCM.
//!
//! El DEK (`db::VaultKey`, 32 bytes aleatorios que cifran realmente la base
//! SQLCipher) nunca se guarda en disco directamente. Se guarda "envuelto":
//! cifrado con una KEK (derivada de la contraseña, o del código de
//! recuperación, vía Argon2id — ver `kdf.rs`) usando AES-256-GCM, una cifra
//! autenticada de una biblioteca consolidada (`aes-gcm`, RustCrypto). El
//! resultado (nonce + texto cifrado con su tag de autenticación) es lo único
//! que se persiste en `vault.meta.json`.
//!
//! Esto da la verificación de "¿esta KEK es la correcta?" gratis: si la KEK
//! es incorrecta (contraseña equivocada), la autenticación de GCM falla y
//! `unwrap_dek` devuelve un error — nunca hace falta guardar un hash de
//! verificación aparte de la propia contraseña.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use std::fmt;

use crate::db::{VaultKey, VaultKeyError, VAULT_KEY_LEN};

use super::kdf::Kek;
use super::random;

pub const NONCE_LEN: usize = 12;

/// DEK envuelto: lo único que se persiste en `vault.meta.json` para una vía
/// de desbloqueo (contraseña o código de recuperación). Ninguno de estos dos
/// campos es secreto por sí solo sin la KEK correspondiente.
#[derive(Debug, Clone)]
pub struct WrappedKey {
    pub nonce: [u8; NONCE_LEN],
    pub ciphertext: Vec<u8>,
}

#[derive(Debug)]
pub enum EnvelopeError {
    Random(getrandom::Error),
    /// Fallo de cifrado (no debería ocurrir con entradas válidas de tamaño fijo).
    EncryptionFailed,
    /// La autenticación falló al desenvolver: KEK incorrecta (contraseña o
    /// código de recuperación equivocados) o el registro envuelto está
    /// dañado/manipulado. Indistinguibles por diseño — AES-GCM autentica
    /// todo el mensaje, no revela "cuál" de los dos ocurrió.
    UnwrapFailed,
    InvalidDekLength(VaultKeyError),
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnvelopeError::Random(_) => write!(f, "no se pudo generar un nonce aleatorio"),
            EnvelopeError::EncryptionFailed => write!(f, "no se pudo envolver el DEK"),
            EnvelopeError::UnwrapFailed => {
                write!(f, "clave incorrecta o registro cifrado dañado")
            }
            EnvelopeError::InvalidDekLength(e) => write!(f, "DEK desenvuelto inválido: {e}"),
        }
    }
}
impl std::error::Error for EnvelopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EnvelopeError::Random(e) => Some(e),
            _ => None,
        }
    }
}
impl From<getrandom::Error> for EnvelopeError {
    fn from(e: getrandom::Error) -> Self {
        EnvelopeError::Random(e)
    }
}

/// Envuelve (cifra) `dek` con `kek`. Genera un nonce aleatorio nuevo en cada
/// llamada — nunca se reutiliza un nonce con la misma KEK.
pub fn wrap_dek(dek: &VaultKey, kek: &Kek) -> Result<WrappedKey, EnvelopeError> {
    let nonce_bytes = random::bytes::<NONCE_LEN>()?;
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*kek.expose_secret()));
    let nonce = Nonce::from(nonce_bytes);
    let nonce = &nonce;
    let ciphertext = cipher
        .encrypt(nonce, dek.expose_secret().as_slice())
        .map_err(|_| EnvelopeError::EncryptionFailed)?;
    Ok(WrappedKey {
        nonce: nonce_bytes,
        ciphertext,
    })
}

/// Desenvuelve (descifra) un `WrappedKey` con `kek`, devolviendo el DEK
/// original si y solo si `kek` es la que se usó para envolverlo.
pub fn unwrap_dek(wrapped: &WrappedKey, kek: &Kek) -> Result<VaultKey, EnvelopeError> {
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*kek.expose_secret()));
    let nonce = Nonce::from(wrapped.nonce);
    let nonce = &nonce;
    let plaintext = cipher
        .decrypt(nonce, wrapped.ciphertext.as_slice())
        .map_err(|_| EnvelopeError::UnwrapFailed)?;
    let dek = VaultKey::from_slice(&plaintext).map_err(EnvelopeError::InvalidDekLength);
    // El texto plano intermedio contiene el DEK: no dejarlo en memoria más
    // de lo necesario, aunque `plaintext` vaya a salir de scope de inmediato.
    let mut plaintext = plaintext;
    use zeroize::Zeroize;
    plaintext.zeroize();
    dek
}

pub fn generate_dek() -> Result<VaultKey, EnvelopeError> {
    let bytes = random::bytes::<VAULT_KEY_LEN>()?;
    Ok(VaultKey::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::kdf::{derive_kek, KdfParams, Salt};

    fn test_kek(secret: &[u8]) -> Kek {
        let salt = Salt::generate().unwrap();
        derive_kek(secret, &salt, &KdfParams::recommended()).unwrap()
    }

    #[test]
    fn wrapping_and_unwrapping_with_the_same_kek_recovers_the_dek() {
        let dek = generate_dek().unwrap();
        let kek = test_kek(b"correcta");

        let wrapped = wrap_dek(&dek, &kek).unwrap();
        let recovered = unwrap_dek(&wrapped, &kek).unwrap();

        assert_eq!(dek.expose_secret(), recovered.expose_secret());
    }

    #[test]
    fn unwrapping_with_a_different_kek_fails() {
        let dek = generate_dek().unwrap();
        let wrapped = wrap_dek(&dek, &test_kek(b"correcta")).unwrap();

        let err = unwrap_dek(&wrapped, &test_kek(b"incorrecta")).unwrap_err();
        assert!(matches!(err, EnvelopeError::UnwrapFailed));
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let dek = generate_dek().unwrap();
        let kek = test_kek(b"correcta");
        let mut wrapped = wrap_dek(&dek, &kek).unwrap();
        wrapped.ciphertext[0] ^= 0xFF;

        let err = unwrap_dek(&wrapped, &kek).unwrap_err();
        assert!(matches!(err, EnvelopeError::UnwrapFailed));
    }

    #[test]
    fn two_wraps_of_the_same_dek_use_different_nonces() {
        let dek = generate_dek().unwrap();
        let kek = test_kek(b"correcta");
        let a = wrap_dek(&dek, &kek).unwrap();
        let b = wrap_dek(&dek, &kek).unwrap();
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ciphertext, b.ciphertext);
    }
}
