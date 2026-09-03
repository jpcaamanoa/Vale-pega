//! Reglas de negocio, un módulo por dominio. No conocen Tauri ni el estado
//! de bloqueo del vault — reciben una `&rusqlite::Connection` ya
//! desbloqueada desde la capa de comandos.

pub mod appointments;
pub mod goals;
pub mod patient_clinical_profile;
pub mod patients;
pub mod payments;
pub mod rut;
pub mod sessions;
