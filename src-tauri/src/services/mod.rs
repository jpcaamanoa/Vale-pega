//! Reglas de negocio, un módulo por dominio. No conocen Tauri ni el estado
//! de bloqueo del vault — reciben una `&rusqlite::Connection` ya
//! desbloqueada desde la capa de comandos.

pub mod appointments;
pub mod episode_clinical_profile;
pub mod goals;
pub mod patient_clinical_profile;
pub mod patient_prep_notes;
pub mod patients;
pub mod payments;
pub mod rut;
pub mod sessions;
pub mod therapy_tasks;
pub mod treatment_episodes;
