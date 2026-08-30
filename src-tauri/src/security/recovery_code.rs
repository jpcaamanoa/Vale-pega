//! Código de recuperación de alta entropía.
//!
//! 15 bytes (120 bits) del generador de aleatoriedad del sistema operativo
//! (`random::bytes`, la misma fuente que el DEK — nunca un RNG no
//! criptográfico), codificados en Base32 de Crockford (32 símbolos, excluye
//! I/L/O/U para reducir errores de transcripción) y agrupados en el formato
//! `XXXX-XXXX-XXXX-XXXX-XXXX-XXXX` que pidió la usuaria.
//!
//! El código en sí **nunca se guarda**, ni siquiera cifrado, ni como hash de
//! verificación separado. Su única función es servir de entrada a Argon2id
//! para derivar la KEK de recuperación (`kdf::derive_kek`), que a su vez
//! desenvuelve el DEK (`envelope::unwrap_dek`). Si el código escrito por la
//! usuaria es incorrecto, la derivación produce una KEK distinta y el
//! desenvolvimiento falla por autenticación — igual que con la contraseña.
//! No existe una tabla ni un campo "código de recuperación" en ningún lado.

use std::fmt;

use zeroize::Zeroize;

use super::random;

pub const RAW_LEN: usize = 15; // 120 bits
const SYMBOL_COUNT: usize = 24; // 120 bits / 5 bits por símbolo
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug)]
pub enum RecoveryCodeError {
    Random(getrandom::Error),
    WrongLength { got_symbols: usize },
    InvalidCharacter(char),
}

impl fmt::Display for RecoveryCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecoveryCodeError::Random(_) => write!(f, "no se pudo generar el código de recuperación"),
            RecoveryCodeError::WrongLength { got_symbols } => write!(
                f,
                "el código de recuperación debe tener {SYMBOL_COUNT} caracteres, se recibieron {got_symbols}"
            ),
            RecoveryCodeError::InvalidCharacter(c) => {
                write!(f, "carácter no válido en el código de recuperación: '{c}'")
            }
        }
    }
}
impl std::error::Error for RecoveryCodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RecoveryCodeError::Random(e) => Some(e),
            _ => None,
        }
    }
}
impl From<getrandom::Error> for RecoveryCodeError {
    fn from(e: getrandom::Error) -> Self {
        RecoveryCodeError::Random(e)
    }
}

/// Código de recuperación ya generado o ya parseado. Se trata como un
/// secreto: `Debug` redactado, se zeroiza al soltarse, igual que el DEK y la
/// KEK.
pub struct RecoveryCode([u8; RAW_LEN]);

impl RecoveryCode {
    /// Genera un código nuevo a partir del generador de aleatoriedad
    /// criptográfica del sistema operativo.
    pub fn generate() -> Result<Self, RecoveryCodeError> {
        Ok(Self(random::bytes::<RAW_LEN>()?))
    }

    /// Formato para mostrar a la usuaria: `XXXX-XXXX-XXXX-XXXX-XXXX-XXXX`.
    pub fn to_display_string(&self) -> String {
        let symbols = encode(&self.0);
        let mut out = String::with_capacity(SYMBOL_COUNT + SYMBOL_COUNT / 4 - 1);
        for (i, ch) in symbols.chars().enumerate() {
            if i > 0 && i % 4 == 0 {
                out.push('-');
            }
            out.push(ch);
        }
        out
    }

    /// Interpreta lo que la usuaria escribió (tolerante a mayúsculas,
    /// minúsculas, guiones y espacios de más, y a los reemplazos típicos de
    /// Crockford: O↔0, I/L↔1) y lo convierte de vuelta a los 15 bytes
    /// originales.
    pub fn parse(input: &str) -> Result<Self, RecoveryCodeError> {
        let symbols: String = input.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
        if symbols.chars().count() != SYMBOL_COUNT {
            return Err(RecoveryCodeError::WrongLength {
                got_symbols: symbols.chars().count(),
            });
        }
        Ok(Self(decode(&symbols)?))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; RAW_LEN] {
        &self.0
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryCode").field("value", &"<redacted>").finish()
    }
}

