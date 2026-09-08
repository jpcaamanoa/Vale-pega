//! Acceso a datos de `assessment_instruments`/`assessment_administrations`
//! (Fase 13). SQL puro — sin reglas de negocio (eso vive en
//! `services::assessments`). Ver `docs/assessments.md` para el diseño
//! completo, en particular la regla de copyright (esta aplicación nunca
//! almacena el contenido real de un instrumento, solo metadatos y
//! resultados).
//!
//! `raw_responses` (columna de `SCHEMA_V1`) existe en la tabla pero
//! deliberadamente **nunca** aparece en `AssessmentAdministration` ni en
//! ningún `INSERT`/`UPDATE` de este módulo — queda como columna legado sin
//! uso, ver `docs/assessments.md` para la justificación completa.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentInstrument {
    pub id: String,
    pub name: String,
    pub abbreviation: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub is_custom: bool,
}

pub struct NewInstrumentRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub abbreviation: Option<&'a str>,
    pub description: Option<&'a str>,
    pub category: Option<&'a str>,
}

pub struct InstrumentUpdateRow<'a> {
    pub name: &'a str,
    pub abbreviation: Option<&'a str>,
    pub description: Option<&'a str>,
    pub category: Option<&'a str>,
}

const INSTRUMENT_COLUMNS: &str = "id, name, abbreviation, description, category, is_custom";

fn map_instrument_row(row: &Row) -> rusqlite::Result<AssessmentInstrument> {
    Ok(AssessmentInstrument {
        id: row.get(0)?,
        name: row.get(1)?,
        abbreviation: row.get(2)?,
        description: row.get(3)?,
        category: row.get(4)?,
        is_custom: row.get::<_, i64>(5)? != 0,
    })
}

