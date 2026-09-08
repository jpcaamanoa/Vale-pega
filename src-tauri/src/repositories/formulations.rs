//! Acceso a datos de `case_formulations`/`formulation_versions` (Fase 15).
//! SQL puro — sin reglas de negocio (eso vive en `services::formulations`).
//!
//! `formulation_nodes`/`formulation_edges` (reservadas para una futura
//! Formulación Visual, ver `docs/formulation.md`) no se leen ni se escriben
//! desde ningún punto de este módulo — Fase 15 es exclusivamente textual.
//!
//! Una formulación es **inmutable versión a versión**: `formulation_versions`
//! no tiene ninguna función de `update` sobre `summary_text` — solo
//! `insert_version`. "Actualizar formulación" siempre crea una versión
//! nueva; la anterior permanece intacta para siempre.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseFormulation {
    pub id: String,
    pub patient_id: String,
    pub episode_id: Option<String>,
    pub title: String,
    pub model_type: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulationVersion {
    pub id: String,
    pub formulation_id: String,
    pub version_number: i64,
    pub summary_text: Option<String>,
    pub created_at: String,
}

/// Fila de listado — minimización de IPC (mismo criterio que
/// `SafetyPlanSummary`/`AssessmentAdministrationSummary`): nunca lleva
/// `summaryText` completo, solo lo necesario para mostrar una lista. La
/// versión actual es siempre `MAX(version_number)` de la formulación —
/// no existe estado de "borrador" separado.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulationSummary {
    pub id: String,
    pub patient_id: String,
    pub episode_id: Option<String>,
    pub title: String,
    pub model_type: Option<String>,
    pub current_version: i64,
    /// Fecha de creación de la versión actual — "última actualización" en
    /// términos de contenido, distinto de `case_formulations.updated_at`
    /// (que solo cambiaría si se editaran `title`/`model_type`, algo que
    /// esta fase no expone).
    pub updated_at: String,
}

pub struct NewFormulationRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub episode_id: &'a str,
    pub title: &'a str,
    pub model_type: Option<&'a str>,
}

pub struct NewFormulationVersionRow<'a> {
    pub id: &'a str,
    pub formulation_id: &'a str,
    pub version_number: i64,
    pub summary_text: Option<&'a str>,
}

const FORMULATION_COLUMNS: &str = "id, patient_id, episode_id, title, model_type, created_at, updated_at, deleted_at";
const VERSION_COLUMNS: &str = "id, formulation_id, version_number, summary_text, created_at";

fn map_formulation_row(row: &Row) -> rusqlite::Result<CaseFormulation> {
    Ok(CaseFormulation {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        episode_id: row.get(2)?,
        title: row.get(3)?,
        model_type: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        deleted_at: row.get(7)?,
    })
}

fn map_version_row(row: &Row) -> rusqlite::Result<FormulationVersion> {
    Ok(FormulationVersion { id: row.get(0)?, formulation_id: row.get(1)?, version_number: row.get(2)?, summary_text: row.get(3)?, created_at: row.get(4)? })
}

