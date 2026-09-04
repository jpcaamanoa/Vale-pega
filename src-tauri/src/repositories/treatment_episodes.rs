//! Acceso a datos de `treatment_episodes`. SQL puro — sin reglas de
//! negocio (eso vive en `services::treatment_episodes`) y sin ninguna
//! noción de si el vault está desbloqueado.
//!
//! Deliberadamente pequeña (Fase 9): solo `started_at`/`status`. Nada de
//! `reason_for_end`/`closure_summary`/`recommendations` — eso pertenece a
//! la futura Fase 10 (Cierre/Alta). Ver `docs/treatment-episodes.md`.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreatmentEpisode {
    pub id: String,
    pub patient_id: String,
    pub started_at: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

pub struct NewTreatmentEpisodeRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub started_at: &'a str,
    pub status: &'a str,
}

const EPISODE_COLUMNS: &str = "id, patient_id, started_at, status, created_at, updated_at, deleted_at";

fn map_row(row: &Row) -> rusqlite::Result<TreatmentEpisode> {
    Ok(TreatmentEpisode {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        started_at: row.get(2)?,
        status: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        deleted_at: row.get(6)?,
    })
}

pub fn insert(conn: &Connection, row: &NewTreatmentEpisodeRow) -> rusqlite::Result<TreatmentEpisode> {
    conn.execute(
        "INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES (?1, ?2, ?3, ?4)",
        params![row.id, row.patient_id, row.started_at, row.status],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

/// Devuelve el proceso exista o no `deleted_at` — igual criterio que
/// `repositories::sessions::find_by_id`.
pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<TreatmentEpisode>> {
    conn.query_row(&format!("SELECT {EPISODE_COLUMNS} FROM treatment_episodes WHERE id = ?1"), params![id], map_row).optional()
}

/// El proceso activo (no archivado) de un paciente, si existe. Nunca hay
/// más de uno — garantizado por `idx_treatment_episodes_one_active_per_patient`.
pub fn find_active_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Option<TreatmentEpisode>> {
    conn.query_row(
        &format!("SELECT {EPISODE_COLUMNS} FROM treatment_episodes WHERE patient_id = ?1 AND status = 'activo' AND deleted_at IS NULL"),
        params![patient_id],
        map_row,
    )
    .optional()
}

fn list(conn: &Connection, patient_id: &str, deleted: bool) -> rusqlite::Result<Vec<TreatmentEpisode>> {
    let deleted_clause = if deleted { "deleted_at IS NOT NULL" } else { "deleted_at IS NULL" };
    let sql = format!("SELECT {EPISODE_COLUMNS} FROM treatment_episodes WHERE patient_id = ?1 AND {deleted_clause} ORDER BY started_at DESC, created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], map_row)?;
    rows.collect()
}

pub fn list_active_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<TreatmentEpisode>> {
    list(conn, patient_id, false)
}

pub fn list_archived_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<TreatmentEpisode>> {
    list(conn, patient_id, true)
}

/// Solo cambia `status` — nunca `started_at` ni ningún otro campo. La
/// capa de servicio decide qué transiciones son válidas; este módulo solo
/// ejecuta el `UPDATE`.
pub fn set_status(conn: &Connection, id: &str, status: &str) -> rusqlite::Result<Option<TreatmentEpisode>> {
    let affected = conn.execute("UPDATE treatment_episodes SET status = ?1 WHERE id = ?2 AND deleted_at IS NULL", params![status, id])?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

pub fn soft_delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE treatment_episodes SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("UPDATE treatment_episodes SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-episodes-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x42u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_patient(conn: &Connection, name: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        patients::insert(
            conn,
            &NewPatientRow {
                id: &id, full_name: name, preferred_name: None, rut: None, birth_date: None, phone: None, email: None,
                address: None, emergency_contact_name: None, emergency_contact_phone: None, emergency_contact_relationship: None,
                status: "activo", referred_by: None, intake_date: None, region: None, commune: None,
            },
        )
        .unwrap();
        id
    }

    #[test]
    fn inserts_and_finds_an_episode() {
        let conn = test_conn("insert-find");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let episode = insert(&conn, &NewTreatmentEpisodeRow { id: "ep1", patient_id: &patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        assert_eq!(episode.patient_id, patient_id);
        assert_eq!(episode.status, "activo");
        assert!(episode.deleted_at.is_none());

        let found = find_by_id(&conn, "ep1").unwrap().unwrap();
        assert_eq!(found.started_at, "2026-01-01");
    }

    #[test]
    fn find_by_id_returns_none_for_nonexistent() {
        let conn = test_conn("find-none");
        assert!(find_by_id(&conn, "no-existe").unwrap().is_none());
    }

    #[test]
    fn find_active_by_patient_returns_none_when_no_active_episode() {
        let conn = test_conn("find-active-none");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        assert!(find_active_by_patient(&conn, &patient_id).unwrap().is_none());
    }

    #[test]
    fn find_active_by_patient_returns_the_active_episode() {
        let conn = test_conn("find-active-some");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep1", patient_id: &patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        let found = find_active_by_patient(&conn, &patient_id).unwrap().unwrap();
        assert_eq!(found.id, "ep1");
    }

    #[test]
    fn find_active_by_patient_ignores_paused_and_closed() {
        let conn = test_conn("find-active-ignores");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep1", patient_id: &patient_id, started_at: "2026-01-01", status: "pausado" }).unwrap();
        assert!(find_active_by_patient(&conn, &patient_id).unwrap().is_none());
    }

    #[test]
    fn list_active_excludes_archived_and_list_archived_excludes_active() {
        let conn = test_conn("list-active-archived");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep1", patient_id: &patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep2", patient_id: &patient_id, started_at: "2025-01-01", status: "pausado" }).unwrap();
        soft_delete(&conn, "ep2").unwrap();

        let active = list_active_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "ep1");

        let archived = list_archived_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, "ep2");
    }

    #[test]
    fn list_orders_by_started_at_descending() {
        let conn = test_conn("list-order");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep1", patient_id: &patient_id, started_at: "2024-01-01", status: "pausado" }).unwrap();
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep2", patient_id: &patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        let all = list_active_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(all[0].id, "ep2");
        assert_eq!(all[1].id, "ep1");
    }

    #[test]
    fn set_status_changes_status_and_touches_updated_at() {
        let conn = test_conn("set-status");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let created = insert(&conn, &NewTreatmentEpisodeRow { id: "ep1", patient_id: &patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let updated = set_status(&conn, "ep1", "pausado").unwrap().unwrap();
        assert_eq!(updated.status, "pausado");
        assert!(updated.updated_at >= created.updated_at);
    }

    #[test]
    fn set_status_on_nonexistent_episode_returns_none() {
        let conn = test_conn("set-status-none");
        assert!(set_status(&conn, "no-existe", "pausado").unwrap().is_none());
    }

    #[test]
    fn set_status_on_archived_episode_returns_none() {
        let conn = test_conn("set-status-archived");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep1", patient_id: &patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        soft_delete(&conn, "ep1").unwrap();
        assert!(set_status(&conn, "ep1", "pausado").unwrap().is_none());
    }

    #[test]
    fn soft_delete_and_restore_roundtrip() {
        let conn = test_conn("soft-delete-restore");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep1", patient_id: &patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        assert!(soft_delete(&conn, "ep1").unwrap());
        assert!(find_by_id(&conn, "ep1").unwrap().unwrap().deleted_at.is_some());
        assert!(restore(&conn, "ep1").unwrap());
        assert!(find_by_id(&conn, "ep1").unwrap().unwrap().deleted_at.is_none());
    }

    #[test]
    fn soft_deleting_an_episode_frees_the_active_slot_for_a_new_one() {
        let conn = test_conn("soft-delete-frees-slot");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep1", patient_id: &patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        soft_delete(&conn, "ep1").unwrap();
        // Con ep1 archivado, un segundo proceso 'activo' para el mismo
        // paciente ya no viola el índice único parcial.
        insert(&conn, &NewTreatmentEpisodeRow { id: "ep2", patient_id: &patient_id, started_at: "2026-02-01", status: "activo" }).unwrap();
        assert!(find_active_by_patient(&conn, &patient_id).unwrap().is_some());
    }
}
