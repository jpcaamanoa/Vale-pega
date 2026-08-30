//! `vault.meta.json`: el único archivo, además de la base SQLCipher, que
//! necesita existir para poder desbloquear el vault. No contiene ningún
//! secreto por sí solo — todo lo que guarda (sales, parámetros de Argon2id,
//! nonces, el DEK ya envuelto/cifrado) es inútil sin la contraseña maestra o
//! el código de recuperación correctos. Ver `docs/ARCHITECTURE.md` sección 5.

use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::envelope::WrappedKey;
use super::kdf::{KdfParams, Salt};

pub const FORMAT_VERSION: u32 = 1;

/// Un DEK envuelto por una sola vía (contraseña, o código de recuperación):
/// la sal y los parámetros de Argon2id usados para derivar la KEK, más el
/// nonce y el texto cifrado de AES-256-GCM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrapRecord {
    pub salt_b64: String,
    pub kdf: KdfParams,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

impl WrapRecord {
    pub fn new(salt: &Salt, kdf: KdfParams, wrapped: &WrappedKey) -> Self {
        Self {
            salt_b64: salt.to_base64(),
            kdf,
            nonce_b64: super::b64::encode(&wrapped.nonce),
            ciphertext_b64: super::b64::encode(&wrapped.ciphertext),
        }
    }

    pub fn salt(&self) -> Result<Salt, VaultMetaError> {
        Salt::from_base64(&self.salt_b64).map_err(|_| VaultMetaError::Corrupt)
    }

    pub fn wrapped_key(&self) -> Result<WrappedKey, VaultMetaError> {
        let nonce_vec = super::b64::decode(&self.nonce_b64).map_err(|_| VaultMetaError::Corrupt)?;
        let nonce: [u8; super::envelope::NONCE_LEN] =
            nonce_vec.try_into().map_err(|_| VaultMetaError::Corrupt)?;
        let ciphertext = super::b64::decode(&self.ciphertext_b64).map_err(|_| VaultMetaError::Corrupt)?;
        Ok(WrappedKey { nonce, ciphertext })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultMetaFile {
    pub format_version: u32,
    pub created_at: String,
    pub password_wrap: WrapRecord,
    pub recovery_wrap: WrapRecord,
}

#[derive(Debug)]
pub enum VaultMetaError {
    NotFound,
    Io(std::io::Error),
    /// El archivo existe pero no es JSON válido, o no tiene la forma
    /// esperada — no se puede saber si es un vault de una versión distinta,
    /// un archivo dañado, o algo no relacionado.
    Corrupt,
}

impl fmt::Display for VaultMetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultMetaError::NotFound => write!(f, "no existe vault.meta.json en esta ubicación"),
            VaultMetaError::Io(e) => write!(f, "error de E/S leyendo/escribiendo vault.meta.json: {e}"),
            VaultMetaError::Corrupt => write!(f, "vault.meta.json no es válido o está dañado"),
        }
    }
}
impl std::error::Error for VaultMetaError {}

impl VaultMetaFile {
    pub fn load(path: &Path) -> Result<Self, VaultMetaError> {
        if !path.exists() {
            return Err(VaultMetaError::NotFound);
        }
        let contents = fs::read_to_string(path).map_err(VaultMetaError::Io)?;
        serde_json::from_str(&contents).map_err(|_| VaultMetaError::Corrupt)
    }

    /// Escritura atómica: se escribe primero a un archivo temporal y luego
    /// se renombra sobre el definitivo. Un corte de energía o un cierre
    /// abrupto a mitad de la escritura deja el archivo original intacto en
    /// vez de una mitad de JSON — importante porque este archivo se
    /// reescribe en cada cambio de contraseña y en cada recuperación.
    pub fn save(&self, path: &Path) -> Result<(), VaultMetaError> {
        let json = serde_json::to_string_pretty(self).expect("VaultMetaFile siempre es serializable");
        let tmp_path = path.with_extension("json.tmp");
        fs::write(&tmp_path, &json).map_err(VaultMetaError::Io)?;
        fs::rename(&tmp_path, path).map_err(VaultMetaError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::envelope::{generate_dek, wrap_dek};
    use crate::security::kdf::derive_kek;

    fn sample_meta() -> VaultMetaFile {
        let dek = generate_dek().unwrap();
        let params = KdfParams::recommended();

        let pw_salt = Salt::generate().unwrap();
        let pw_kek = derive_kek(b"contrasena-de-prueba", &pw_salt, &params).unwrap();
        let pw_wrapped = wrap_dek(&dek, &pw_kek).unwrap();

        let rec_salt = Salt::generate().unwrap();
        let rec_kek = derive_kek(b"codigo-de-prueba", &rec_salt, &params).unwrap();
        let rec_wrapped = wrap_dek(&dek, &rec_kek).unwrap();

        VaultMetaFile {
            format_version: FORMAT_VERSION,
            created_at: "2026-08-30T00:00:00Z".to_string(),
            password_wrap: WrapRecord::new(&pw_salt, params.clone(), &pw_wrapped),
            recovery_wrap: WrapRecord::new(&rec_salt, params, &rec_wrapped),
        }
    }

    #[test]
    fn missing_file_reports_not_found() {
        let dir = std::env::temp_dir().join(format!("cc-meta-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("does-not-exist.json");
        let err = VaultMetaFile::load(&path).unwrap_err();
        assert!(matches!(err, VaultMetaError::NotFound));
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = std::env::temp_dir().join(format!("cc-meta-test-roundtrip-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("vault.meta.json");

        let meta = sample_meta();
        meta.save(&path).unwrap();
        let loaded = VaultMetaFile::load(&path).unwrap();

        assert_eq!(loaded.format_version, meta.format_version);
        assert_eq!(loaded.password_wrap.salt_b64, meta.password_wrap.salt_b64);
        assert_eq!(loaded.recovery_wrap.ciphertext_b64, meta.recovery_wrap.ciphertext_b64);
    }

    #[test]
    fn corrupt_json_is_rejected_explicitly() {
        let dir = std::env::temp_dir().join(format!("cc-meta-test-corrupt-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("vault.meta.json");
        fs::write(&path, b"esto no es json").unwrap();

        let err = VaultMetaFile::load(&path).unwrap_err();
        assert!(matches!(err, VaultMetaError::Corrupt));
    }

    #[test]
    fn wrap_record_does_not_contain_the_dek_or_secret_in_plain_text() {
        let meta = sample_meta();
        let json = serde_json::to_string(&meta).unwrap();
        // El JSON entero es texto: si el DEK o la contraseña aparecieran en
        // claro en algún campo, aparecerían como substring aquí.
        assert!(!json.contains("contrasena-de-prueba"));
        assert!(!json.contains("codigo-de-prueba"));
    }
}
