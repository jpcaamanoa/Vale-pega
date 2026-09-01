//! Acceso a datos de `therapeutic_goals`. SQL puro — sin reglas de negocio
//! (eso vive en `services::goals`) y sin ninguna noción de si el vault está
//! desbloqueado.
//!
//! `formulation_id` se mapea porque la columna existe en `SCHEMA_V1`, pero
//! esta fase nunca la escribe (siempre `NULL` al insertar) — Formulación no
//! se implementa aquí, ver `docs/goals.md`.
//!
//! El listado (`GoalListItem`) no lleva `description` — solo lo necesario
//! para una lista y para saber cuántos indicadores/sesiones tiene, nunca el
//! contenido clínico completo. Mismo criterio de minimización que
//! `repositories::sessions::SessionListItem`.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: String,
    pub patient_id: String,
    pub formulation_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub target_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalListItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub target_date: Option<String>,
    pub indicator_count: i64,
    pub session_count: i64,
}

pub struct NewGoalRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub status: &'a str,
    pub target_date: Option<&'a str>,
}

/// Campos editables. Deliberadamente sin `patientId`, igual que
/// `SessionMetadataUpdateRow`: reasignar un objetivo a otro paciente no es
/// una operación de este MVP.
pub struct GoalUpdateRow<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub status: &'a str,
    pub target_date: Option<&'a str>,
}

const GOAL_COLUMNS: &str =
    "id, patient_id, formulation_id, title, description, status, target_date, created_at, updated_at, deleted_at";

fn map_row(row: &Row) -> rusqlite::Result<Goal> {
    Ok(Goal {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        formulation_id: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        status: row.get(5)?,
        target_date: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        deleted_at: row.get(9)?,
    })
}

pub fn insert(conn: &Connection, row: &NewGoalRow) -> rusqlite::Result<Goal> {
    conn.execute(
        "INSERT INTO therapeutic_goals (id, patient_id, title, description, status, target_date) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![row.id, row.patient_id, row.title, row.description, row.status, row.target_date],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

/// Devuelve el objetivo exista o no `deleted_at` — igual criterio que
/// `repositories::sessions::find_by_id`: archivado no es lo mismo que
/// inexistente, y la capa de servicio decide qué hacer con cada caso.
pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<Goal>> {
    conn.query_row(&format!("SELECT {GOAL_COLUMNS} FROM therapeutic_goals WHERE id = ?1"), params![id], map_row).optional()
}

fn list(conn: &Connection, patient_id: &str, deleted: bool) -> rusqlite::Result<Vec<GoalListItem>> {
    let deleted_clause = if deleted { "g.deleted_at IS NOT NULL" } else { "g.deleted_at IS NULL" };
    let sql = format!(
        "SELECT g.id, g.title, g.status, g.target_date, \
         (SELECT COUNT(*) FROM goal_indicators gi WHERE gi.goal_id = g.id), \
         (SELECT COUNT(*) FROM session_goals sg WHERE sg.goal_id = g.id) \
         FROM therapeutic_goals g \
         WHERE g.patient_id = ?1 AND {deleted_clause} \
         ORDER BY g.created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], |row| {
        Ok(GoalListItem {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            target_date: row.get(3)?,
            indicator_count: row.get(4)?,
            session_count: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn list_active_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<GoalListItem>> {
    list(conn, patient_id, false)
}

pub fn list_deleted_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<GoalListItem>> {
    list(conn, patient_id, true)
}

pub fn update(conn: &Connection, id: &str, row: &GoalUpdateRow) -> rusqlite::Result<Option<Goal>> {
    let affected = conn.execute(
        "UPDATE therapeutic_goals SET title = ?1, description = ?2, status = ?3, target_date = ?4 \
         WHERE id = ?5 AND deleted_at IS NULL",
        params![row.title, row.description, row.status, row.target_date, id],
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
        "UPDATE therapeutic_goals SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("UPDATE therapeutic_goals SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-goals-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x21u8; VAULT_KEY_LEN]);
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
    fn inserts_and_finds_a_goal() {
        let conn = test_conn("insert-find");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let g = insert(
            &conn,
            &NewGoalRow { id: "g1", patient_id: &patient_id, title: "Reducir ansiedad", description: None, status: "activo", target_date: None },
        )
        .unwrap();
        assert_eq!(g.patient_id, patient_id);
        assert_eq!(g.title, "Reducir ansiedad");
        assert!(g.formulation_id.is_none());
        assert_eq!(find_by_id(&conn, "g1").unwrap().unwrap().id, "g1");
    }

    #[test]
    fn find_by_id_returns_none_for_unknown_id() {
        let conn = test_conn("find-unknown");
        assert!(find_by_id(&conn, "no-existe").unwrap().is_none());
    }

    #[test]
    fn list_active_and_deleted_are_mutually_exclusive() {
        let conn = test_conn("list-active-deleted");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        insert(&conn, &NewGoalRow { id: "g1", patient_id: &patient_id, title: "Objetivo", description: None, status: "activo", target_date: None })
            .unwrap();

        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 1);
        assert_eq!(list_deleted_by_patient(&conn, &patient_id).unwrap().len(), 0);

        assert!(soft_delete(&conn, "g1").unwrap());

        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 0);
        assert_eq!(list_deleted_by_patient(&conn, &patient_id).unwrap().len(), 1);

        assert!(restore(&conn, "g1").unwrap());
        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 1);
    }

    #[test]
    fn list_item_reports_indicator_and_session_counts() {
        let conn = test_conn("list-counts");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        insert(&conn, &NewGoalRow { id: "g1", patient_id: &patient_id, title: "Objetivo", description: None, status: "activo", target_date: None })
            .unwrap();
        conn.execute(
            "INSERT INTO goal_indicators (id, goal_id, description) VALUES ('i1', 'g1', 'Indicador uno')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO goal_indicators (id, goal_id, description) VALUES ('i2', 'g1', 'Indicador dos')",
            [],
        )
        .unwrap();

        let items = list_active_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(items[0].indicator_count, 2);
        assert_eq!(items[0].session_count, 0);
    }

    #[test]
    fn update_changes_fields_but_not_patient() {
        let conn = test_conn("update");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        insert(&conn, &NewGoalRow { id: "g1", patient_id: &patient_id, title: "Original", description: None, status: "activo", target_date: None })
            .unwrap();

        let updated = update(
            &conn,
            "g1",
            &GoalUpdateRow { title: "Editado", description: Some("Detalle"), status: "pausado", target_date: Some("2026-12-01") },
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.title, "Editado");
        assert_eq!(updated.status, "pausado");
        assert_eq!(updated.patient_id, patient_id);
    }

    #[test]
    fn update_on_archived_goal_does_nothing() {
        let conn = test_conn("update-archived");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        insert(&conn, &NewGoalRow { id: "g1", patient_id: &patient_id, title: "Original", description: None, status: "activo", target_date: None })
            .unwrap();
        soft_delete(&conn, "g1").unwrap();

        let result = update(&conn, "g1", &GoalUpdateRow { title: "Editado", description: None, status: "activo", target_date: None }).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn restoring_a_never_archived_goal_reports_nothing_changed() {
        let conn = test_conn("restore-noop");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        insert(&conn, &NewGoalRow { id: "g1", patient_id: &patient_id, title: "Objetivo", description: None, status: "activo", target_date: None })
            .unwrap();
        assert!(!restore(&conn, "g1").unwrap());
    }
}
