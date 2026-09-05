//! Acceso a datos de `episode_closures`. SQL puro — sin reglas de negocio
//! (eso vive en `services::episode_closures`) y sin ninguna noción de si el
//! vault está desbloqueado.
//!
//! Inmutable tras crearse (Fase 11): este módulo nunca expone una función
//! `update` sobre el contenido del cierre — solo `insert` y `revert` (que
//! únicamente puede escribir `reverted_at`/`reverted_reason`). Corregir un
//! error de fondo es responsabilidad de la capa de servicio, vía anular +
//! crear un cierre nuevo. Ver `docs/episode-closure.md`.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeClosure {
    pub id: String,
    pub episode_id: String,
    pub closed_at: String,
    pub reason: String,
    pub reason_detail: Option<String>,
    pub outcome: String,
    pub summary: Option<String>,
    pub recommendations: Option<String>,
    pub reverted_at: Option<String>,
    pub reverted_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewEpisodeClosureRow<'a> {
    pub id: &'a str,
    pub episode_id: &'a str,
    pub closed_at: &'a str,
    pub reason: &'a str,
    pub reason_detail: Option<&'a str>,
    pub outcome: &'a str,
    pub summary: Option<&'a str>,
    pub recommendations: Option<&'a str>,
}

const CLOSURE_COLUMNS: &str = "id, episode_id, closed_at, reason, reason_detail, outcome, summary, recommendations, \
     reverted_at, reverted_reason, created_at, updated_at";

