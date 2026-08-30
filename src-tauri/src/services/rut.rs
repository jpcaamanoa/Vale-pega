//! Validación de RUT chileno (con dígito verificador, algoritmo módulo 11).
//!
//! No es un componente de seguridad ni de datos clínicos — es una validación
//! de formato de un campo administrativo opcional. El RUT nunca es
//! obligatorio (el esquema de `patients` lo permite `NULL`); esta función
//! solo se invoca cuando la usuaria ingresó un valor.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum RutError {
    Empty,
    /// No quedó ningún dígito para el cuerpo del RUT (p. ej. solo se
    /// escribió el dígito verificador).
    MissingBody,
    /// El cuerpo del RUT contiene algo que no es un dígito.
    InvalidBody,
    /// El dígito verificador no es '0'-'9' ni 'K'.
    InvalidCheckDigit,
    /// El dígito verificador no coincide con el calculado.
    CheckDigitMismatch,
}

impl fmt::Display for RutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RutError::Empty => write!(f, "el RUT está vacío"),
            RutError::MissingBody => write!(f, "el RUT no tiene dígitos antes del verificador"),
            RutError::InvalidBody => write!(f, "el RUT contiene caracteres inválidos"),
            RutError::InvalidCheckDigit => write!(f, "el dígito verificador no es válido"),
            RutError::CheckDigitMismatch => write!(f, "el dígito verificador no corresponde a este RUT"),
        }
    }
}
impl std::error::Error for RutError {}

/// Calcula el dígito verificador (algoritmo módulo 11) para un cuerpo de
/// RUT ya limpio (solo dígitos). Devuelve `'0'`-`'9'` o `'K'`.
fn compute_check_digit(body_digits: &[u32]) -> char {
    let mut sum = 0u32;
    let mut weight = 2u32;
    for &digit in body_digits.iter().rev() {
        sum += digit * weight;
        weight = if weight == 7 { 2 } else { weight + 1 };
    }
    match 11 - (sum % 11) {
        11 => '0',
        10 => 'K',
        n => char::from_digit(n, 10).expect("n está entre 0 y 9"),
    }
}

/// Normaliza (quita puntos, espacios y guiones, mayúsculas) y valida un RUT
/// chileno, incluyendo su dígito verificador. Acepta tanto `12.345.678-5`
/// como `123456785` o `12345678-5`.
pub fn validate_chilean_rut(input: &str) -> Result<(), RutError> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '.' && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();

    if cleaned.is_empty() {
        return Err(RutError::Empty);
    }

    let mut chars: Vec<char> = cleaned.chars().collect();
    let check_char = chars.pop().unwrap();
    if chars.is_empty() {
        return Err(RutError::MissingBody);
    }
    if !(check_char.is_ascii_digit() || check_char == 'K') {
        return Err(RutError::InvalidCheckDigit);
    }

    let mut body_digits = Vec::with_capacity(chars.len());
    for c in chars {
        match c.to_digit(10) {
            Some(d) => body_digits.push(d),
            None => return Err(RutError::InvalidBody),
        }
    }

    let expected = compute_check_digit(&body_digits);
    if expected == check_char {
        Ok(())
    } else {
        Err(RutError::CheckDigitMismatch)
    }
}

/// Normaliza un RUT ya validado al formato canónico de almacenamiento
/// (`12345678-5`, sin puntos). Se asume que `input` ya pasó
/// `validate_chilean_rut`.
pub fn normalize_chilean_rut(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '.' && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let (body, check) = cleaned.split_at(cleaned.len() - 1);
    format!("{body}-{check}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verificados a mano con el algoritmo módulo 11 oficial (no supuestos):
    // cuerpo 12345678 -> suma ponderada 138 -> 11 - (138 % 11) = 11-6 = 5.
    // cuerpo 7564230   -> suma ponderada 122 -> 11 - (122 % 11) = 11-1 = 10 -> 'K'.
    #[test]
    fn accepts_a_hand_verified_valid_rut_with_numeric_check_digit() {
        assert!(validate_chilean_rut("12345678-5").is_ok());
    }

    #[test]
    fn accepts_a_hand_verified_valid_rut_with_k_check_digit() {
        assert!(validate_chilean_rut("7564230-K").is_ok());
        assert!(validate_chilean_rut("7564230-k").is_ok(), "debe aceptar 'k' minúscula");
    }

    #[test]
    fn accepts_formats_with_and_without_dots_or_hyphen() {
        assert!(validate_chilean_rut("12.345.678-5").is_ok());
        assert!(validate_chilean_rut("123456785").is_ok());
        assert!(validate_chilean_rut("12345678-5").is_ok());
    }

    #[test]
    fn rejects_a_wrong_check_digit() {
        assert_eq!(validate_chilean_rut("12345678-6").unwrap_err(), RutError::CheckDigitMismatch);
    }

    #[test]
    fn rejects_empty_input() {
        assert_eq!(validate_chilean_rut("").unwrap_err(), RutError::Empty);
        assert_eq!(validate_chilean_rut("   ").unwrap_err(), RutError::Empty);
    }

    #[test]
    fn rejects_non_digit_body() {
        assert_eq!(validate_chilean_rut("12A45678-5").unwrap_err(), RutError::InvalidBody);
    }

    #[test]
    fn rejects_missing_body() {
        assert_eq!(validate_chilean_rut("5").unwrap_err(), RutError::MissingBody);
    }

    #[test]
    fn rejects_invalid_check_digit_character() {
        assert_eq!(validate_chilean_rut("12345678-X").unwrap_err(), RutError::InvalidCheckDigit);
    }

    #[test]
    fn normalizes_to_canonical_format() {
        assert_eq!(normalize_chilean_rut("12.345.678-5"), "12345678-5");
        assert_eq!(normalize_chilean_rut("7564230-k"), "7564230-K");
    }
}
