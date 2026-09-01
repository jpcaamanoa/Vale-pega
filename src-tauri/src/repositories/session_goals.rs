//! Acceso a datos de `session_goals` — tabla puente N:M entre `sessions` y
//! `therapeutic_goals`. SQL puro.
//!
//! La propia clave primaria compuesta `(session_id, goal_id)` de
//! `SCHEMA_V1` impide duplicados a nivel de base de datos — este módulo no
//! agrega ningún índice ni constraint adicional (ver `docs/goals.md`).
//! `services::goals` verifica existencia antes de insertar para devolver un
//! error de dominio claro en vez de dejar que la restricción de la base
//! falle con un error SQL crudo.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

/// Vista de un vínculo desde el lado de la sesión: qué objetivo se trabajó,
/// con lo mínimo del objetivo para mostrarlo (nunca su `description`
/// completa — eso es responsabilidad de `GoalDetailScreen`, no de esta
/// lista).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionGoalRow {
    pub goal_id: String,
    pub goal_title: String,
    pub goal_status: String,
    pub progress_note: Option<String>,
}

/// Vista de un vínculo desde el lado del objetivo: en qué sesión se trabajó,
/// con lo mínimo administrativo de la sesión (nunca el contenido de su
/// nota clínica).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSessionRow {
    pub session_id: String,
    pub session_date: String,
    pub start_time: Option<String>,
    pub session_status: String,
    pub progress_note: Option<String>,
}

fn map_session_goal_row(row: &Row) -> rusqlite::Result<SessionGoalRow> {
    Ok(SessionGoalRow { goal_id: row.get(0)?, goal_title: row.get(1)?, goal_status: row.get(2)?, progress_note: row.get(3)? })
}

fn map_goal_session_row(row: &Row) -> rusqlite::Result<GoalSessionRow> {
    Ok(GoalSessionRow { session_id: row.get(0)?, session_date: row.get(1)?, start_time: row.get(2)?, session_status: row.get(3)?, progress_note: row.get(4)? })
}

pub fn exists(conn: &Connection, session_id: &str, goal_id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM session_goals WHERE session_id = ?1 AND goal_id = ?2",
        params![session_id, goal_id],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
}

pub fn link(conn: &Connection, session_id: &str, goal_id: &str, progress_note: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO session_goals (session_id, goal_id, progress_note) VALUES (?1, ?2, ?3)",
        params![session_id, goal_id, progress_note],
    )?;
    Ok(())
}

pub fn unlink(conn: &Connection, session_id: &str, goal_id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM session_goals WHERE session_id = ?1 AND goal_id = ?2", params![session_id, goal_id])?;
    Ok(affected > 0)
}

pub fn update_progress_note(conn: &Connection, session_id: &str, goal_id: &str, progress_note: Option<&str>) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE session_goals SET progress_note = ?1 WHERE session_id = ?2 AND goal_id = ?3",
        params![progress_note, session_id, goal_id],
    )?;
    Ok(affected > 0)
}

/// Objetivos trabajados en una sesión — incluye objetivos archivados si ya
/// estaban vinculados (archivar un objetivo no borra su historial de
/// trabajo en sesiones pasadas, ver `docs/goals.md`).
pub fn list_for_session(conn: &Connection, session_id: &str) -> rusqlite::Result<Vec<SessionGoalRow>> {
    let mut stmt = conn.prepare(
        "SELECT g.id, g.title, g.status, sg.progress_note \
         FROM session_goals sg \
         JOIN therapeutic_goals g ON g.id = sg.goal_id \
         WHERE sg.session_id = ?1 \
         ORDER BY g.title",
    )?;
    let rows = stmt.query_map(params![session_id], map_session_goal_row)?;
    rows.collect()
}