/// Siempre inserta con `is_custom = 1`: en Fase 13 no existe ningún catálogo
/// precargado de instrumentos conocidos — todo instrumento nace del catálogo
/// que la propia usuaria mantiene. La columna se conserva como la definió
/// `SCHEMA_V1` por si una fase futura decide sembrar instrumentos comunes
/// por nombre (nunca su contenido protegido).
pub fn insert_instrument(conn: &Connection, row: &NewInstrumentRow) -> rusqlite::Result<AssessmentInstrument> {
    conn.execute(
        "INSERT INTO assessment_instruments (id, name, abbreviation, description, category, is_custom) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        params![row.id, row.name, row.abbreviation, row.description, row.category],
    )?;
    find_instrument_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_instrument_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<AssessmentInstrument>> {
    conn.query_row(&format!("SELECT {INSTRUMENT_COLUMNS} FROM assessment_instruments WHERE id = ?1"), params![id], map_instrument_row).optional()
}

/// Usado por el servicio para rechazar nombres duplicados con un error de
/// dominio claro, en vez de dejar pasar la violación cruda de `UNIQUE` de
/// SQLite hasta la interfaz.
pub fn find_instrument_by_name(conn: &Connection, name: &str) -> rusqlite::Result<Option<AssessmentInstrument>> {
    conn.query_row(&format!("SELECT {INSTRUMENT_COLUMNS} FROM assessment_instruments WHERE name = ?1"), params![name], map_instrument_row).optional()
}

pub fn list_instruments(conn: &Connection) -> rusqlite::Result<Vec<AssessmentInstrument>> {
    let mut stmt = conn.prepare(&format!("SELECT {INSTRUMENT_COLUMNS} FROM assessment_instruments ORDER BY name COLLATE NOCASE"))?;
    let rows = stmt.query_map([], map_instrument_row)?;
    rows.collect()
}

/// Corrige nombre/abreviatura/descripción/categoría de un instrumento ya
/// existente. Nunca toca `is_custom`. No hay eliminación de instrumentos en
/// este módulo: `assessment_administrations.instrument_id` usa
/// `ON DELETE RESTRICT`, así que un instrumento con administraciones
/// asociadas no podría borrarse de todas formas, y no hay ningún caso de uso
/// pedido para borrar uno que nunca se usó.
pub fn update_instrument(conn: &Connection, id: &str, row: &InstrumentUpdateRow) -> rusqlite::Result<Option<AssessmentInstrument>> {
    let affected = conn.execute(
        "UPDATE assessment_instruments SET name = ?1, abbreviation = ?2, description = ?3, category = ?4 WHERE id = ?5",
        params![row.name, row.abbreviation, row.description, row.category, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_instrument_by_id(conn, id)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentAdministration {
    pub id: String,
    pub patient_id: String,
    pub instrument_id: String,
    pub episode_id: Option<String>,
    pub administered_at: String,
    pub context: Option<String>,
    pub total_score: Option<f64>,
    pub subscale_scores: Option<String>,
    pub interpretation_text: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// Fila de listado — minimización de IPC (mismo criterio que
/// `SafetyPlanSummary` de Fase 12): incluye el nombre/abreviatura del
/// instrumento (vía `JOIN`, para no obligar a una segunda consulta desde
/// React) y el puntaje total (necesario para pintar la evolución
/// longitudinal), pero **no** `subscale_scores` ni `interpretation_text` —
/// eso solo viaja al abrir una administración concreta con `get_by_id`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentAdministrationSummary {
    pub id: String,
    pub instrument_id: String,
    pub instrument_name: String,
    pub instrument_abbreviation: Option<String>,
    pub episode_id: Option<String>,
    pub administered_at: String,
    pub context: Option<String>,
    pub total_score: Option<f64>,
}

pub struct NewAdministrationRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub instrument_id: &'a str,
    pub episode_id: Option<&'a str>,
    pub administered_at: &'a str,
    pub context: Option<&'a str>,
    pub total_score: Option<f64>,
    pub subscale_scores: Option<&'a str>,
    pub interpretation_text: Option<&'a str>,
}

/// Igual que `NewAdministrationRow` salvo `patient_id`/`instrument_id`, que
/// nunca se reasignan una vez creada (mismo criterio que `TherapyTaskUpdateRow`
/// respecto a `patient_id`).
pub struct AdministrationUpdateRow<'a> {
    pub episode_id: Option<&'a str>,
    pub administered_at: &'a str,
    pub context: Option<&'a str>,
    pub total_score: Option<f64>,
    pub subscale_scores: Option<&'a str>,
    pub interpretation_text: Option<&'a str>,
}

const ADMINISTRATION_COLUMNS: &str = "id, patient_id, instrument_id, episode_id, administered_at, context, total_score, \
     subscale_scores, interpretation_text, created_at, updated_at, deleted_at";

fn map_administration_row(row: &Row) -> rusqlite::Result<AssessmentAdministration> {
    Ok(AssessmentAdministration {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        instrument_id: row.get(2)?,
        episode_id: row.get(3)?,
        administered_at: row.get(4)?,
        context: row.get(5)?,
        total_score: row.get(6)?,
        subscale_scores: row.get(7)?,
        interpretation_text: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        deleted_at: row.get(11)?,
    })
}

fn map_administration_summary_row(row: &Row) -> rusqlite::Result<AssessmentAdministrationSummary> {
    Ok(AssessmentAdministrationSummary {
        id: row.get(0)?,
        instrument_id: row.get(1)?,
        instrument_name: row.get(2)?,
        instrument_abbreviation: row.get(3)?,
        episode_id: row.get(4)?,
        administered_at: row.get(5)?,
        context: row.get(6)?,
        total_score: row.get(7)?,
    })
}

pub fn insert_administration(conn: &Connection, row: &NewAdministrationRow) -> rusqlite::Result<AssessmentAdministration> {
    conn.execute(
        "INSERT INTO assessment_administrations \
         (id, patient_id, instrument_id, episode_id, administered_at, context, total_score, subscale_scores, interpretation_text) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.id,
            row.patient_id,
            row.instrument_id,
            row.episode_id,
            row.administered_at,
            row.context,
            row.total_score,
            row.subscale_scores,
            row.interpretation_text,
        ],
    )?;
    find_administration_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

/// Devuelve la administración exista o no `deleted_at` — mismo criterio que
/// `repositories::therapy_tasks::find_by_id`.
pub fn find_administration_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<AssessmentAdministration>> {
    conn.query_row(&format!("SELECT {ADMINISTRATION_COLUMNS} FROM assessment_administrations WHERE id = ?1"), params![id], map_administration_row).optional()
}

fn list_by_patient(conn: &Connection, patient_id: &str, deleted: bool) -> rusqlite::Result<Vec<AssessmentAdministrationSummary>> {
    let deleted_clause = if deleted { "a.deleted_at IS NOT NULL" } else { "a.deleted_at IS NULL" };
    let sql = format!(
        "SELECT a.id, a.instrument_id, i.name, i.abbreviation, a.episode_id, a.administered_at, a.context, a.total_score \
         FROM assessment_administrations a \
         JOIN assessment_instruments i ON i.id = a.instrument_id \
         WHERE a.patient_id = ?1 AND {deleted_clause} \
         ORDER BY a.administered_at DESC, a.created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], map_administration_summary_row)?;
    rows.collect()
}

pub fn list_active_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<AssessmentAdministrationSummary>> {
    list_by_patient(conn, patient_id, false)
}

pub fn list_archived_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<AssessmentAdministrationSummary>> {
    list_by_patient(conn, patient_id, true)
}

/// Evolución longitudinal: todas las administraciones **de un mismo
/// instrumento** para un paciente, más antigua primero (orden natural para
/// graficar/tabular una evolución en el tiempo). Nunca mezcla instrumentos
/// distintos — ver `docs/assessments.md` sobre por qué eso nunca debe
/// ocurrir clínicamente. Reutiliza `idx_assessments_patient_instrument`.
pub fn list_by_patient_and_instrument(conn: &Connection, patient_id: &str, instrument_id: &str) -> rusqlite::Result<Vec<AssessmentAdministrationSummary>> {
    let sql = "SELECT a.id, a.instrument_id, i.name, i.abbreviation, a.episode_id, a.administered_at, a.context, a.total_score \
         FROM assessment_administrations a \
         JOIN assessment_instruments i ON i.id = a.instrument_id \
         WHERE a.patient_id = ?1 AND a.instrument_id = ?2 AND a.deleted_at IS NULL \
         ORDER BY a.administered_at ASC, a.created_at ASC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![patient_id, instrument_id], map_administration_summary_row)?;
    rows.collect()
}

/// Edita los campos de una administración ya registrada. No re-comprueba el
/// paciente archivado (mismo criterio que `repositories::therapy_tasks::update_fields`
/// / `repositories::goals::update`): corregir un registro ya existente de un
/// paciente archivado sigue permitido. Sin efecto sobre una administración
/// archivada.
pub fn update_administration(conn: &Connection, id: &str, row: &AdministrationUpdateRow) -> rusqlite::Result<Option<AssessmentAdministration>> {
    let affected = conn.execute(
        "UPDATE assessment_administrations \
         SET episode_id = ?1, administered_at = ?2, context = ?3, total_score = ?4, subscale_scores = ?5, interpretation_text = ?6 \
         WHERE id = ?7 AND deleted_at IS NULL",
        params![row.episode_id, row.administered_at, row.context, row.total_score, row.subscale_scores, row.interpretation_text, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_administration_by_id(conn, id)
}

/// Soft delete únicamente. No existe, en ningún punto de este módulo, una
/// operación de borrado físico alcanzable desde un comando normal.
pub fn soft_delete_administration(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE assessment_administrations SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore_administration(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("UPDATE assessment_administrations SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::treatment_episodes::{self, NewTreatmentEpisodeRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-assessments-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x71u8; VAULT_KEY_LEN]);
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

    fn create_test_instrument(conn: &Connection, name: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        insert_instrument(conn, &NewInstrumentRow { id: &id, name, abbreviation: None, description: None, category: None }).unwrap();
        id
    }

    #[test]
    fn inserts_and_finds_an_instrument_marked_custom() {
        let conn = test_conn("instrument-insert");
        let i = insert_instrument(&conn, &NewInstrumentRow { id: "i1", name: "BDI-II", abbreviation: Some("BDI-II"), description: Some("Inventario de depresión"), category: Some("Depresión") }).unwrap();
        assert!(i.is_custom);
        assert_eq!(i.abbreviation.as_deref(), Some("BDI-II"));
        assert_eq!(find_instrument_by_id(&conn, "i1").unwrap().unwrap().name, "BDI-II");
    }

    #[test]
    fn find_instrument_by_name_is_case_sensitive_exact_match() {
        let conn = test_conn("instrument-by-name");
        insert_instrument(&conn, &NewInstrumentRow { id: "i1", name: "PHQ-9", abbreviation: None, description: None, category: None }).unwrap();
        assert!(find_instrument_by_name(&conn, "PHQ-9").unwrap().is_some());
        assert!(find_instrument_by_name(&conn, "no-existe").unwrap().is_none());
    }

    #[test]
    fn duplicate_instrument_name_is_rejected_at_database_level() {
        let conn = test_conn("instrument-duplicate");
        insert_instrument(&conn, &NewInstrumentRow { id: "i1", name: "PHQ-9", abbreviation: None, description: None, category: None }).unwrap();
        let err = insert_instrument(&conn, &NewInstrumentRow { id: "i2", name: "PHQ-9", abbreviation: None, description: None, category: None }).unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    #[test]
    fn list_instruments_orders_by_name_case_insensitively() {
        let conn = test_conn("instrument-list");
        insert_instrument(&conn, &NewInstrumentRow { id: "i1", name: "stai", abbreviation: None, description: None, category: None }).unwrap();
        insert_instrument(&conn, &NewInstrumentRow { id: "i2", name: "BDI-II", abbreviation: None, description: None, category: None }).unwrap();
        let names: Vec<String> = list_instruments(&conn).unwrap().into_iter().map(|i| i.name).collect();
        assert_eq!(names, vec!["BDI-II".to_string(), "stai".to_string()]);
    }

    #[test]
    fn update_instrument_edits_metadata_but_not_is_custom() {
        let conn = test_conn("instrument-update");
        insert_instrument(&conn, &NewInstrumentRow { id: "i1", name: "Original", abbreviation: None, description: None, category: None }).unwrap();
        let updated = update_instrument(&conn, "i1", &InstrumentUpdateRow { name: "Corregido", abbreviation: Some("COR"), description: Some("Desc"), category: Some("Cat") }).unwrap().unwrap();
        assert_eq!(updated.name, "Corregido");
        assert_eq!(updated.abbreviation.as_deref(), Some("COR"));
        assert!(updated.is_custom);
    }

    #[test]
    fn inserts_and_finds_an_administration() {
        let conn = test_conn("admin-insert");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let instrument_id = create_test_instrument(&conn, "BDI-II");
        let a = insert_administration(
            &conn,
            &NewAdministrationRow {
                id: "a1",
                patient_id: &patient_id,
                instrument_id: &instrument_id,
                episode_id: None,
                administered_at: "2026-01-10",
                context: Some("ingreso"),
                total_score: Some(18.0),
                subscale_scores: Some(r#"{"cognitivo":10,"somatico":8}"#),
                interpretation_text: Some("Depresión moderada"),
            },
        )
        .unwrap();
        assert_eq!(a.total_score, Some(18.0));
        assert!(a.deleted_at.is_none());
        assert_eq!(find_administration_by_id(&conn, "a1").unwrap().unwrap().id, "a1");
    }

    #[test]
    fn administration_can_link_to_an_episode_of_the_same_patient() {
        let conn = test_conn("admin-episode");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let instrument_id = create_test_instrument(&conn, "BDI-II");
        let episode_id = create_test_episode(&conn, &patient_id);
        let a = insert_administration(
            &conn,
            &NewAdministrationRow { id: "a1", patient_id: &patient_id, instrument_id: &instrument_id, episode_id: Some(&episode_id), administered_at: "2026-01-10", context: None, total_score: None, subscale_scores: None, interpretation_text: None },
        )
        .unwrap();
        assert_eq!(a.episode_id.as_deref(), Some(episode_id.as_str()));
    }

    #[test]
    fn list_item_includes_instrument_name_and_abbreviation_but_not_interpretation() {
        let conn = test_conn("admin-list-summary");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let instrument_id = create_test_instrument(&conn, "STAI");
        insert_administration(&conn, &NewAdministrationRow { id: "a1", patient_id: &patient_id, instrument_id: &instrument_id, episode_id: None, administered_at: "2026-01-10", context: None, total_score: Some(40.0), subscale_scores: None, interpretation_text: Some("contenido clínico") }).unwrap();

        let items = list_active_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].instrument_name, "STAI");
        assert_eq!(items[0].total_score, Some(40.0));
    }

    #[test]
    fn list_by_patient_and_instrument_orders_oldest_first_and_excludes_other_instruments() {
        let conn = test_conn("admin-longitudinal");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let instrument_a = create_test_instrument(&conn, "BDI-II");
        let instrument_b = create_test_instrument(&conn, "STAI");
        insert_administration(&conn, &NewAdministrationRow { id: "a2", patient_id: &patient_id, instrument_id: &instrument_a, episode_id: None, administered_at: "2026-03-01", context: None, total_score: Some(10.0), subscale_scores: None, interpretation_text: None }).unwrap();
        insert_administration(&conn, &NewAdministrationRow { id: "a1", patient_id: &patient_id, instrument_id: &instrument_a, episode_id: None, administered_at: "2026-01-01", context: None, total_score: Some(20.0), subscale_scores: None, interpretation_text: None }).unwrap();
        insert_administration(&conn, &NewAdministrationRow { id: "b1", patient_id: &patient_id, instrument_id: &instrument_b, episode_id: None, administered_at: "2026-02-01", context: None, total_score: Some(30.0), subscale_scores: None, interpretation_text: None }).unwrap();

        let evolution = list_by_patient_and_instrument(&conn, &patient_id, &instrument_a).unwrap();
        let ids: Vec<String> = evolution.iter().map(|a| a.id.clone()).collect();
        assert_eq!(ids, vec!["a1".to_string(), "a2".to_string()], "más antigua primero, sin mezclar el otro instrumento");
    }

    #[test]
    fn update_administration_edits_score_and_interpretation() {
        let conn = test_conn("admin-update");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let instrument_id = create_test_instrument(&conn, "BDI-II");
        insert_administration(&conn, &NewAdministrationRow { id: "a1", patient_id: &patient_id, instrument_id: &instrument_id, episode_id: None, administered_at: "2026-01-10", context: None, total_score: Some(18.0), subscale_scores: None, interpretation_text: None }).unwrap();

        let updated = update_administration(&conn, "a1", &AdministrationUpdateRow { episode_id: None, administered_at: "2026-01-11", context: Some("seguimiento"), total_score: Some(12.0), subscale_scores: None, interpretation_text: Some("Mejoría") }).unwrap().unwrap();
        assert_eq!(updated.administered_at, "2026-01-11");
        assert_eq!(updated.total_score, Some(12.0));
        assert_eq!(updated.interpretation_text.as_deref(), Some("Mejoría"));
    }

    #[test]
    fn update_administration_on_archived_administration_does_nothing() {
        let conn = test_conn("admin-update-archived");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let instrument_id = create_test_instrument(&conn, "BDI-II");
        insert_administration(&conn, &NewAdministrationRow { id: "a1", patient_id: &patient_id, instrument_id: &instrument_id, episode_id: None, administered_at: "2026-01-10", context: None, total_score: None, subscale_scores: None, interpretation_text: None }).unwrap();
        soft_delete_administration(&conn, "a1").unwrap();

        let result = update_administration(&conn, "a1", &AdministrationUpdateRow { episode_id: None, administered_at: "2026-01-11", context: None, total_score: None, subscale_scores: None, interpretation_text: None }).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn archive_and_restore_round_trip() {
        let conn = test_conn("admin-archive-restore");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let instrument_id = create_test_instrument(&conn, "BDI-II");
        insert_administration(&conn, &NewAdministrationRow { id: "a1", patient_id: &patient_id, instrument_id: &instrument_id, episode_id: None, administered_at: "2026-01-10", context: None, total_score: None, subscale_scores: None, interpretation_text: None }).unwrap();

        assert!(soft_delete_administration(&conn, "a1").unwrap());
        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 0);
        assert_eq!(list_archived_by_patient(&conn, &patient_id).unwrap().len(), 1);

        assert!(restore_administration(&conn, "a1").unwrap());
        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 1);
    }

    #[test]
    fn deleting_an_instrument_with_administrations_is_restricted() {
        let conn = test_conn("instrument-restrict-delete");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let instrument_id = create_test_instrument(&conn, "BDI-II");
        insert_administration(&conn, &NewAdministrationRow { id: "a1", patient_id: &patient_id, instrument_id: &instrument_id, episode_id: None, administered_at: "2026-01-10", context: None, total_score: None, subscale_scores: None, interpretation_text: None }).unwrap();

        let err = conn.execute("DELETE FROM assessment_instruments WHERE id = ?1", params![instrument_id]).unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }
}
