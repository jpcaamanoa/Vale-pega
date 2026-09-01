//! Acceso a datos de `sessions`. SQL puro — sin reglas de negocio (eso vive
//! en `services::sessions`) y sin ninguna noción de si el vault está
//! desbloqueado.
//!
//! Ningún campo de `session_notes` (contenido clínico) aparece en este
//! archivo: el listado (`SessionListItem`) solo sabe si hay una nota
//! vigente y si está cerrada, nunca su texto — ver
//! `docs/sessions.md` (minimización de exposición).

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub patient_id: String,
    pub appointment_id: Option<String>,
    pub session_date: String,
    pub start_time: Option<String>,
    pub duration_minutes: Option<i64>,
    pub modality: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// Fila de listado — deliberadamente sin contenido clínico. Suficiente para
/// una lista cronológica y para saber si hay una nota abierta, nunca el
/// texto de la nota.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListItem {
    pub id: String,
    pub session_date: String,
    pub start_time: Option<String>,
    pub duration_minutes: Option<i64>,
    pub modality: Option<String>,
    pub status: String,
    pub has_current_note: bool,
    pub current_note_is_locked: bool,
}

pub struct NewSessionRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub appointment_id: Option<&'a str>,
    pub session_date: &'a str,
    pub start_time: Option<&'a str>,
    pub duration_minutes: Option<i64>,
    pub modality: Option<&'a str>,
    pub status: &'a str,
}

/// Campos editables desde "metadata administrativa" (ver
/// `services::sessions`). Deliberadamente sin `patient_id` ni
/// `appointment_id`: ambos son estructurales, fijados una sola vez al
/// crear la sesión — reasignar una sesión clínica a otro paciente u otra
/// cita no es una operación de este MVP.
pub struct SessionMetadataUpdateRow<'a> {
    pub session_date: &'a str,
    pub start_time: Option<&'a str>,
    pub duration_minutes: Option<i64>,
    pub modality: Option<&'a str>,
    pub status: &'a str,
}

const SESSION_COLUMNS: &str = "id, patient_id, appointment_id, session_date, start_time, \
     duration_minutes, modality, status, created_at, updated_at, deleted_at";

fn map_row(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        appointment_id: row.get(2)?,
        session_date: row.get(3)?,
        start_time: row.get(4)?,
        duration_minutes: row.get(5)?,
        modality: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        deleted_at: row.get(10)?,
    })
}

