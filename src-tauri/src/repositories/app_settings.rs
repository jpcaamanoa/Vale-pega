//! Acceso a `app_settings` (clave-valor genérica, Fase 1.3). SQL puro.
//!
//! Se usa en la Fase 3 para preferencias no sensibles de la integración con
//! Google Calendar (Client ID, Client Secret de la app de escritorio — ver
//! `docs/google-calendar.md` sobre por qué esto no es equivalente a un
//! `access_token`/`refresh_token`, y el calendario de Google seleccionado).
//! Los tokens de acceso NUNCA pasan por aquí — viven en el keychain del
//! sistema operativo (`security::calendar_tokens` en el módulo `calendar`).

use rusqlite::{params, Connection, OptionalExtension};

pub fn get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM app_settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()
}

pub fn set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, key: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM app_settings WHERE key = ?1", params![key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-app-settings-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x51u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    #[test]
    fn missing_key_returns_none() {
        let conn = test_conn("app-settings-missing");
        assert_eq!(get(&conn, "no_existe").unwrap(), None);
    }

    #[test]
    fn set_then_get_roundtrips() {
        let conn = test_conn("app-settings-roundtrip");
        set(&conn, "google_calendar_id", "primary").unwrap();
        assert_eq!(get(&conn, "google_calendar_id").unwrap(), Some("primary".to_string()));
    }

    #[test]
    fn set_again_overwrites_the_previous_value() {
        let conn = test_conn("app-settings-overwrite");
        set(&conn, "google_calendar_id", "primary").unwrap();
        set(&conn, "google_calendar_id", "otro@group.calendar.google.com").unwrap();
        assert_eq!(
            get(&conn, "google_calendar_id").unwrap(),
            Some("otro@group.calendar.google.com".to_string())
        );
    }

    #[test]
    fn delete_removes_the_key() {
        let conn = test_conn("app-settings-delete");
        set(&conn, "google_calendar_id", "primary").unwrap();
        delete(&conn, "google_calendar_id").unwrap();
        assert_eq!(get(&conn, "google_calendar_id").unwrap(), None);
    }
}
