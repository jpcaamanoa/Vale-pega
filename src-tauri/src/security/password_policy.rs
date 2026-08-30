//! Evaluación de fortaleza de contraseña.
//!
//! Esto no es un componente criptográfico: es una heurística de UX real
//! (longitud + diversidad de tipos de carácter), no un simulacro. Es
//! deliberadamente más simple que un detector de patrones estilo zxcvbn —
//! ver `docs/security.md` para la justificación de no agregar esa
//! dependencia en esta fase — pero el mínimo (`validate`) sí se aplica de
//! verdad como bloqueo, no solo como sugerencia visual.

pub const MIN_LENGTH: usize = 12;
const MIN_CHARACTER_CLASSES: usize = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum PasswordPolicyError {
    TooShort,
    TooFewCharacterClasses,
}

impl std::fmt::Display for PasswordPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasswordPolicyError::TooShort => {
                write!(f, "la contraseña debe tener al menos {MIN_LENGTH} caracteres")
            }
            PasswordPolicyError::TooFewCharacterClasses => write!(
                f,
                "la contraseña debe combinar al menos {MIN_CHARACTER_CLASSES} tipos de carácter (minúsculas, mayúsculas, números, símbolos)"
            ),
        }
    }
}
impl std::error::Error for PasswordPolicyError {}

fn character_classes(password: &str) -> usize {
    let has_lower = password.chars().any(|c| c.is_ascii_lowercase() || c.is_lowercase());
    let has_upper = password.chars().any(|c| c.is_ascii_uppercase() || c.is_uppercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_symbol = password
        .chars()
        .any(|c| !c.is_alphanumeric() && !c.is_whitespace());
    [has_lower, has_upper, has_digit, has_symbol]
        .iter()
        .filter(|x| **x)
        .count()
}

/// Bloqueo real: se aplica antes de crear el vault, cambiar la contraseña, o
/// establecer una nueva contraseña tras recuperar el acceso.
pub fn validate(password: &str) -> Result<(), PasswordPolicyError> {
    if password.chars().count() < MIN_LENGTH {
        return Err(PasswordPolicyError::TooShort);
    }
    if character_classes(password) < MIN_CHARACTER_CLASSES {
        return Err(PasswordPolicyError::TooFewCharacterClasses);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StrengthLabel {
    Debil,
    Aceptable,
    Fuerte,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct PasswordStrength {
    pub score: u8,
    pub label: StrengthLabel,
}

/// Puntaje puramente informativo para un medidor en la UI. No sustituye a
/// `validate`, que es lo único que realmente bloquea contraseñas débiles.
pub fn evaluate(password: &str) -> PasswordStrength {
    let len = password.chars().count();
    let len_score = (len.min(24) as f32 / 24.0) * 60.0;
    let class_score = (character_classes(password) as f32 / 4.0) * 40.0;
    let score = (len_score + class_score).round().clamp(0.0, 100.0) as u8;
    let label = match score {
        0..=39 => StrengthLabel::Debil,
        40..=69 => StrengthLabel::Aceptable,
        _ => StrengthLabel::Fuerte,
    };
    PasswordStrength { score, label }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_passwords() {
        assert_eq!(validate("Corta1!").unwrap_err(), PasswordPolicyError::TooShort);
    }

    #[test]
    fn rejects_long_but_single_class_passwords() {
        assert_eq!(
            validate("aaaaaaaaaaaaaaaaaaaa").unwrap_err(),
            PasswordPolicyError::TooFewCharacterClasses
        );
    }

    #[test]
    fn accepts_a_reasonable_passphrase() {
        assert!(validate("correcto caballo bateria 42").is_ok());
    }

    #[test]
    fn accepts_a_mixed_case_password_with_digits() {
        assert!(validate("ContrasenaSegura2026").is_ok());
    }

    #[test]
    fn strength_score_increases_with_length_and_diversity() {
        let weak = evaluate("aaaa");
        let strong = evaluate("C0ntras3ña!Larga#Segura");
        assert!(strong.score > weak.score);
        assert_eq!(weak.label, StrengthLabel::Debil);
        assert_eq!(strong.label, StrengthLabel::Fuerte);
    }
}