fn map_row(row: &Row) -> rusqlite::Result<EpisodeClosure> {
    Ok(EpisodeClosure {
        id: row.get(0)?,
        episode_id: row.get(1)?,
        closed_at: row.get(2)?,
        reason: row.get(3)?,
        reason_detail: row.get(4)?,
        outcome: row.get(5)?,
        summary: row.get(6)?,
        recommendations: row.get(7)?,
        reverted_at: row.get(8)?,
        reverted_reason: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

/// Falla con `SQLITE_CONSTRAINT` si ya existe un cierre vigente (no anulado)
/// para ese proceso — `idx_episode_closures_active` lo garantiza también a
/// nivel de base de datos; la capa de servicio nunca debería dejar llegar
/// ese caso hasta aquí, pero el índice queda como último recurso.
pub fn insert(conn: &Connection, row: &NewEpisodeClosureRow) -> rusqlite::Result<EpisodeClosure> {
    conn.execute(
        "INSERT INTO episode_closures (id, episode_id, closed_at, reason, reason_detail, outcome, summary, recommendations) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![row.id, row.episode_id, row.closed_at, row.reason, row.reason_detail, row.outcome, row.summary, row.recommendations],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<EpisodeClosure>> {
    conn.query_row(&format!("SELECT {CLOSURE_COLUMNS} FROM episode_closures WHERE id = ?1"), params![id], map_row).optional()
}

/// El cierre vigente (no anulado) de un proceso, si existe. Nunca hay más
/// de uno — garantizado por `idx_episode_closures_active`.
pub fn find_active_by_episode(conn: &Connection, episode_id: &str) -> rusqlite::Result<Option<EpisodeClosure>> {
    conn.query_row(
        &format!("SELECT {CLOSURE_COLUMNS} FROM episode_closures WHERE episode_id = ?1 AND reverted_at IS NULL"),
        params![episode_id],
        map_row,
    )
    .optional()
}

/// Todo el historial de cierres de un proceso — vigente y anulados — más
/// reciente primero. Nunca se borra ni se filtra nada: es el registro
/// auditable completo.
pub fn list_history_by_episode(conn: &Connection, episode_id: &str) -> rusqlite::Result<Vec<EpisodeClosure>> {
    let sql = format!("SELECT {CLOSURE_COLUMNS} FROM episode_closures WHERE episode_id = ?1 ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![episode_id], map_row)?;
    rows.collect()
}

/// Marca un cierre como anulado. Nunca borra ni modifica ningún otro campo
/// de la fila original — el motivo/resumen/resultado del cierre anulado
/// permanece exactamente como se registró. Falla en silencio (devuelve
/// `Ok(None)`) si el cierre no existe o ya estaba anulado — la capa de
/// servicio decide cómo reportar cada caso por separado.
pub fn revert(conn: &Connection, id: &str, reverted_reason: &str) -> rusqlite::Result<Option<EpisodeClosure>> {
    let affected = conn.execute(
        "UPDATE episode_closures SET reverted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), reverted_reason = ?1 \
         WHERE id = ?2 AND reverted_at IS NULL",
        params![reverted_reason, id],
    )?;
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
    use crate::repositories::treatment_episodes::{self, NewTreatmentEpisodeRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-closures-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x46u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_episode(conn: &Connection, patient_name: &str, status: &str) -> String {
        let patient_id = uuid::Uuid::new_v4().to_string();
        patients::insert(
            conn,
            &NewPatientRow {
                id: &patient_id, full_name: patient_name, preferred_name: None, rut: None, birth_date: None, phone: None, email: None,
                address: None, emergency_contact_name: None, emergency_contact_phone: None, emergency_contact_relationship: None,
                status: "activo", referred_by: None, intake_date: None, region: None, commune: None,
            },
        )
        .unwrap();
        let episode_id = uuid::Uuid::new_v4().to_string();
        treatment_episodes::insert(conn, &NewTreatmentEpisodeRow { id: &episode_id, patient_id: &patient_id, started_at: "2026-01-01", status }).unwrap();
        episode_id
    }

    fn minimal_closure_row<'a>(id: &'a str, episode_id: &'a str) -> NewEpisodeClosureRow<'a> {
        NewEpisodeClosureRow { id, episode_id, closed_at: "2026-02-01", reason: "alta", reason_detail: None, outcome: "objetivos_logrados", summary: None, recommendations: None }
    }

    #[test]
    fn inserts_and_finds_a_closure() {
        let conn = test_conn("insert-find");
        let episode_id = create_test_episode(&conn, "Paciente Uno", "cerrado");
        let closure = insert(&conn, &minimal_closure_row("c1", &episode_id)).unwrap();
        assert_eq!(closure.reason, "alta");
        assert!(closure.reverted_at.is_none());
        let found = find_by_id(&conn, "c1").unwrap().unwrap();
        assert_eq!(found.outcome, "objetivos_logrados");
    }

    #[test]
    fn find_active_by_episode_returns_none_when_no_closure_exists() {
        let conn = test_conn("find-active-none");
        let episode_id = create_test_episode(&conn, "Paciente Dos", "activo");
        assert!(find_active_by_episode(&conn, &episode_id).unwrap().is_none());
    }

    #[test]
    fn find_active_by_episode_ignores_reverted_closures() {
        let conn = test_conn("find-active-ignores-reverted");
        let episode_id = create_test_episode(&conn, "Paciente Tres", "cerrado");
        insert(&conn, &minimal_closure_row("c1", &episode_id)).unwrap();
        revert(&conn, "c1", "Cerrado por error").unwrap();
        assert!(find_active_by_episode(&conn, &episode_id).unwrap().is_none());
    }

    #[test]
    fn a_second_active_insert_for_the_same_episode_violates_the_unique_index() {
        let conn = test_conn("duplicate-active");
        let episode_id = create_test_episode(&conn, "Paciente Cuatro", "cerrado");
        insert(&conn, &minimal_closure_row("c1", &episode_id)).unwrap();
        let err = insert(&conn, &minimal_closure_row("c2", &episode_id)).unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    #[test]
    fn revert_marks_reverted_without_deleting_the_original_content() {
        let conn = test_conn("revert-preserves");
        let episode_id = create_test_episode(&conn, "Paciente Cinco", "cerrado");
        insert(&conn, &minimal_closure_row("c1", &episode_id)).unwrap();
        let reverted = revert(&conn, "c1", "Cerrado por error").unwrap().unwrap();
        assert!(reverted.reverted_at.is_some());
        assert_eq!(reverted.reverted_reason.as_deref(), Some("Cerrado por error"));
        // El contenido original del cierre sigue exactamente igual.
        assert_eq!(reverted.reason, "alta");
        assert_eq!(reverted.outcome, "objetivos_logrados");
    }

    #[test]
    fn reverting_an_already_reverted_closure_returns_none() {
        let conn = test_conn("revert-twice");
        let episode_id = create_test_episode(&conn, "Paciente Seis", "cerrado");
        insert(&conn, &minimal_closure_row("c1", &episode_id)).unwrap();
        revert(&conn, "c1", "Primer motivo").unwrap();
        assert!(revert(&conn, "c1", "Segundo motivo").unwrap().is_none());
    }

    #[test]
    fn reverting_a_nonexistent_closure_returns_none() {
        let conn = test_conn("revert-nonexistent");
        assert!(revert(&conn, "no-existe", "motivo").unwrap().is_none());
    }

    #[test]
    fn after_revert_a_new_active_closure_can_be_created() {
        let conn = test_conn("revert-then-new");
        let episode_id = create_test_episode(&conn, "Paciente Siete", "cerrado");
        insert(&conn, &minimal_closure_row("c1", &episode_id)).unwrap();
        revert(&conn, "c1", "Motivo incorrecto").unwrap();
        // El índice único parcial solo restringe reverted_at IS NULL, así
        // que un segundo cierre vigente para el mismo proceso es válido.
        let c2 = insert(&conn, &NewEpisodeClosureRow { id: "c2", episode_id: &episode_id, closed_at: "2026-02-05", reason: "derivacion", reason_detail: None, outcome: "parcialmente_logrados", summary: None, recommendations: None }).unwrap();
        assert!(c2.reverted_at.is_none());
        assert_eq!(find_active_by_episode(&conn, &episode_id).unwrap().unwrap().id, "c2");
    }

    #[test]
    fn list_history_includes_both_active_and_reverted_ordered_most_recent_first() {
        let conn = test_conn("history-order");
        let episode_id = create_test_episode(&conn, "Paciente Ocho", "cerrado");
        insert(&conn, &minimal_closure_row("c1", &episode_id)).unwrap();
        revert(&conn, "c1", "Motivo incorrecto").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        insert(&conn, &NewEpisodeClosureRow { id: "c2", episode_id: &episode_id, closed_at: "2026-02-05", reason: "derivacion", reason_detail: None, outcome: "parcialmente_logrados", summary: None, recommendations: None }).unwrap();

        let history = list_history_by_episode(&conn, &episode_id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].id, "c2", "el más reciente va primero");
        assert_eq!(history[1].id, "c1");
        assert!(history[1].reverted_at.is_some());
    }

    #[test]
    fn reason_detail_and_summary_and_recommendations_round_trip() {
        let conn = test_conn("full-fields");
        let episode_id = create_test_episode(&conn, "Paciente Nueve", "cerrado");
        let closure = insert(
            &conn,
            &NewEpisodeClosureRow {
                id: "c1", episode_id: &episode_id, closed_at: "2026-02-01", reason: "otro", reason_detail: Some("Detalle específico"),
                outcome: "no_evaluable", summary: Some("Resumen del proceso"), recommendations: Some("Continuar en otro dispositivo terapéutico"),
            },
        )
        .unwrap();
        assert_eq!(closure.reason_detail.as_deref(), Some("Detalle específico"));
        assert_eq!(closure.summary.as_deref(), Some("Resumen del proceso"));
        assert_eq!(closure.recommendations.as_deref(), Some("Continuar en otro dispositivo terapéutico"));
    }
}
