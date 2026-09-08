//! Acceso a datos de `safety_plans` y `safety_plan_contacts`. SQL puro —
//! sin reglas de negocio (eso vive en `services::safety_plans`). Ver
//! `docs/safety-plan.md` para el diseño completo.
//!
//! Mismo principio de inmutabilidad ya aplicado a `session_notes` y
//! `episode_closures`: ningún `UPDATE` de este módulo puede tocar el
//! contenido de un plan que no esté en estado `borrador` — cada función de
//! escritura de contenido incluye `WHERE status = 'borrador'` en el propio
//! SQL, no solo como convención de la capa de servicio.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyPlan {
    pub id: String,
    pub patient_id: String,
    pub version: i64,
    pub status: String,
    pub warning_signs: Option<String>,
    pub internal_strategies: Option<String>,
    pub social_support_strategies: Option<String>,
    pub means_safety: Option<String>,
    pub crisis_steps: Option<String>,
    pub notes: Option<String>,
    pub reviewed_at: Option<String>,
    pub confirmed_at: Option<String>,
    pub superseded_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewSafetyPlanRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub version: i64,
    pub warning_signs: Option<&'a str>,
    pub internal_strategies: Option<&'a str>,
    pub social_support_strategies: Option<&'a str>,
    pub means_safety: Option<&'a str>,
    pub crisis_steps: Option<&'a str>,
    pub notes: Option<&'a str>,
    /// Solo se usa al crear un borrador que parte de una copia de otra
    /// versión (`create_draft_from_current`, micro-hardening post-Fase 12);
    /// una creación en blanco normal pasa `None` — el primer guardado real
    /// del borrador (`update_draft`) ya lo persiste de todas formas.
    pub reviewed_at: Option<&'a str>,
}

pub struct SafetyPlanDraftUpdateRow<'a> {
    pub warning_signs: Option<&'a str>,
    pub internal_strategies: Option<&'a str>,
    pub social_support_strategies: Option<&'a str>,
    pub means_safety: Option<&'a str>,
    pub crisis_steps: Option<&'a str>,
    pub notes: Option<&'a str>,
    pub reviewed_at: Option<&'a str>,
}

const PLAN_COLUMNS: &str = "id, patient_id, version, status, warning_signs, internal_strategies, \
     social_support_strategies, means_safety, crisis_steps, notes, reviewed_at, confirmed_at, \
     superseded_at, created_at, updated_at";

