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
