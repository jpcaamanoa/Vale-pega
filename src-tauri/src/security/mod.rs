//! Módulo de seguridad (Fase 1.4): Argon2id + cifrado por sobres del DEK,
//! generación del código de recuperación, y el archivo `vault.meta.json`.
//! Ver `docs/ARCHITECTURE.md` sección 5 y `docs/security.md` para el diseño
//! completo y las decisiones tomadas en esta fase.
//!
//! `session::VaultSession` es la única puerta de entrada pensada para el
//! resto de la aplicación (comandos Tauri incluidos); el resto de los
//! submódulos son detalles de implementación. Los tipos de error de cada
//! operación (`vault_manager::UnlockError` y similares) no se re-exportan
//! aquí porque hoy solo se consumen a través de su `Display` (los comandos
//! Tauri devuelven `Result<_, String>`) — si una fase futura necesita
//! nombrarlos fuera de `security`, se agregan entonces.

mod b64;
mod envelope;
mod kdf;
mod password_policy;
mod random;
mod recovery_code;
mod session;
mod vault_manager;
mod vault_meta;

pub use password_policy::{evaluate as evaluate_password_strength, PasswordStrength};
pub use session::{VaultSession, VaultStatus};

// Fase 16 (Documentos cifrados): capacidad mínima de envolver/desenvolver
// una DEK de archivo con la DEK del vault, sin exponer nunca la DEK del
// vault en sí — ver el bloque de comentarios en `session.rs` justo antes de
// `FileKey`. Consumido exclusivamente por `services::document_crypto`.
pub use session::{FileKey, UnwrapFileKeyError, WrapFileKeyError, WrappedFileKey, FILE_KEY_WRAP_NONCE_LEN};

// CRYPTO-1 (hardening pre-RC, Fase 17): esquema de envoltura de la DEK de un archivo, tipado en
// vez de propagar el entero crudo de `documents.key_wrap_version` por las capas de
// servicio/repositorio. Consumido por `services::documents`. `UnknownKeyWrapVersion` (el error de
// `TryFrom<i64>`) no se re-exporta: quien la consume solo necesita su `Display` vía `DocumentError`.
pub use session::KeyWrapVersion;

// Re-exportado únicamente para `backup::service` (Fase 10): validar la
// contraseña de un vault en *staging* (una copia restaurada temporal, nunca
// el vault activo de `VaultSession`) exige llamar exactamente la misma
// lógica de desenvolvimiento del DEK que ya usa `VaultSession` — nunca una
// reimplementación paralela. `VaultSession` sigue siendo la única puerta de
// entrada para el vault *activo*; esto no cambia esa regla, solo permite
// ejercer la misma lógica pura sobre una ruta de archivo distinta y
// desechable. `recover_access` (la variante pública que migra) no se
// re-exporta aquí: `backup::service` solo necesita `recover_access_without_migrating`
// (más abajo) para su verificación de staging, y `VaultSession::recover_access`
// llama a `vault_manager::recover_access` directamente dentro del propio
// módulo `security`, sin pasar por este re-export.
pub use vault_manager::{unlock_vault, RecoveryError, UnlockError, VaultPaths};
/// Uso interno exclusivo de `backup::service::restore_backup` — ver el
/// comentario de estas funciones en `vault_manager.rs` sobre por qué el
/// staging de un restore necesita inspeccionar el esquema antes de migrar.
pub(crate) use vault_manager::{recover_access_without_migrating, unlock_vault_without_migrating};
