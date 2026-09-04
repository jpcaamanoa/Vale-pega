//! Acceso a datos de `patient_prep_notes` (Fase 8). SQL puro — sin reglas de
//! negocio (eso vive en `services::patient_prep_notes`).
//!
//! Deliberadamente sin `deleted_at` — ver la nota de diseño en
//! `src-tauri/src/db/migrations.rs` junto a `SCHEMA_V3` y
//! `docs/session-continuity.md`: el ciclo de vida completo de una
//! preparación ya queda representado por su propio `status`
//! (`pendiente`/`abordado`/`descartado`).

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatientPrepNote {
    pub id: String,
    pub patient_id: String,
    pub origin_session_id: Option<String>,
    pub content: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewPrepNoteRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub origin_session_id: Option<&'a str>,
    pub content: &'a str,
}

const PREP_NOTE_COLUMNS: &str = "id, patient_id, origin_session_id, content, status, created_at, updated_at";

fn map_row(row: &Row) -> rusqlite::Result<PatientPrepNote> {
    Ok(PatientPrepNote {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        origin_session_id: row.get(2)?,
        content: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn insert(conn: &Connection, row: &NewPrepNoteRow) -> rusqlite::Result<PatientPrepNote> {
    conn.execute(
        "INSERT INTO patient_prep_notes (id, patient_id, origin_session_id, content) VALUES (?1, ?2, ?3, ?4)",
        params![row.id, row.patient_id, row.origin_session_id, row.content],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<PatientPrepNote>> {
    conn.query_row(&format!("SELECT {PREP_NOTE_COLUMNS} FROM patient_prep_notes WHERE id = ?1"), params![id], map_row).optional()
}

/// Todas las preparaciones de un paciente, sin importar su estado —
/// historial completo, más reciente primero.
pub fn list_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<PatientPrepNote>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PREP_NOTE_COLUMNS} FROM patient_prep_notes WHERE patient_id = ?1 ORDER BY created_at DESC"
    ))?;
    let rows = stmt.query_map(params![patient_id], map_row)?;
    rows.collect()
}

/// Únicamente las que siguen `pendiente` — lo que se muestra al abrir una
/// sesión nueva y en el panel de continuidad de la ficha del paciente.
pub fn list_pending_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<PatientPrepNote>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PREP_NOTE_COLUMNS} FROM patient_prep_notes WHERE patient_id = ?1 AND status = 'pendiente' ORDER BY created_at ASC"
    ))?;
    let rows = stmt.query_map(params![patient_id], map_row)?;
    rows.collect()
}

/// Edita el contenido. Solo tiene efecto si la preparación sigue
/// `pendiente` — una vez resuelta (abordada/descartada) el contenido queda
/// congelado, para no reescribir en silencio lo que efectivamente se
/// decidió en su momento.
pub fn update_content(conn: &Connection, id: &str, content: &str) -> rusqlite::Result<Option<PatientPrepNote>> {
    let affected = conn.execute(
        "UPDATE patient_prep_notes SET content = ?1 WHERE id = ?2 AND status = 'pendiente'",
        params![content, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

/// Cambia el estado. Transición libre entre los tres valores — igual
/// criterio que `therapeutic_goals` (`logrado` no es terminal): "abordado"
/// y "descartado" tampoco lo son, se puede volver a "pendiente" si la
/// profesional reconsidera.
pub fn set_status(conn: &Connection, id: &str, status: &str) -> rusqlite::Result<Option<PatientPrepNote>> {
    let affected = conn.execute("UPDATE patient_prep_notes SET status = ?1 WHERE id = ?2", params![status, id])?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::sessions::{self, NewSessionRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-prep-notes-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x31u8; VAULT_KEY_LEN]);
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
                region: None,
                commune: None,
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
                start_time: None,
                duration_minutes: None,
                modality: None,
                status: "programada",
            },
        )
        .unwrap();
        id
    }

    #[test]
    fn inserts_and_finds_a_prep_note_defaulting_to_pendiente() {
        let conn = test_conn("insert-find");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let n = insert(&conn, &NewPrepNoteRow { id: "n1", patient_id: &patient_id, origin_session_id: None, content: "Retomar exposición" }).unwrap();
        assert_eq!(n.status, "pendiente");
        assert_eq!(n.content, "Retomar exposición");
        assert!(n.origin_session_id.is_none());
        assert_eq!(find_by_id(&conn, "n1").unwrap().unwrap().id, "n1");
    }

    #[test]
    fn find_by_id_returns_none_for_unknown_id() {
        let conn = test_conn("find-unknown");
        assert!(find_by_id(&conn, "no-existe").unwrap().is_none());
    }

    #[test]
    fn insert_with_origin_session_persists_it() {
        let conn = test_conn("insert-with-session");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let session_id = create_test_session(&conn, &patient_id);
        let n = insert(&conn, &NewPrepNoteRow { id: "n1", patient_id: &patient_id, origin_session_id: Some(&session_id), content: "Nota" }).unwrap();
        assert_eq!(n.origin_session_id.as_deref(), Some(session_id.as_str()));
    }

    #[test]
    fn list_pending_excludes_resolved_notes() {
        let conn = test_conn("list-pending");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        insert(&conn, &NewPrepNoteRow { id: "n1", patient_id: &patient_id, origin_session_id: None, content: "Uno" }).unwrap();
        insert(&conn, &NewPrepNoteRow { id: "n2", patient_id: &patient_id, origin_session_id: None, content: "Dos" }).unwrap();
        set_status(&conn, "n2", "abordado").unwrap();

        let pending = list_pending_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "n1");

        let all = list_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(all.len(), 2, "el historial completo conserva la nota resuelta");
    }

    #[test]
    fn update_content_only_works_while_pending() {
        let conn = test_conn("update-content");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        insert(&conn, &NewPrepNoteRow { id: "n1", patient_id: &patient_id, origin_session_id: None, content: "Original" }).unwrap();

        let updated = update_content(&conn, "n1", "Editado").unwrap().unwrap();
        assert_eq!(updated.content, "Editado");

        set_status(&conn, "n1", "abordado").unwrap();
        let result = update_content(&conn, "n1", "Ya no debería aplicarse").unwrap();
        assert!(result.is_none(), "una nota resuelta no debe poder editarse en su contenido");
        assert_eq!(find_by_id(&conn, "n1").unwrap().unwrap().content, "Editado");
    }

    #[test]
    fn set_status_allows_reopening_from_abordado_back_to_pendiente() {
        let conn = test_conn("reopen");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        insert(&conn, &NewPrepNoteRow { id: "n1", patient_id: &patient_id, origin_session_id: None, content: "Nota" }).unwrap();
        set_status(&conn, "n1", "abordado").unwrap();
        let reopened = set_status(&conn, "n1", "pendiente").unwrap().unwrap();
        assert_eq!(reopened.status, "pendiente");
    }

    #[test]
    fn set_status_on_unknown_id_reports_nothing_changed() {
        let conn = test_conn("set-status-unknown");
        assert!(set_status(&conn, "no-existe", "abordado").unwrap().is_none());
    }
}
