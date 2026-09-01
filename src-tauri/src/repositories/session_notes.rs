//! Acceso a datos de `session_notes`. SQL puro. Es el único módulo con
//! permiso de tocar contenido clínico de una nota — todo pasa por aquí,
//! nunca por SQL directo desde otra capa.
//!
//! Principio no negociable de esta fase, reforzado aquí a nivel de SQL, no
//! solo de convención: **una nota cerrada es inmutable**.
//! `update_draft_content` incluye `WHERE is_locked = 0` en el propio
//! `UPDATE` — estructuralmente imposible que esta función cambie el
//! contenido de una fila con `is_locked = 1`, incluso si quien la llama se
//! equivocara. Ver `updating_a_locked_note_directly_changes_nothing` más
//! abajo.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNote {
    pub id: String,
    pub session_id: String,
    pub content: Option<String>,
    pub interventions: Option<String>,
    pub homework_tasks: Option<String>,
    pub next_focus: Option<String>,
    pub version: i64,
    pub is_locked: bool,
    pub is_current: bool,
    pub closed_at: Option<String>,
    pub superseded_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewSessionNoteRow<'a> {
    pub id: &'a str,
    pub session_id: &'a str,
    pub content: Option<&'a str>,
    pub interventions: Option<&'a str>,
    pub homework_tasks: Option<&'a str>,
    pub next_focus: Option<&'a str>,
    pub version: i64,
}

const NOTE_COLUMNS: &str = "id, session_id, content, interventions, homework_tasks, next_focus, \
     version, is_locked, is_current, closed_at, superseded_at, created_at, updated_at";

