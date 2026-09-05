//! `manifest.json`: el índice del contenedor `.cclinbackup` — qué archivos
//! contiene, su tamaño y su hash. Deliberadamente pequeño y sin ningún
//! campo clínico: la minimización es estructural (el tipo no tiene dónde
//! poner un nombre de paciente), no una convención que dependa de que
//! nadie la rompa por accidente.
//!
//! Nunca contiene: nombre del equipo, username del sistema, nombres de
//! paciente, conteos de pacientes/diagnósticos, dirección, RUT, cuenta de
//! Google, ni ningún dato clínico. Ver `docs/backup-restore.md`.

use serde::{Deserialize, Serialize};

/// Versión del *formato del contenedor* (estructura del ZIP + manifest),
/// independiente de `schema_version` (la versión del esquema SQL de
/// `vault.db`, ver `db::migrations`) y de `vault_meta_format_version` (la
/// versión de `vault.meta.json`, ver `security::vault_meta::FORMAT_VERSION`
/// — no reexportada, se copia su valor tal cual al manifest).
pub const BACKUP_FORMAT_VERSION: u32 = 1;

/// Nombres de archivo fijos dentro del contenedor. `vault.db`/`vault.meta.json`
/// son obligatorios en todo backup `v1`; `documents/` es un directorio
/// opcional reservado para una fase futura (Documentos clínicos, todavía no
/// implementada) — un backup `v1` de hoy simplemente no lo incluye, y el
/// restore nunca exige que exista.
pub const VAULT_DB_ENTRY: &str = "vault.db";
pub const VAULT_META_ENTRY: &str = "vault.meta.json";
pub const MANIFEST_ENTRY: &str = "manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFileEntry {
    /// Ruta relativa dentro del contenedor (p. ej. `"vault.db"`, o en el
    /// futuro `"documents/<id>.enc"`). Nunca una ruta absoluta del sistema
    /// que creó el backup.
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub backup_format_version: u32,
    /// UUID aleatorio — solo para trazabilidad interna del propio archivo
    /// (p. ej. distinguir dos backups con el mismo nombre de archivo). No
    /// identifica a la usuaria ni al dispositivo.
    pub backup_id: String,
    pub created_at: String,
    /// `CARGO_PKG_VERSION` de esta build — para diagnóstico humano
    /// ("¿con qué versión de la app se hizo esto?"), nunca para decidir
    /// compatibilidad (eso lo decide `schema_version`, ver
    /// `docs/backup-restore.md` sección de compatibilidad).
    pub app_version: String,
    /// `PRAGMA user_version` de `vault.db` en el momento del backup —
    /// mismo número que usa `rusqlite_migration` internamente.
    pub schema_version: i64,
    pub vault_meta_format_version: u32,
    pub files: Vec<BackupFileEntry>,
}

impl BackupManifest {
    pub fn find_entry(&self, path: &str) -> Option<&BackupFileEntry> {
        self.files.iter().find(|f| f.path == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips_through_json() {
        let manifest = BackupManifest {
            backup_format_version: BACKUP_FORMAT_VERSION,
            backup_id: "11111111-1111-1111-1111-111111111111".to_string(),
            created_at: "2026-09-05T00:00:00.000Z".to_string(),
            app_version: "0.1.0".to_string(),
            schema_version: 4,
            vault_meta_format_version: 1,
            files: vec![
                BackupFileEntry { path: VAULT_DB_ENTRY.to_string(), size_bytes: 1234, sha256: "a".repeat(64) },
                BackupFileEntry { path: VAULT_META_ENTRY.to_string(), size_bytes: 200, sha256: "b".repeat(64) },
            ],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: BackupManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.schema_version, 4);
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(parsed.find_entry(VAULT_DB_ENTRY).unwrap().size_bytes, 1234);
    }

    /// El tipo no tiene ningún campo donde pudiera colarse un dato clínico
    /// — esta prueba documenta la minimización estructural serializando y
    /// confirmando que el conjunto exacto de claves es el esperado, ni una
    /// más.
    #[test]
    fn manifest_json_has_exactly_the_expected_top_level_keys() {
        let manifest = BackupManifest {
            backup_format_version: BACKUP_FORMAT_VERSION,
            backup_id: "id".to_string(),
            created_at: "2026-09-05T00:00:00.000Z".to_string(),
            app_version: "0.1.0".to_string(),
            schema_version: 4,
            vault_meta_format_version: 1,
            files: vec![],
        };
        let value: serde_json::Value = serde_json::to_value(&manifest).unwrap();
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["app_version", "backup_format_version", "backup_id", "created_at", "files", "schema_version", "vault_meta_format_version"]
        );
    }
}
