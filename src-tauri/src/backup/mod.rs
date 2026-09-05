//! Backup y Restore (Fase 10). Ver `docs/backup-restore.md` para el diseño
//! completo: formato `.cclinbackup`, manifest, snapshot consistente,
//! staging, swap atómico y recuperación ante fallos.

mod archive;
mod manifest;
pub mod service;

pub use manifest::BackupManifest;
