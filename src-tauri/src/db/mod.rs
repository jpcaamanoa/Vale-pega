//! Acceso a la base de datos cifrada (SQLite + SQLCipher).
//!
//! Ver `docs/ARCHITECTURE.md`, sección 5, y `docs/sqlcipher.md` para el diseño
//! completo de seguridad y las versiones exactas verificadas.

mod connection;
mod migrations;
#[cfg(test)]
mod test_support;

pub use connection::{open_vault, VaultError, VaultKey, VaultKeyError, VAULT_KEY_LEN};
pub use migrations::run_migrations;
