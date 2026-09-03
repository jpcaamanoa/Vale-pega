//! Acceso a datos de `goal_indicators`. SQL puro. A diferencia de
//! `therapeutic_goals`, esta tabla no tiene `deleted_at` ni columnas de
//! fecha en `SCHEMA_V1` — eliminar un indicador es un `DELETE` real, no un
//! soft delete. Esto es una decisión ya tomada por el esquema, no de esta
//! capa (ver `docs/goals.md`).

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalIndicator {
    pub id: String,
    pub goal_id: String,
    pub description: String,
    pub baseline_value: Option<String>,
    pub target_value: Option<String>,
}

pub struct NewGoalIndicatorRow<'a> {
    pub id: &'a str,
    pub goal_id: &'a str,
    pub description: &'a str,
    pub baseline_value: Option<&'a str>,
    pub target_value: Option<&'a str>,
}

pub struct GoalIndicatorUpdateRow<'a> {
    pub description: &'a str,
    pub baseline_value: Option<&'a str>,
    pub target_value: Option<&'a str>,
}

const INDICATOR_COLUMNS: &str = "id, goal_id, description, baseline_value, target_value";

fn map_row(row: &Row) -> rusqlite::Result<GoalIndicator> {
    Ok(GoalIndicator { id: row.get(0)?, goal_id: row.get(1)?, description: row.get(2)?, baseline_value: row.get(3)?, target_value: row.get(4)? })
}

pub fn insert(conn: &Connection, row: &NewGoalIndicatorRow) -> rusqlite::Result<GoalIndicator> {
    conn.execute(
        "INSERT INTO goal_indicators (id, goal_id, description, baseline_value, target_value) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![row.id, row.goal_id, row.description, row.baseline_value, row.target_value],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<GoalIndicator>> {
    conn.query_row(&format!("SELECT {INDICATOR_COLUMNS} FROM goal_indicators WHERE id = ?1"), params![id], map_row).optional()
}

pub fn list_by_goal(conn: &Connection, goal_id: &str) -> rusqlite::Result<Vec<GoalIndicator>> {
    let mut stmt = conn.prepare(&format!("SELECT {INDICATOR_COLUMNS} FROM goal_indicators WHERE goal_id = ?1 ORDER BY rowid"))?;
    let rows = stmt.query_map(params![goal_id], map_row)?;
    rows.collect()
}

pub fn update(conn: &Connection, id: &str, row: &GoalIndicatorUpdateRow) -> rusqlite::Result<Option<GoalIndicator>> {
    let affected = conn.execute(
        "UPDATE goal_indicators SET description = ?1, baseline_value = ?2, target_value = ?3 WHERE id = ?4",
        params![row.description, row.baseline_value, row.target_value, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

/// Borrado real (no hay `deleted_at` en esta tabla) — ver nota de módulo.
pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM goal_indicators WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::goals::{self, NewGoalRow};
    use crate::repositories::patients::{self, NewPatientRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-goal-indicators-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x22u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_goal(conn: &Connection) -> String {
        let patient_id = uuid::Uuid::new_v4().to_string();
        patients::insert(
            conn,
            &NewPatientRow {
                id: &patient_id,
                full_name: "Paciente de prueba",
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
        let goal_id = uuid::Uuid::new_v4().to_string();
        goals::insert(
            conn,
            &NewGoalRow { id: &goal_id, patient_id: &patient_id, title: "Objetivo de prueba", description: None, status: "activo", target_date: None },
        )
        .unwrap();
        goal_id
    }

    #[test]
    fn inserts_and_finds_an_indicator() {
        let conn = test_conn("insert-find");
        let goal_id = create_test_goal(&conn);
        let i = insert(
            &conn,
            &NewGoalIndicatorRow { id: "i1", goal_id: &goal_id, description: "Frecuencia de crisis", baseline_value: Some("3/semana"), target_value: Some("0/semana") },
        )
        .unwrap();
        assert_eq!(i.goal_id, goal_id);
        assert_eq!(i.description, "Frecuencia de crisis");
        assert_eq!(find_by_id(&conn, "i1").unwrap().unwrap().id, "i1");
    }

    #[test]
    fn lists_indicators_of_a_goal_in_insertion_order() {
        let conn = test_conn("list");
        let goal_id = create_test_goal(&conn);
        insert(&conn, &NewGoalIndicatorRow { id: "i1", goal_id: &goal_id, description: "Primero", baseline_value: None, target_value: None }).unwrap();
        insert(&conn, &NewGoalIndicatorRow { id: "i2", goal_id: &goal_id, description: "Segundo", baseline_value: None, target_value: None }).unwrap();

        let list = list_by_goal(&conn, &goal_id).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].description, "Primero");
        assert_eq!(list[1].description, "Segundo");
    }

    #[test]
    fn updates_an_indicator() {
        let conn = test_conn("update");
        let goal_id = create_test_goal(&conn);
        insert(&conn, &NewGoalIndicatorRow { id: "i1", goal_id: &goal_id, description: "Original", baseline_value: None, target_value: None }).unwrap();

        let updated = update(&conn, "i1", &GoalIndicatorUpdateRow { description: "Editado", baseline_value: Some("2"), target_value: Some("0") })
            .unwrap()
            .unwrap();
        assert_eq!(updated.description, "Editado");
        assert_eq!(updated.baseline_value.as_deref(), Some("2"));
    }

    #[test]
    fn updating_an_unknown_indicator_reports_nothing_changed() {
        let conn = test_conn("update-unknown");
        let result = update(&conn, "no-existe", &GoalIndicatorUpdateRow { description: "x", baseline_value: None, target_value: None }).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn deletes_an_indicator() {
        let conn = test_conn("delete");
        let goal_id = create_test_goal(&conn);
        insert(&conn, &NewGoalIndicatorRow { id: "i1", goal_id: &goal_id, description: "A borrar", baseline_value: None, target_value: None }).unwrap();

        assert!(delete(&conn, "i1").unwrap());
        assert!(find_by_id(&conn, "i1").unwrap().is_none());
        assert!(list_by_goal(&conn, &goal_id).unwrap().is_empty());
    }

    #[test]
    fn deleting_an_unknown_indicator_reports_nothing_changed() {
        let conn = test_conn("delete-unknown");
        assert!(!delete(&conn, "no-existe").unwrap());
    }
}
