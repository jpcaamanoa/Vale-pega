//! Única fuente de aleatoriedad criptográfica del módulo de seguridad.
//!
//! Todo lo que necesita ser impredecible (el DEK, el código de recuperación,
//! las sales de Argon2id, los nonces de AES-GCM) pasa por aquí. Se usa
//! `getrandom` directamente — es una envoltura mínima sobre el generador de
//! aleatoriedad del sistema operativo (no una implementación propia de un
//! PRNG), la misma fuente que usan `rand`, `argon2` y `aes-gcm` internamente.

/// Llena `buf` con bytes del generador de aleatoriedad criptográfica del
/// sistema operativo.
pub fn fill(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    getrandom::fill(buf)
}

/// Genera un arreglo de `N` bytes aleatorios.
pub fn bytes<const N: usize>() -> Result<[u8; N], getrandom::Error> {
    let mut buf = [0u8; N];
    fill(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_the_whole_buffer_and_is_not_all_zero() {
        let a: [u8; 32] = bytes().unwrap();
        // Con 256 bits de aleatoriedad real, la probabilidad de que salga
        // todo cero es astronómicamente baja; si esto llega a fallar algún
        // día, hay un problema real con la fuente de entropía, no un bug de
        // este test.
        assert_ne!(a, [0u8; 32]);
    }

    #[test]
    fn two_calls_produce_different_output() {
        let a: [u8; 32] = bytes().unwrap();
        let b: [u8; 32] = bytes().unwrap();
        assert_ne!(a, b);
    }
}