fn map_row(row: &Row) -> rusqlite::Result<SafetyPlan> {
    Ok(SafetyPlan {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        version: row.get(2)?,
        status: row.get(3)?,
        warning_signs: row.get(4)?,
        internal_strategies: row.get(5)?,
        social_support_strategies: row.get(6)?,
        means_safety: row.get(7)?,
        crisis_steps: row.get(8)?,
        notes: row.get(9)?,
        reviewed_at: row.get(10)?,
        confirmed_at: row.get(11)?,
        superseded_at: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

/// Proyección minimizada de un plan, para el historial (§33/§49 de la
/// aprobación de Fase 12): nunca lleva el contenido narrativo
/// (`warning_signs`, `internal_strategies`, `means_safety`, `crisis_steps`,
/// `notes`) ni sus contactos — solo lo necesario para listar versiones y
/// elegir cuál abrir. El contenido completo de una versión histórica
/// concreta solo cruza IPC cuando la usuaria la abre explícitamente, vía
/// `find_by_id`. Mismo criterio de minimización que `GoalListItem`/
/// `SessionListItem`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyPlanSummary {
    pub id: String,
    pub patient_id: String,
    pub version: i64,
    pub status: String,
    pub reviewed_at: Option<String>,
    pub confirmed_at: Option<String>,
    pub superseded_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const SUMMARY_COLUMNS: &str = "id, patient_id, version, status, reviewed_at, confirmed_at, superseded_at, created_at, updated_at";

fn map_summary_row(row: &Row) -> rusqlite::Result<SafetyPlanSummary> {
    Ok(SafetyPlanSummary {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        version: row.get(2)?,
        status: row.get(3)?,
        reviewed_at: row.get(4)?,
        confirmed_at: row.get(5)?,
        superseded_at: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

/// Todas las versiones de un paciente, más reciente primero — igual que
/// `list_history_by_patient`, pero sin contenido narrativo.
pub fn list_history_summaries_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<SafetyPlanSummary>> {
    let sql = format!("SELECT {SUMMARY_COLUMNS} FROM safety_plans WHERE patient_id = ?1 ORDER BY version DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], map_summary_row)?;
    rows.collect()
}

/// Inserta siempre como `status = 'borrador'` — el `INSERT` no puede crear
/// un plan ya vigente; esa transición pasa exclusivamente por `confirm`.
pub fn insert(conn: &Connection, row: &NewSafetyPlanRow) -> rusqlite::Result<SafetyPlan> {
    conn.execute(
        "INSERT INTO safety_plans (id, patient_id, version, status, warning_signs, internal_strategies, \
         social_support_strategies, means_safety, crisis_steps, notes, reviewed_at) \
         VALUES (?1, ?2, ?3, 'borrador', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            row.id,
            row.patient_id,
            row.version,
            row.warning_signs,
            row.internal_strategies,
            row.social_support_strategies,
            row.means_safety,
            row.crisis_steps,
            row.notes,
            row.reviewed_at
        ],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<SafetyPlan>> {
    conn.query_row(&format!("SELECT {PLAN_COLUMNS} FROM safety_plans WHERE id = ?1"), params![id], map_row).optional()
}

/// El borrador sin confirmar de un paciente, si existe. A lo sumo uno,
/// garantizado por `idx_safety_plans_one_draft_per_patient`.
pub fn find_draft_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Option<SafetyPlan>> {
    conn.query_row(
        &format!("SELECT {PLAN_COLUMNS} FROM safety_plans WHERE patient_id = ?1 AND status = 'borrador'"),
        params![patient_id],
        map_row,
    )
    .optional()
}

/// El plan vigente de un paciente, si existe. A lo sumo uno, garantizado
/// por `idx_safety_plans_one_current_per_patient`.
pub fn find_current_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Option<SafetyPlan>> {
    conn.query_row(
        &format!("SELECT {PLAN_COLUMNS} FROM safety_plans WHERE patient_id = ?1 AND status = 'vigente'"),
        params![patient_id],
        map_row,
    )
    .optional()
}

/// Todas las versiones de un paciente (vigente, reemplazadas y, si existe,
/// el borrador), de la más reciente a la más antigua. Nunca se filtra ni se
/// borra nada: es el historial auditable completo. Contenido completo — la
/// capa de comandos nunca la expone por IPC, ver `list_history_summaries_by_patient`.
#[allow(dead_code)] // se usa desde los tests de este módulo y de services::safety_plans
pub fn list_history_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<SafetyPlan>> {
    let sql = format!("SELECT {PLAN_COLUMNS} FROM safety_plans WHERE patient_id = ?1 ORDER BY version DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], map_row)?;
    rows.collect()
}

pub fn max_version_for_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COALESCE(MAX(version), 0) FROM safety_plans WHERE patient_id = ?1", params![patient_id], |r| r.get(0))
}

/// Sobrescribe el contenido de un borrador. `WHERE status = 'borrador'` es
/// la barrera estructural — devuelve `false` sin tocar nada si la fila no
/// existe o ya no está en borrador.
pub fn update_draft(conn: &Connection, id: &str, row: &SafetyPlanDraftUpdateRow) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE safety_plans SET warning_signs = ?1, internal_strategies = ?2, social_support_strategies = ?3, \
         means_safety = ?4, crisis_steps = ?5, notes = ?6, reviewed_at = ?7 WHERE id = ?8 AND status = 'borrador'",
        params![row.warning_signs, row.internal_strategies, row.social_support_strategies, row.means_safety, row.crisis_steps, row.notes, row.reviewed_at, id],
    )?;
    Ok(affected > 0)
}

/// Elimina en duro un borrador — la única fila que este módulo permite
/// borrar de verdad (`WHERE status = 'borrador'`); un plan `vigente` o
/// `reemplazado` es estructuralmente inalcanzable para esta función.
pub fn delete_draft(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM safety_plans WHERE id = ?1 AND status = 'borrador'", params![id])?;
    Ok(affected > 0)
}

/// Confirma un borrador: pasa a `vigente` con `confirmed_at` = ahora. Solo
/// afecta filas todavía en borrador.
pub fn confirm(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE safety_plans SET status = 'vigente', confirmed_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
         WHERE id = ?1 AND status = 'borrador'",
        params![id],
    )?;
    Ok(affected > 0)
}

/// Marca el plan vigente como reemplazado (`superseded_at` = ahora). Debe
/// llamarse **antes** de confirmar el borrador siguiente — así nunca hay un
/// instante con dos filas `vigente` para el mismo paciente, que el índice
/// único parcial rechazaría.
pub fn mark_superseded(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE safety_plans SET status = 'reemplazado', superseded_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND status = 'vigente'",
        params![id],
    )?;
    Ok(affected > 0)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyPlanContact {
    pub id: String,
    pub safety_plan_id: String,
    pub contact_type: String,
    pub name: String,
    pub relationship_or_role: Option<String>,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub sort_order: i64,
}

pub struct NewSafetyPlanContactRow<'a> {
    pub id: &'a str,
    pub safety_plan_id: &'a str,
    pub contact_type: &'a str,
    pub name: &'a str,
    pub relationship_or_role: Option<&'a str>,
    pub phone: Option<&'a str>,
    pub notes: Option<&'a str>,
    pub sort_order: i64,
}

pub struct SafetyPlanContactUpdateRow<'a> {
    pub contact_type: &'a str,
    pub name: &'a str,
    pub relationship_or_role: Option<&'a str>,
    pub phone: Option<&'a str>,
    pub notes: Option<&'a str>,
    pub sort_order: i64,
}

const CONTACT_COLUMNS: &str = "id, safety_plan_id, contact_type, name, relationship_or_role, phone, notes, sort_order";

fn map_contact_row(row: &Row) -> rusqlite::Result<SafetyPlanContact> {
    Ok(SafetyPlanContact {
        id: row.get(0)?,
        safety_plan_id: row.get(1)?,
        contact_type: row.get(2)?,
        name: row.get(3)?,
        relationship_or_role: row.get(4)?,
        phone: row.get(5)?,
        notes: row.get(6)?,
        sort_order: row.get(7)?,
    })
}

pub fn insert_contact(conn: &Connection, row: &NewSafetyPlanContactRow) -> rusqlite::Result<SafetyPlanContact> {
    conn.execute(
        "INSERT INTO safety_plan_contacts (id, safety_plan_id, contact_type, name, relationship_or_role, phone, notes, sort_order) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![row.id, row.safety_plan_id, row.contact_type, row.name, row.relationship_or_role, row.phone, row.notes, row.sort_order],
    )?;
    find_contact_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_contact_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<SafetyPlanContact>> {
    conn.query_row(&format!("SELECT {CONTACT_COLUMNS} FROM safety_plan_contacts WHERE id = ?1"), params![id], map_contact_row).optional()
}

pub fn list_contacts_by_plan(conn: &Connection, safety_plan_id: &str) -> rusqlite::Result<Vec<SafetyPlanContact>> {
    let sql = format!("SELECT {CONTACT_COLUMNS} FROM safety_plan_contacts WHERE safety_plan_id = ?1 ORDER BY sort_order, rowid");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![safety_plan_id], map_contact_row)?;
    rows.collect()
}

pub fn update_contact(conn: &Connection, id: &str, row: &SafetyPlanContactUpdateRow) -> rusqlite::Result<Option<SafetyPlanContact>> {
    let affected = conn.execute(
        "UPDATE safety_plan_contacts SET contact_type = ?1, name = ?2, relationship_or_role = ?3, phone = ?4, notes = ?5, sort_order = ?6 WHERE id = ?7",
        params![row.contact_type, row.name, row.relationship_or_role, row.phone, row.notes, row.sort_order, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_contact_by_id(conn, id)
}

/// Borrado real (sin `deleted_at`) — mismo criterio que `goal_indicators`:
/// un contacto es un detalle editable de un borrador, no un registro con
/// valor histórico propio una vez eliminado.
pub fn delete_contact(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM safety_plan_contacts WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-safety-plans-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x53u8; VAULT_KEY_LEN]);
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

    fn minimal_row<'a>(id: &'a str, patient_id: &'a str, version: i64) -> NewSafetyPlanRow<'a> {
        NewSafetyPlanRow {
            id, patient_id, version, warning_signs: None, internal_strategies: None,
            social_support_strategies: None, means_safety: None, crisis_steps: None, notes: None, reviewed_at: None,
        }
    }

    #[test]
    fn inserts_a_plan_as_a_draft() {
        let conn = test_conn("insert-draft");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let plan = insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        assert_eq!(plan.status, "borrador");
        assert!(plan.confirmed_at.is_none());
        assert!(plan.superseded_at.is_none());
    }

    #[test]
    fn a_second_draft_for_the_same_patient_violates_the_unique_index() {
        let conn = test_conn("duplicate-draft");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        let err = insert(&conn, &minimal_row("p2", &patient_id, 2)).unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    #[test]
    fn find_draft_by_patient_returns_none_when_no_draft_exists() {
        let conn = test_conn("find-draft-none");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        assert!(find_draft_by_patient(&conn, &patient_id).unwrap().is_none());
    }

    #[test]
    fn update_draft_writes_to_a_draft_row() {
        let conn = test_conn("update-draft");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();

        let changed = update_draft(
            &conn,
            "p1",
            &SafetyPlanDraftUpdateRow {
                warning_signs: Some("Señales de prueba"), internal_strategies: None, social_support_strategies: None,
                means_safety: None, crisis_steps: None, notes: None, reviewed_at: None,
            },
        )
        .unwrap();
        assert!(changed);
        let plan = find_by_id(&conn, "p1").unwrap().unwrap();
        assert_eq!(plan.warning_signs.as_deref(), Some("Señales de prueba"));
    }

    #[test]
    fn confirming_a_draft_marks_it_current() {
        let conn = test_conn("confirm-draft");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        assert!(confirm(&conn, "p1").unwrap());

        let plan = find_by_id(&conn, "p1").unwrap().unwrap();
        assert_eq!(plan.status, "vigente");
        assert!(plan.confirmed_at.is_some());
        assert_eq!(find_current_by_patient(&conn, &patient_id).unwrap().unwrap().id, "p1");
    }

    #[test]
    fn updating_a_confirmed_plan_directly_changes_nothing() {
        let conn = test_conn("update-confirmed-noop");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        confirm(&conn, "p1").unwrap();

        let changed = update_draft(
            &conn,
            "p1",
            &SafetyPlanDraftUpdateRow {
                warning_signs: Some("intento de sobrescritura"), internal_strategies: None, social_support_strategies: None,
                means_safety: None, crisis_steps: None, notes: None, reviewed_at: None,
            },
        )
        .unwrap();
        assert!(!changed, "update_draft nunca debe reportar éxito sobre un plan ya confirmado");
        let plan = find_by_id(&conn, "p1").unwrap().unwrap();
        assert!(plan.warning_signs.is_none());
    }

    #[test]
    fn mark_superseded_sets_flags_and_a_new_draft_can_then_be_confirmed() {
        let conn = test_conn("mark-superseded");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        confirm(&conn, "p1").unwrap();

        assert!(mark_superseded(&conn, "p1").unwrap());
        let p1 = find_by_id(&conn, "p1").unwrap().unwrap();
        assert_eq!(p1.status, "reemplazado");
        assert!(p1.superseded_at.is_some());
        assert!(find_current_by_patient(&conn, &patient_id).unwrap().is_none());

        insert(&conn, &minimal_row("p2", &patient_id, 2)).unwrap();
        assert!(confirm(&conn, "p2").unwrap());
        assert_eq!(find_current_by_patient(&conn, &patient_id).unwrap().unwrap().id, "p2");
    }

    #[test]
    fn deletes_a_draft_but_never_a_confirmed_plan() {
        let conn = test_conn("delete-draft-only");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        assert!(delete_draft(&conn, "p1").unwrap());
        assert!(find_by_id(&conn, "p1").unwrap().is_none());

        insert(&conn, &minimal_row("p2", &patient_id, 1)).unwrap();
        confirm(&conn, "p2").unwrap();
        assert!(!delete_draft(&conn, "p2").unwrap(), "un plan vigente nunca debe poder eliminarse por esta vía");
        assert!(find_by_id(&conn, "p2").unwrap().is_some());
    }

    #[test]
    fn list_history_includes_all_versions_most_recent_first() {
        let conn = test_conn("history-order");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        confirm(&conn, "p1").unwrap();
        mark_superseded(&conn, "p1").unwrap();
        insert(&conn, &minimal_row("p2", &patient_id, 2)).unwrap();

        let history = list_history_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].id, "p2");
        assert_eq!(history[1].id, "p1");
    }

    #[test]
    fn list_history_summaries_matches_full_history_but_without_narrative_content() {
        let conn = test_conn("history-summaries");
        let patient_id = create_test_patient(&conn, "Paciente Diecinueve");
        insert(
            &conn,
            &NewSafetyPlanRow {
                id: "p1", patient_id: &patient_id, version: 1, warning_signs: Some("Señales sensibles"),
                internal_strategies: None, social_support_strategies: None, means_safety: Some("Detalle sensible"), crisis_steps: None, notes: None, reviewed_at: None,
            },
        )
        .unwrap();
        confirm(&conn, "p1").unwrap();
        mark_superseded(&conn, "p1").unwrap();
        insert(&conn, &minimal_row("p2", &patient_id, 2)).unwrap();

        let summaries = list_history_summaries_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, "p2");
        assert_eq!(summaries[1].id, "p1");
        assert_eq!(summaries[1].status, "reemplazado");
        assert!(summaries[1].confirmed_at.is_some());
        assert!(summaries[1].superseded_at.is_some());
    }

    #[test]
    fn max_version_for_patient_defaults_to_zero() {
        let conn = test_conn("max-version-zero");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        assert_eq!(max_version_for_patient(&conn, &patient_id).unwrap(), 0);
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        assert_eq!(max_version_for_patient(&conn, &patient_id).unwrap(), 1);
    }

    // ---- contactos ----

    #[test]
    fn inserts_lists_and_orders_contacts() {
        let conn = test_conn("contacts-insert-list");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();

        insert_contact(&conn, &NewSafetyPlanContactRow { id: "c2", safety_plan_id: "p1", contact_type: "support_person", name: "Segundo", relationship_or_role: None, phone: None, notes: None, sort_order: 1 }).unwrap();
        insert_contact(&conn, &NewSafetyPlanContactRow { id: "c1", safety_plan_id: "p1", contact_type: "professional", name: "Primero", relationship_or_role: Some("Psiquiatra"), phone: Some("+56900000001"), notes: None, sort_order: 0 }).unwrap();

        let contacts = list_contacts_by_plan(&conn, "p1").unwrap();
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].name, "Primero", "ordenado por sort_order");
        assert_eq!(contacts[1].name, "Segundo");
    }

    #[test]
    fn updates_and_deletes_a_contact() {
        let conn = test_conn("contacts-update-delete");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        insert_contact(&conn, &NewSafetyPlanContactRow { id: "c1", safety_plan_id: "p1", contact_type: "service", name: "Servicio Original", relationship_or_role: None, phone: None, notes: None, sort_order: 0 }).unwrap();

        let updated = update_contact(&conn, "c1", &SafetyPlanContactUpdateRow { contact_type: "service", name: "Servicio Editado", relationship_or_role: None, phone: Some("+56900000002"), notes: None, sort_order: 0 }).unwrap().unwrap();
        assert_eq!(updated.name, "Servicio Editado");

        assert!(delete_contact(&conn, "c1").unwrap());
        assert!(list_contacts_by_plan(&conn, "p1").unwrap().is_empty());
    }

    #[test]
    fn deleting_the_parent_plan_cascades_to_its_contacts() {
        let conn = test_conn("cascade-delete");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        insert(&conn, &minimal_row("p1", &patient_id, 1)).unwrap();
        insert_contact(&conn, &NewSafetyPlanContactRow { id: "c1", safety_plan_id: "p1", contact_type: "support_person", name: "Contacto", relationship_or_role: None, phone: None, notes: None, sort_order: 0 }).unwrap();

        assert!(delete_draft(&conn, "p1").unwrap());
        assert!(list_contacts_by_plan(&conn, "p1").unwrap().is_empty(), "ON DELETE CASCADE debe eliminar los contactos huérfanos");
    }
}
