//! Integración con Google Calendar (Fase 3): OAuth 2.0 Authorization Code +
//! PKCE, almacenamiento de tokens en el keychain del sistema operativo, y
//! sincronización unidireccional (app → Google) de citas. Ver
//! `docs/google-calendar.md` para el diseño completo.
//!
//! Ningún otro módulo de la aplicación debe construir una llamada HTTP a
//! Google directamente — todo pasa por aquí.

pub mod client;
pub mod oauth;
pub mod sync;
pub mod tokens;