fn map_row(row: &Row) -> rusqlite::Result<SessionNote> {
    Ok(SessionNote {
        id: row.get(0)?,
        session_id: row.get(1)?,
        content: row.get(2)?,
        interventions: row.get(3)?,
        homework_tasks: row.get(4)?,
        next_focus: row.get(5)?,
        version: row.get(6)?,
        is_locked: row.get(7)?,
        is_current: row.get(8)?,
        closed_at: row.get(9)?,
        superseded_at: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

/// Inserta una versión nueva. `is_locked = 0, is_current = 1` son fijos en
/// el propio `INSERT` — toda fila nueva nace como borrador vigente; nunca
/// se inserta ya cerrada.
pub fn insert(conn: &Connection, row: &NewSessionNoteRow) -> rusqlite::Result<SessionNote> {
    conn.execute(
        "INSERT INTO session_notes (id, session_id, content, interventions, homework_tasks, \
         next_focus, version, is_locked, is_current) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 1)",
        params![row.id, row.session_id, row.content, row.interventions, row.homework_tasks, row.next_focus, row.version],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<SessionNote>> {
    conn.query_row(&format!("SELECT {NOTE_COLUMNS} FROM session_notes WHERE id = ?1"), params![id], map_row).optional()
}

/// La versión vigente de la nota de una sesión. A lo sumo una fila, por el
/// índice único parcial `idx_session_notes_current` de `SCHEMA_V1`.
pub fn find_current(conn: &Connection, session_id: &str) -> rusqlite::Result<Option<SessionNote>> {
    conn.query_row(
        &format!("SELECT {NOTE_COLUMNS} FROM session_notes WHERE session_id = ?1 AND is_current = 1"),
        params![session_id],
        map_row,
    )
    .optional()
}

/// Todas las versiones de la nota de una sesión, de la más reciente a la
/// más antigua.
pub fn list_history(conn: &Connection, session_id: &str) -> rusqlite::Result<Vec<SessionNote>> {
    let mut stmt = conn.prepare(&format!("SELECT {NOTE_COLUMNS} FROM session_notes WHERE session_id = ?1 ORDER BY version DESC"))?;
    let rows = stmt.query_map(params![session_id], map_row)?;
    rows.collect()
}

/// Sobrescribe el contenido de un borrador (autoguardado). `WHERE
/// is_locked = 0` es la barrera estructural — ver el comentario de módulo.
/// Devuelve `false` (sin tocar nada) si la fila no existe o ya está
/// cerrada.
pub fn update_draft_content(
    conn: &Connection,
    id: &str,
    content: Option<&str>,
    interventions: Option<&str>,
    homework_tasks: Option<&str>,
    next_focus: Option<&str>,
) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE session_notes SET content = ?1, interventions = ?2, homework_tasks = ?3, \
         next_focus = ?4 WHERE id = ?5 AND is_locked = 0",
        params![content, interventions, homework_tasks, next_focus, id],
    )?;
    Ok(affected > 0)
}

/// Cierra una nota (`is_locked = 1`, `closed_at` = ahora). Solo afecta
/// filas todavía en borrador (`WHERE is_locked = 0`) — cerrar una nota ya
/// cerrada no cambia nada aquí (`affected == 0`); `services::sessions`
/// interpreta eso como éxito idempotente, no como error.
pub fn close(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE session_notes SET is_locked = 1, closed_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
         WHERE id = ?1 AND is_locked = 0",
        params![id],
    )?;
    Ok(affected > 0)
}

/// Marca una versión como reemplazada (`is_current = 0`, `superseded_at` =
/// ahora). Debe llamarse **antes** de insertar la versión siguiente — así
/// nunca hay un instante con dos filas `is_current = 1` para la misma
/// sesión, que el índice único parcial rechazaría.
pub fn mark_superseded(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE session_notes SET is_current = 0, superseded_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND is_current = 1",
        params![id],
    )?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::sessions::{self, NewSessionRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-session-notes-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x22u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    /// Crea un paciente y una sesión de prueba, devuelve el id de la sesión.
    fn test_session(conn: &Connection) -> String {
        let patient_id = uuid::Uuid::new_v4().to_string();
        patients::insert(
            conn,
            &NewPatientRow {
                id: &patient_id,
                full_name: "Paciente de Prueba",
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
        let session_id = uuid::Uuid::new_v4().to_string();
        sessions::insert(
            conn,
            &NewSessionRow {
                id: &session_id,
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
        session_id
    }

    #[test]
    fn inserts_a_note_as_an_open_current_draft() {
        let conn = test_conn("insert-draft");
        let session_id = test_session(&conn);
        let note = insert(&conn, &NewSessionNoteRow { id: "n1", session_id: &session_id, content: None, interventions: None, homework_tasks: None, next_focus: None, version: 1 })
            .unwrap();
        assert_eq!(note.version, 1);
        assert!(!note.is_locked);
        assert!(note.is_current);
        assert!(note.closed_at.is_none());
        assert!(note.superseded_at.is_none());
    }

    #[test]
    fn find_current_returns_the_only_vigente_version() {
        let conn = test_conn("find-current");
        let session_id = test_session(&conn);
        insert(&conn, &NewSessionNoteRow { id: "n1", session_id: &session_id, content: None, interventions: None, homework_tasks: None, next_focus: None, version: 1 }).unwrap();
        let current = find_current(&conn, &session_id).unwrap().unwrap();
        assert_eq!(current.id, "n1");
    }

    #[test]
    fn update_draft_content_writes_to_an_unlocked_note() {
        let conn = test_conn("update-draft");
        let session_id = test_session(&conn);
        insert(&conn, &NewSessionNoteRow { id: "n1", session_id: &session_id, content: None, interventions: None, homework_tasks: None, next_focus: None, version: 1 }).unwrap();

        let changed = update_draft_content(&conn, "n1", Some("contenido de prueba"), None, None, None).unwrap();
        assert!(changed);
        let note = find_by_id(&conn, "n1").unwrap().unwrap();
        assert_eq!(note.content.as_deref(), Some("contenido de prueba"));
    }

    /// El test central de la regla de inmutabilidad: intentar escribir
    /// directamente sobre una nota cerrada, vía la misma función que usa el
    /// autoguardado, no cambia absolutamente nada.
    #[test]
    fn updating_a_locked_note_directly_changes_nothing() {
        let conn = test_conn("update-locked-noop");
        let session_id = test_session(&conn);
        insert(&conn, &NewSessionNoteRow { id: "n1", session_id: &session_id, content: Some("contenido original"), interventions: None, homework_tasks: None, next_focus: None, version: 1 })
            .unwrap();
        assert!(close(&conn, "n1").unwrap());

        let changed = update_draft_content(&conn, "n1", Some("intento de sobrescritura"), None, None, None).unwrap();
        assert!(!changed, "update_draft_content nunca debe reportar éxito sobre una nota cerrada");

        let note = find_by_id(&conn, "n1").unwrap().unwrap();
        assert_eq!(note.content.as_deref(), Some("contenido original"), "el contenido de una nota cerrada no debe cambiar jamás");
        assert!(note.is_locked);
    }

    #[test]
    fn closing_sets_locked_and_closed_at() {
        let conn = test_conn("close-note");
        let session_id = test_session(&conn);
        insert(&conn, &NewSessionNoteRow { id: "n1", session_id: &session_id, content: Some("algo"), interventions: None, homework_tasks: None, next_focus: None, version: 1 }).unwrap();

        assert!(close(&conn, "n1").unwrap());
        let note = find_by_id(&conn, "n1").unwrap().unwrap();
        assert!(note.is_locked);
        assert!(note.closed_at.is_some());
    }

    #[test]
    fn closing_an_already_closed_note_reports_no_change() {
        let conn = test_conn("close-already-closed");
        let session_id = test_session(&conn);
        insert(&conn, &NewSessionNoteRow { id: "n1", session_id: &session_id, content: Some("algo"), interventions: None, homework_tasks: None, next_focus: None, version: 1 }).unwrap();
        assert!(close(&conn, "n1").unwrap());

        assert!(!close(&conn, "n1").unwrap(), "cerrar una nota ya cerrada no debe volver a reportar éxito a nivel de repositorio");
    }

    #[test]
    fn mark_superseded_sets_flags_and_only_affects_the_current_row() {
        let conn = test_conn("mark-superseded");
        let session_id = test_session(&conn);
        insert(&conn, &NewSessionNoteRow { id: "n1", session_id: &session_id, content: Some("v1"), interventions: None, homework_tasks: None, next_focus: None, version: 1 }).unwrap();

        assert!(mark_superseded(&conn, "n1").unwrap());
        let note = find_by_id(&conn, "n1").unwrap().unwrap();
        assert!(!note.is_current);
        assert!(note.superseded_at.is_some());

        assert!(!mark_superseded(&conn, "n1").unwrap(), "ya no es la vigente, no debería volver a marcarse");
    }

    #[test]
    fn list_history_returns_all_versions_most_recent_first() {
        let conn = test_conn("list-history");
        let session_id = test_session(&conn);
        insert(&conn, &NewSessionNoteRow { id: "n1", session_id: &session_id, content: Some("v1"), interventions: None, homework_tasks: None, next_focus: None, version: 1 }).unwrap();
        mark_superseded(&conn, "n1").unwrap();
        insert(&conn, &NewSessionNoteRow { id: "n2", session_id: &session_id, content: Some("v2"), interventions: None, homework_tasks: None, next_focus: None, version: 2 }).unwrap();
        mark_superseded(&conn, "n2").unwrap();
        insert(&conn, &NewSessionNoteRow { id: "n3", session_id: &session_id, content: Some("v3"), interventions: None, homework_tasks: None, next_focus: None, version: 3 }).unwrap();

        let history = list_history(&conn, &session_id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].version, 3);
        assert_eq!(history[1].version, 2);
        assert_eq!(history[2].version, 1);
        assert!(history[0].is_current);
        assert!(!history[1].is_current);
        assert!(!history[2].is_current);
        // Las versiones anteriores conservan su contenido original intacto.
        assert_eq!(history[1].content.as_deref(), Some("v2"));
        assert_eq!(history[2].content.as_deref(), Some("v1"));
    }

    #[test]
    fn the_database_itself_rejects_two_current_versions_for_the_same_session() {
        let conn = test_conn("db-rejects-two-current");
        let session_id = test_session(&conn);
        insert(&conn, &NewSessionNoteRow { id: "n1", session_id: &session_id, content: None, interventions: None, homework_tasks: None, next_focus: None, version: 1 }).unwrap();

        // Sin marcar n1 como no-vigente primero, insertar una segunda fila
        // `is_current = 1` para la misma sesión debe violar el índice único
        // parcial `idx_session_notes_current` de SCHEMA_V1.
        let result = insert(&conn, &NewSessionNoteRow { id: "n2", session_id: &session_id, content: None, interventions: None, homework_tasks: None, next_focus: None, version: 2 });
        assert!(result.is_err(), "la base de datos debe rechazar una segunda versión vigente para la misma sesión");
    }
}