pub fn insert(conn: &Connection, row: &NewSessionRow) -> rusqlite::Result<Session> {
    conn.execute(
        "INSERT INTO sessions (id, patient_id, appointment_id, session_date, start_time, \
         duration_minutes, modality, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            row.id,
            row.patient_id,
            row.appointment_id,
            row.session_date,
            row.start_time,
            row.duration_minutes,
            row.modality,
            row.status
        ],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<Session>> {
    conn.query_row(&format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id = ?1"), params![id], map_row).optional()
}

/// Cualquier sesión (activa o archivada) vinculada a esa cita — se usa para
/// aplicar la regla "una cita, como máximo una sesión" sin distinguir por
/// estado de archivado (ver `services::sessions::create_session`): una
/// sesión archivada sigue contando como "esta cita ya tiene una sesión".
pub fn find_by_appointment_id(conn: &Connection, appointment_id: &str) -> rusqlite::Result<Option<Session>> {
    conn.query_row(
        &format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE appointment_id = ?1"),
        params![appointment_id],
        map_row,
    )
    .optional()
}

fn list(conn: &Connection, patient_id: &str, deleted: bool) -> rusqlite::Result<Vec<SessionListItem>> {
    let deleted_clause = if deleted { "s.deleted_at IS NOT NULL" } else { "s.deleted_at IS NULL" };
    let sql = format!(
        "SELECT s.id, s.session_date, s.start_time, s.duration_minutes, s.modality, s.status, \
         n.id IS NOT NULL, COALESCE(n.is_locked, 0) \
         FROM sessions s \
         LEFT JOIN session_notes n ON n.session_id = s.id AND n.is_current = 1 \
         WHERE s.patient_id = ?1 AND {deleted_clause} \
         ORDER BY s.session_date DESC, COALESCE(s.start_time, '') DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], |row| {
        Ok(SessionListItem {
            id: row.get(0)?,
            session_date: row.get(1)?,
            start_time: row.get(2)?,
            duration_minutes: row.get(3)?,
            modality: row.get(4)?,
            status: row.get(5)?,
            has_current_note: row.get(6)?,
            current_note_is_locked: row.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn list_active_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<SessionListItem>> {
    list(conn, patient_id, false)
}

pub fn list_deleted_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<SessionListItem>> {
    list(conn, patient_id, true)
}

pub fn update_metadata(conn: &Connection, id: &str, row: &SessionMetadataUpdateRow) -> rusqlite::Result<Option<Session>> {
    let affected = conn.execute(
        "UPDATE sessions SET session_date = ?1, start_time = ?2, duration_minutes = ?3, \
         modality = ?4, status = ?5 WHERE id = ?6 AND deleted_at IS NULL",
        params![row.session_date, row.start_time, row.duration_minutes, row.modality, row.status, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

/// Soft delete únicamente. No existe, en ningún punto de este módulo, una
/// operación de borrado físico alcanzable desde un comando normal.
pub fn soft_delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE sessions SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("UPDATE sessions SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-sessions-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x11u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_patient(conn: &Connection, name: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        patients::insert(
            conn,
            &NewPatientRow {
                id: &id,
                full_name: name,
                preferred_name: None,
                rut: None,
                birth_date: None,
                phone: None,
                email: None,
                address: None,
                emergency_contact_name: None,
                emergency_contact_phone: None,
                emergency_contact_relationship: None,
                status: "activo",
                referred_by: None,
                intake_date: None,
            },
        )
        .unwrap();
        id
    }

    #[test]
    fn inserts_and_finds_a_session() {
        let conn = test_conn("insert-find");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let s = insert(
            &conn,
            &NewSessionRow {
                id: "s1",
                patient_id: &patient_id,
                appointment_id: None,
                session_date: "2026-09-01",
                start_time: Some("15:00"),
                duration_minutes: Some(50),
                modality: Some("presencial"),
                status: "programada",
            },
        )
        .unwrap();
        assert_eq!(s.patient_id, patient_id);
        assert!(s.appointment_id.is_none());
        assert_eq!(find_by_id(&conn, "s1").unwrap().unwrap().id, "s1");
    }

    #[test]
    fn find_by_appointment_id_returns_none_when_no_session_linked() {
        let conn = test_conn("find-by-appointment-none");
        assert!(find_by_appointment_id(&conn, "does-not-exist").unwrap().is_none());
    }

    #[test]
    fn list_active_and_deleted_are_mutually_exclusive() {
        let conn = test_conn("list-active-deleted");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        insert(
            &conn,
            &NewSessionRow {
                id: "s1",
                patient_id: &patient_id,
                appointment_id: None,
                session_date: "2026-09-01",
                start_time: None,
                duration_minutes: None,
                modality: None,
                status: "programada",
            },
        )
        .unwrap();

        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 1);
        assert_eq!(list_deleted_by_patient(&conn, &patient_id).unwrap().len(), 0);

        assert!(soft_delete(&conn, "s1").unwrap());

        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 0);
        assert_eq!(list_deleted_by_patient(&conn, &patient_id).unwrap().len(), 1);

        assert!(restore(&conn, "s1").unwrap());
        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 1);
    }

    #[test]
    fn update_metadata_changes_fields_but_not_patient_or_appointment() {
        let conn = test_conn("update-metadata");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        insert(
            &conn,
            &NewSessionRow {
                id: "s1",
                patient_id: &patient_id,
                appointment_id: None,
                session_date: "2026-09-01",
                start_time: None,
                duration_minutes: None,
                modality: None,
                status: "programada",
            },
        )
        .unwrap();

        let updated = update_metadata(
            &conn,
            "s1",
            &SessionMetadataUpdateRow {
                session_date: "2026-09-02",
                start_time: Some("16:00"),
                duration_minutes: Some(45),
                modality: Some("online"),
                status: "realizada",
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.session_date, "2026-09-02");
        assert_eq!(updated.status, "realizada");
        assert_eq!(updated.patient_id, patient_id);
    }

    #[test]
    fn update_metadata_on_archived_session_does_nothing() {
        let conn = test_conn("update-metadata-archived");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        insert(
            &conn,
            &NewSessionRow {
                id: "s1",
                patient_id: &patient_id,
                appointment_id: None,
                session_date: "2026-09-01",
                start_time: None,
                duration_minutes: None,
                modality: None,
                status: "programada",
            },
        )
        .unwrap();
        soft_delete(&conn, "s1").unwrap();

        let result = update_metadata(
            &conn,
            "s1",
            &SessionMetadataUpdateRow { session_date: "2026-09-02", start_time: None, duration_minutes: None, modality: None, status: "realizada" },
        )
        .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn restoring_a_never_archived_session_reports_nothing_changed() {
        let conn = test_conn("restore-noop");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        insert(
            &conn,
            &NewSessionRow {
                id: "s1",
                patient_id: &patient_id,
                appointment_id: None,
                session_date: "2026-09-01",
                start_time: None,
                duration_minutes: None,
                modality: None,
                status: "programada",
            },
        )
        .unwrap();
        assert!(!restore(&conn, "s1").unwrap());
    }
}