fn map_summary_row(row: &Row) -> rusqlite::Result<FormulationSummary> {
    Ok(FormulationSummary {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        episode_id: row.get(2)?,
        title: row.get(3)?,
        model_type: row.get(4)?,
        current_version: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn insert_formulation(conn: &Connection, row: &NewFormulationRow) -> rusqlite::Result<CaseFormulation> {
    conn.execute(
        "INSERT INTO case_formulations (id, patient_id, episode_id, title, model_type) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![row.id, row.patient_id, row.episode_id, row.title, row.model_type],
    )?;
    find_formulation_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

/// Devuelve la formulación exista o no `deleted_at` — igual criterio que
/// `repositories::treatment_episodes::find_by_id`.
pub fn find_formulation_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<CaseFormulation>> {
    conn.query_row(&format!("SELECT {FORMULATION_COLUMNS} FROM case_formulations WHERE id = ?1"), params![id], map_formulation_row).optional()
}

/// La formulación principal de un proceso, si existe. Nunca hay más de una
/// — garantizado por `idx_case_formulations_one_per_episode`.
pub fn find_formulation_by_episode(conn: &Connection, episode_id: &str) -> rusqlite::Result<Option<CaseFormulation>> {
    conn.query_row(
        &format!("SELECT {FORMULATION_COLUMNS} FROM case_formulations WHERE episode_id = ?1 AND deleted_at IS NULL"),
        params![episode_id],
        map_formulation_row,
    )
    .optional()
}

/// Todas las formulaciones del paciente (de cualquiera de sus procesos,
/// pasados o presente), más reciente primero según la fecha de su versión
/// actual. Nunca lleva `summaryText` — ver `FormulationSummary`.
pub fn list_formulations_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<FormulationSummary>> {
    let sql = "SELECT f.id, f.patient_id, f.episode_id, f.title, f.model_type, v.version_number, v.created_at \
         FROM case_formulations f \
         JOIN formulation_versions v ON v.formulation_id = f.id \
         WHERE f.patient_id = ?1 AND f.deleted_at IS NULL \
           AND v.version_number = (SELECT MAX(version_number) FROM formulation_versions WHERE formulation_id = f.id) \
         ORDER BY v.created_at DESC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![patient_id], map_summary_row)?;
    rows.collect()
}

pub fn insert_version(conn: &Connection, row: &NewFormulationVersionRow) -> rusqlite::Result<FormulationVersion> {
    conn.execute(
        "INSERT INTO formulation_versions (id, formulation_id, version_number, summary_text) VALUES (?1, ?2, ?3, ?4)",
        params![row.id, row.formulation_id, row.version_number, row.summary_text],
    )?;
    find_version_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_version_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<FormulationVersion>> {
    conn.query_row(&format!("SELECT {VERSION_COLUMNS} FROM formulation_versions WHERE id = ?1"), params![id], map_version_row).optional()
}

/// La versión vigente de una formulación — siempre `MAX(version_number)`,
/// nunca un estado separado de "borrador"/"confirmada".
pub fn latest_version(conn: &Connection, formulation_id: &str) -> rusqlite::Result<Option<FormulationVersion>> {
    conn.query_row(
        &format!("SELECT {VERSION_COLUMNS} FROM formulation_versions WHERE formulation_id = ?1 ORDER BY version_number DESC LIMIT 1"),
        params![formulation_id],
        map_version_row,
    )
    .optional()
}

/// Historial completo, más reciente primero — mismo criterio que
/// `repositories::episode_closures::list_history_by_episode`. Nunca se
/// borra ni se filtra nada.
pub fn list_versions(conn: &Connection, formulation_id: &str) -> rusqlite::Result<Vec<FormulationVersion>> {
    let sql = format!("SELECT {VERSION_COLUMNS} FROM formulation_versions WHERE formulation_id = ?1 ORDER BY version_number DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![formulation_id], map_version_row)?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::treatment_episodes::{self, NewTreatmentEpisodeRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-formulations-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x91u8; VAULT_KEY_LEN]);
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

    fn create_test_episode(conn: &Connection, patient_id: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        treatment_episodes::insert(conn, &NewTreatmentEpisodeRow { id: &id, patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        id
    }

    #[test]
    fn inserts_and_finds_a_formulation() {
        let conn = test_conn("insert-find");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let episode_id = create_test_episode(&conn, &patient_id);
        let f = insert_formulation(&conn, &NewFormulationRow { id: "f1", patient_id: &patient_id, episode_id: &episode_id, title: "Formulación inicial", model_type: Some("TCC") }).unwrap();
        assert_eq!(f.title, "Formulación inicial");
        assert_eq!(f.model_type.as_deref(), Some("TCC"));
        assert_eq!(find_formulation_by_id(&conn, "f1").unwrap().unwrap().id, "f1");
    }

    #[test]
    fn find_formulation_by_episode_returns_none_when_no_formulation_exists() {
        let conn = test_conn("by-episode-none");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let episode_id = create_test_episode(&conn, &patient_id);
        assert!(find_formulation_by_episode(&conn, &episode_id).unwrap().is_none());
    }

    #[test]
    fn find_formulation_by_episode_finds_it() {
        let conn = test_conn("by-episode-found");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let episode_id = create_test_episode(&conn, &patient_id);
        insert_formulation(&conn, &NewFormulationRow { id: "f1", patient_id: &patient_id, episode_id: &episode_id, title: "Formulación", model_type: None }).unwrap();
        assert_eq!(find_formulation_by_episode(&conn, &episode_id).unwrap().unwrap().id, "f1");
    }

    #[test]
    fn inserts_v1_and_v2_and_latest_returns_v2() {
        let conn = test_conn("v1-v2-latest");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let episode_id = create_test_episode(&conn, &patient_id);
        insert_formulation(&conn, &NewFormulationRow { id: "f1", patient_id: &patient_id, episode_id: &episode_id, title: "Formulación", model_type: None }).unwrap();
        insert_version(&conn, &NewFormulationVersionRow { id: "v1", formulation_id: "f1", version_number: 1, summary_text: Some("Contenido v1") }).unwrap();
        insert_version(&conn, &NewFormulationVersionRow { id: "v2", formulation_id: "f1", version_number: 2, summary_text: Some("Contenido v2") }).unwrap();

        let latest = latest_version(&conn, "f1").unwrap().unwrap();
        assert_eq!(latest.id, "v2");
        assert_eq!(latest.version_number, 2);
    }

    #[test]
    fn list_versions_orders_most_recent_first_and_never_overwrites() {
        let conn = test_conn("list-versions-order");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let episode_id = create_test_episode(&conn, &patient_id);
        insert_formulation(&conn, &NewFormulationRow { id: "f1", patient_id: &patient_id, episode_id: &episode_id, title: "Formulación", model_type: None }).unwrap();
        insert_version(&conn, &NewFormulationVersionRow { id: "v1", formulation_id: "f1", version_number: 1, summary_text: Some("Contenido v1") }).unwrap();
        insert_version(&conn, &NewFormulationVersionRow { id: "v2", formulation_id: "f1", version_number: 2, summary_text: Some("Contenido v2") }).unwrap();

        let versions = list_versions(&conn, "f1").unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].id, "v2", "la más reciente va primero");
        assert_eq!(versions[1].id, "v1");
        assert_eq!(versions[1].summary_text.as_deref(), Some("Contenido v1"), "v1 permanece intacta");
    }

    #[test]
    fn list_formulations_by_patient_includes_current_version_and_excludes_summary_text() {
        let conn = test_conn("list-summary");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let episode_id = create_test_episode(&conn, &patient_id);
        insert_formulation(&conn, &NewFormulationRow { id: "f1", patient_id: &patient_id, episode_id: &episode_id, title: "Formulación", model_type: Some("5P") }).unwrap();
        insert_version(&conn, &NewFormulationVersionRow { id: "v1", formulation_id: "f1", version_number: 1, summary_text: Some("Contenido clínico extenso") }).unwrap();
        insert_version(&conn, &NewFormulationVersionRow { id: "v2", formulation_id: "f1", version_number: 2, summary_text: Some("Contenido revisado") }).unwrap();

        let summaries = list_formulations_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].current_version, 2);
        assert_eq!(summaries[0].model_type.as_deref(), Some("5P"));
    }

    #[test]
    fn a_second_formulation_for_the_same_episode_is_rejected_at_repository_level() {
        let conn = test_conn("one-per-episode");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let episode_id = create_test_episode(&conn, &patient_id);
        insert_formulation(&conn, &NewFormulationRow { id: "f1", patient_id: &patient_id, episode_id: &episode_id, title: "Formulación", model_type: None }).unwrap();

        let err = insert_formulation(&conn, &NewFormulationRow { id: "f2", patient_id: &patient_id, episode_id: &episode_id, title: "Otra", model_type: None }).unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }
}
