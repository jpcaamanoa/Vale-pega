//! Envoltura mínima sobre `base64ct` para guardar bytes binarios (sales,
//! nonces, texto cifrado) como texto dentro de `vault.meta.json`. No es
//! cifrado ni ofrece ninguna propiedad de seguridad — es solo una
//! representación de texto, igual que lo sería hexadecimal.

use base64ct::{Base64, Encoding};
use std::fmt;

#[derive(Debug)]
pub enum DecodeError {
    InvalidBase64,
    WrongLength,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::InvalidBase64 => write!(f, "base64 inválido"),
            DecodeError::WrongLength => write!(f, "longitud decodificada inesperada"),
        }
    }
}
impl std::error::Error for DecodeError {}

pub fn encode(bytes: &[u8]) -> String {
    Base64::encode_string(bytes)
}

pub fn decode(s: &str) -> Result<Vec<u8>, DecodeError> {
    Base64::decode_vec(s).map_err(|_| DecodeError::InvalidBase64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_arbitrary_bytes() {
        let data = [0u8, 1, 2, 253, 254, 255, 42, 7];
        let encoded = encode(&data);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn rejects_invalid_base64() {
        assert!(decode("no es base64 válido!!").is_err());
    }
}
