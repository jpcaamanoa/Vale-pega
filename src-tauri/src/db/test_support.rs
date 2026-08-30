//! Utilidades compartidas entre los tests de `connection` y `migrations`.
//! No se compila fuera de `cfg(test)`.
#![cfg(test)]

use std::fs;
use std::path::PathBuf;

use super::{VaultKey, VAULT_KEY_LEN};

/// Devuelve una ruta de archivo de base de datos aislada para un test, en un
/// directorio temporal propio (limpio en cada llamada).
pub fn temp_db_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cuaderno-clinico-test-{}-{}",
        std::process::id(),
        name
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    dir.join("vault.db")
}

/// Clave de prueba fija (no proviene de Argon2id: eso es Fase 1.4). Cada
/// llamada con un `byte` distinto produce una clave distinta, útil para
/// probar el rechazo de claves incorrectas.
pub fn key(byte: u8) -> VaultKey {
    VaultKey::new([byte; VAULT_KEY_LEN])
}