/// Sesiones donde se trabajó un objetivo, más recientes primero — incluye
/// sesiones archivadas si ya estaban vinculadas, mismo criterio que arriba.
pub fn list_for_goal(conn: &Connection, goal_id: &str) -> rusqlite::Result<Vec<GoalSessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.session_date, s.start_time, s.status, sg.progress_note \
         FROM session_goals sg \
         JOIN sessions s ON s.id = sg.session_id \
         WHERE sg.goal_id = ?1 \
         ORDER BY s.session_date DESC, COALESCE(s.start_time, '') DESC",
    )?;
    let rows = stmt.query_map(params![goal_id], map_goal_session_row)?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::goals::{self, NewGoalRow};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::sessions::{self, NewSessionRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-session-goals-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x23u8; VAULT_KEY_LEN]);
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

    fn create_test_session(conn: &Connection, patient_id: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        sessions::insert(
            conn,
            &NewSessionRow {
                id: &id,
                patient_id,
                appointment_id: None,
                session_date: "2026-09-01",
                start_time: Some("15:00"),
                duration_minutes: Some(50),
                modality: Some("presencial"),
                status: "programada",
            },
        )
        .unwrap();
        id
    }

    fn create_test_goal(conn: &Connection, patient_id: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        goals::insert(conn, &NewGoalRow { id: &id, patient_id, title: "Objetivo", description: None, status: "activo", target_date: None }).unwrap();
        id
    }

    #[test]
    fn links_and_lists_from_both_sides() {
        let conn = test_conn("link-both-sides");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let session_id = create_test_session(&conn, &patient_id);
        let goal_id = create_test_goal(&conn, &patient_id);

        assert!(!exists(&conn, &session_id, &goal_id).unwrap());
        link(&conn, &session_id, &goal_id, Some("Buen progreso")).unwrap();
        assert!(exists(&conn, &session_id, &goal_id).unwrap());

        let from_session = list_for_session(&conn, &session_id).unwrap();
        assert_eq!(from_session.len(), 1);
        assert_eq!(from_session[0].goal_id, goal_id);
        assert_eq!(from_session[0].progress_note.as_deref(), Some("Buen progreso"));

        let from_goal = list_for_goal(&conn, &goal_id).unwrap();
        assert_eq!(from_goal.len(), 1);
        assert_eq!(from_goal[0].session_id, session_id);
    }

    #[test]
    fn duplicate_link_violates_the_primary_key() {
        let conn = test_conn("duplicate-pk");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let session_id = create_test_session(&conn, &patient_id);
        let goal_id = create_test_goal(&conn, &patient_id);

        link(&conn, &session_id, &goal_id, None).unwrap();
        let err = link(&conn, &session_id, &goal_id, None).unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    #[test]
    fn unlink_removes_only_the_matching_pair() {
        let conn = test_conn("unlink");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let session_id = create_test_session(&conn, &patient_id);
        let goal_a = create_test_goal(&conn, &patient_id);
        let goal_b = create_test_goal(&conn, &patient_id);
        link(&conn, &session_id, &goal_a, None).unwrap();
        link(&conn, &session_id, &goal_b, None).unwrap();

        assert!(unlink(&conn, &session_id, &goal_a).unwrap());
        assert!(!exists(&conn, &session_id, &goal_a).unwrap());
        assert!(exists(&conn, &session_id, &goal_b).unwrap());
    }

    #[test]
    fn unlinking_a_nonexistent_pair_reports_nothing_changed() {
        let conn = test_conn("unlink-noop");
        assert!(!unlink(&conn, "no-existe", "tampoco").unwrap());
    }

    #[test]
    fn updates_the_progress_note_of_an_existing_link() {
        let conn = test_conn("update-progress");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let session_id = create_test_session(&conn, &patient_id);
        let goal_id = create_test_goal(&conn, &patient_id);
        link(&conn, &session_id, &goal_id, None).unwrap();

        assert!(update_progress_note(&conn, &session_id, &goal_id, Some("Actualizado")).unwrap());
        let from_session = list_for_session(&conn, &session_id).unwrap();
        assert_eq!(from_session[0].progress_note.as_deref(), Some("Actualizado"));
    }

    #[test]
    fn a_session_with_multiple_goals_lists_all_of_them() {
        let conn = test_conn("multi-goals");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let session_id = create_test_session(&conn, &patient_id);
        let goal_a = create_test_goal(&conn, &patient_id);
        let goal_b = create_test_goal(&conn, &patient_id);
        link(&conn, &session_id, &goal_a, None).unwrap();
        link(&conn, &session_id, &goal_b, None).unwrap();

        assert_eq!(list_for_session(&conn, &session_id).unwrap().len(), 2);
    }

    #[test]
    fn a_goal_linked_to_multiple_sessions_lists_all_of_them() {
        let conn = test_conn("multi-sessions");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let session_a = create_test_session(&conn, &patient_id);
        let session_b = create_test_session(&conn, &patient_id);
        let goal_id = create_test_goal(&conn, &patient_id);
        link(&conn, &session_a, &goal_id, None).unwrap();
        link(&conn, &session_b, &goal_id, None).unwrap();

        assert_eq!(list_for_goal(&conn, &goal_id).unwrap().len(), 2);
    }
}
