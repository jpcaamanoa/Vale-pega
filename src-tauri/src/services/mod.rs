//! Reglas de negocio, un módulo por dominio. No conocen Tauri ni el estado
//! de bloqueo del vault — reciben una `&rusqlite::Connection` ya
//! desbloqueada desde la capa de comandos.

pub mod patients;
pub mod rut;
