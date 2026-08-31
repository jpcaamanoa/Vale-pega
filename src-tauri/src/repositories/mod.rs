//! Acceso a datos, un módulo por agregado. SQL puro — sin reglas de
//! negocio (eso vive en `services`) y sin acceso directo desde React (eso
//! solo puede pasar por comandos Tauri, que a su vez solo pueden obtener
//! una conexión a través de `security::session::VaultSession`).

pub mod app_settings;
pub mod appointments;
pub mod patients;
