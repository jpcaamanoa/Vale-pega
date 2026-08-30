//! Acceso a la base de datos cifrada (SQLite + SQLCipher).
//!
//! Ver `docs/ARCHITECTURE.md`, sección 5, y `docs/sqlcipher.md` para el diseño
//! completo de seguridad y las versiones exactas verificadas.

// Ver la nota en connection.rs: todavía no hay un comando Tauri que use esta
// API (llega en las Fases 1.4/1.5), así que el re-export se marca sin usar.
#![allow(unused_imports)]

mod connection;

pub use connection::{open_vault, VaultError, VaultKey, VaultKeyError, VAULT_KEY_LEN};