impl Drop for RecoveryCode {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

fn encode(bytes: &[u8; RAW_LEN]) -> String {
    let mut bits: u128 = 0;
    for &b in bytes {
        bits = (bits << 8) | u128::from(b);
    }
    let mut out = String::with_capacity(SYMBOL_COUNT);
    for i in (0..SYMBOL_COUNT).rev() {
        let shift = i * 5;
        let idx = ((bits >> shift) & 0x1F) as usize;
        out.push(ALPHABET[idx] as char);
    }
    out
}

fn char_value(c: char) -> Option<u8> {
    let normalized = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    ALPHABET.iter().position(|&a| a as char == normalized).map(|p| p as u8)
}

fn decode(symbols: &str) -> Result<[u8; RAW_LEN], RecoveryCodeError> {
    let mut bits: u128 = 0;
    for c in symbols.chars() {
        let v = char_value(c).ok_or(RecoveryCodeError::InvalidCharacter(c))?;
        bits = (bits << 5) | u128::from(v);
    }
    let mut out = [0u8; RAW_LEN];
    for (i, byte) in out.iter_mut().enumerate() {
        let shift = (RAW_LEN - 1 - i) * 8;
        *byte = ((bits >> shift) & 0xFF) as u8;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_code_has_the_expected_display_format() {
        let code = RecoveryCode::generate().unwrap();
        let display = code.to_display_string();
        let groups: Vec<&str> = display.split('-').collect();
        assert_eq!(groups.len(), 6, "formato esperado: XXXX-XXXX-XXXX-XXXX-XXXX-XXXX");
        for g in groups {
            assert_eq!(g.len(), 4);
            assert!(g.chars().all(|c| ALPHABET.contains(&(c as u8))));
        }
    }

    #[test]
    fn two_generated_codes_are_different() {
        let a = RecoveryCode::generate().unwrap();
        let b = RecoveryCode::generate().unwrap();
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn parsing_the_displayed_string_recovers_the_same_bytes() {
        let code = RecoveryCode::generate().unwrap();
        let display = code.to_display_string();
        let parsed = RecoveryCode::parse(&display).unwrap();
        assert_eq!(code.0, parsed.0);
    }

    #[test]
    fn parsing_is_tolerant_to_lowercase_and_extra_whitespace() {
        let code = RecoveryCode::generate().unwrap();
        let display = code.to_display_string();
        let messy = format!("  {}  ", display.to_lowercase());
        let parsed = RecoveryCode::parse(&messy).unwrap();
        assert_eq!(code.0, parsed.0);
    }

    #[test]
    fn parsing_tolerates_o_i_l_confusions() {
        // Fabricamos un código de solo dígitos y letras seguras para poder
        // sustituir un carácter conocido por su "confundible" sin adivinar
        // por casualidad qué símbolo salió.
        let code = RecoveryCode::generate().unwrap();
        let mut display = code.to_display_string();
        // Reemplazamos el primer '0' que aparezca por 'O' (y si no hay
        // ninguno, el primer '1' por 'I'), para ejercitar la normalización.
        if let Some(pos) = display.find('0') {
            display.replace_range(pos..pos + 1, "O");
        } else if let Some(pos) = display.find('1') {
            display.replace_range(pos..pos + 1, "I");
        }
        let parsed = RecoveryCode::parse(&display).unwrap();
        assert_eq!(code.0, parsed.0);
    }

    #[test]
    fn rejects_wrong_length() {
        let err = RecoveryCode::parse("ABCD-1234").unwrap_err();
        assert!(matches!(err, RecoveryCodeError::WrongLength { .. }));
    }

    #[test]
    fn rejects_invalid_characters() {
        // 'U' no forma parte del alfabeto de Crockford.
        let err = RecoveryCode::parse("UUUU-UUUU-UUUU-UUUU-UUUU-UUUU").unwrap_err();
        assert!(matches!(err, RecoveryCodeError::InvalidCharacter('U')));
    }

    #[test]
    fn debug_never_prints_the_code() {
        let code = RecoveryCode::generate().unwrap();
        assert_eq!(format!("{code:?}"), "RecoveryCode { value: \"<redacted>\" }");
    }
}
