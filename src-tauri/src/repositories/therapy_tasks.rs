//! Acceso a datos de `therapy_tasks` (Fase 8). SQL puro — sin reglas de
//! negocio (eso vive en `services::therapy_tasks`).
//!
//! `goal_id` usa `ON DELETE SET NULL`, pero en la práctica nunca se activa:
//! los objetivos solo se archivan (soft delete vía `deleted_at`), nunca se
//! borran físicamente — una tarea vinculada a un objetivo archivado
//! conserva el vínculo intacto. `TherapyTaskListItem` incluye `goalTitle`
//! (vía `LEFT JOIN`) para mostrarlo sin una segunda consulta desde React,
//! mismo criterio que `repositories::appointments` con el nombre del
//! paciente.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TherapyTask {
    pub id: String,
    pub patient_id: String,
    pub assigned_in_session_id: Option<String>,
    pub goal_id: Option<String>,
    pub description: String,
    pub status: String,
    pub assigned_at: String,
    pub review_due_at: Option<String>,
    pub reviewed_in_session_id: Option<String>,
    pub reviewed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// Fila de listado — incluye el título del objetivo vinculado (si hay uno)
/// para no obligar a una segunda consulta desde React, pero nunca
/// contenido clínico más allá de eso.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TherapyTaskListItem {
    pub id: String,
    pub assigned_in_session_id: Option<String>,
    pub goal_id: Option<String>,
    pub goal_title: Option<String>,
    pub description: String,
    pub status: String,
    pub assigned_at: String,
    pub review_due_at: Option<String>,
    pub reviewed_in_session_id: Option<String>,
    pub reviewed_at: Option<String>,
}

pub struct NewTherapyTaskRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub assigned_in_session_id: Option<&'a str>,
    pub goal_id: Option<&'a str>,
    pub description: &'a str,
    pub review_due_at: Option<&'a str>,
}

/// Campos editables mientras la tarea no está archivada. Deliberadamente
/// sin `status`/`reviewed_*` — esos cambian solo a través de `review`
/// (`set_review`), nunca de una edición genérica de campos.
pub struct TherapyTaskUpdateRow<'a> {
    pub goal_id: Option<&'a str>,
    pub description: &'a str,
    pub review_due_at: Option<&'a str>,
}

const TASK_COLUMNS: &str = "id, patient_id, assigned_in_session_id, goal_id, description, status, assigned_at, \
     review_due_at, reviewed_in_session_id, reviewed_at, created_at, updated_at, deleted_at";

fn map_row(row: &Row) -> rusqlite::Result<TherapyTask> {
    Ok(TherapyTask {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        assigned_in_session_id: row.get(2)?,
        goal_id: row.get(3)?,
        description: row.get(4)?,
        status: row.get(5)?,
        assigned_at: row.get(6)?,
        review_due_at: row.get(7)?,
        reviewed_in_session_id: row.get(8)?,
        reviewed_at: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        deleted_at: row.get(12)?,
    })
}

fn map_list_row(row: &Row) -> rusqlite::Result<TherapyTaskListItem> {
    Ok(TherapyTaskListItem {
        id: row.get(0)?,
        assigned_in_session_id: row.get(1)?,
        goal_id: row.get(2)?,
        goal_title: row.get(3)?,
        description: row.get(4)?,
        status: row.get(5)?,
        assigned_at: row.get(6)?,
        review_due_at: row.get(7)?,
        reviewed_in_session_id: row.get(8)?,
        reviewed_at: row.get(9)?,
    })
}

pub fn insert(conn: &Connection, row: &NewTherapyTaskRow) -> rusqlite::Result<TherapyTask> {
    conn.execute(
        "INSERT INTO therapy_tasks (id, patient_id, assigned_in_session_id, goal_id, description, review_due_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![row.id, row.patient_id, row.assigned_in_session_id, row.goal_id, row.description, row.review_due_at],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

/// Devuelve la tarea exista o no `deleted_at` — igual criterio que
/// `repositories::goals::find_by_id`.
pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<TherapyTask>> {
    conn.query_row(&format!("SELECT {TASK_COLUMNS} FROM therapy_tasks WHERE id = ?1"), params![id], map_row).optional()
}

fn list(conn: &Connection, patient_id: &str, deleted: bool) -> rusqlite::Result<Vec<TherapyTaskListItem>> {
    let deleted_clause = if deleted { "t.deleted_at IS NOT NULL" } else { "t.deleted_at IS NULL" };
    let sql = format!(
        "SELECT t.id, t.assigned_in_session_id, t.goal_id, g.title, t.description, t.status, t.assigned_at, \
         t.review_due_at, t.reviewed_in_session_id, t.reviewed_at \
         FROM therapy_tasks t \
         LEFT JOIN therapeutic_goals g ON g.id = t.goal_id \
         WHERE t.patient_id = ?1 AND {deleted_clause} \
         ORDER BY t.assigned_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], map_list_row)?;
    rows.collect()
}

pub fn list_active_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<TherapyTaskListItem>> {
    list(conn, patient_id, false)
}

pub fn list_archived_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<TherapyTaskListItem>> {
    list(conn, patient_id, true)
}

/// Únicamente las que siguen `pendiente` (y no están archivadas) — lo que
/// se muestra al abrir una sesión nueva y en el panel de continuidad.
pub fn list_pending_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<TherapyTaskListItem>> {
    let sql = "SELECT t.id, t.assigned_in_session_id, t.goal_id, g.title, t.description, t.status, t.assigned_at, \
         t.review_due_at, t.reviewed_in_session_id, t.reviewed_at \
         FROM therapy_tasks t \
         LEFT JOIN therapeutic_goals g ON g.id = t.goal_id \
         WHERE t.patient_id = ?1 AND t.status = 'pendiente' AND t.deleted_at IS NULL \
         ORDER BY t.assigned_at ASC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![patient_id], map_list_row)?;
    rows.collect()
}

/// Conteo global de tareas pendientes, sin importar el paciente ni si está
/// archivado — mismo criterio que `repositories::payments::dashboard_summary`
/// (un pago/tarea cuenta mientras él mismo no esté archivado). Para el
/// bloque "Pendientes" del Dashboard.
pub fn count_pending(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM therapy_tasks WHERE status = 'pendiente' AND deleted_at IS NULL", [], |r| r.get(0))
}

/// `'pendiente'` + `'parcial'` (no archivadas) — usada exclusivamente por
/// la advertencia del flujo de cierre de un proceso (Fase 11), donde
/// también interesan las tareas "a medio hacer", no solo las que ni
/// siquiera se empezaron. Deliberadamente **distinta** de
/// `list_pending_by_patient` (que sigue significando únicamente
/// `'pendiente'` en el resto de la aplicación — panel de continuidad,
/// conteo del Dashboard) para no alterar ningún comportamiento ya
/// aprobado.
pub fn list_pending_or_partial_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<TherapyTaskListItem>> {
    let sql = "SELECT t.id, t.assigned_in_session_id, t.goal_id, g.title, t.description, t.status, t.assigned_at, \
         t.review_due_at, t.reviewed_in_session_id, t.reviewed_at \
         FROM therapy_tasks t \
         LEFT JOIN therapeutic_goals g ON g.id = t.goal_id \
         WHERE t.patient_id = ?1 AND t.status IN ('pendiente', 'parcial') AND t.deleted_at IS NULL \
         ORDER BY t.assigned_at ASC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![patient_id], map_list_row)?;
    rows.collect()
}

/// Edita descripción/objetivo/fecha de revisión prevista. No toca `status`
/// ni los campos de revisión — eso es responsabilidad de `set_review`.
/// Igual que `repositories::goals::update`, no tiene efecto sobre una tarea
/// archivada.
pub fn update_fields(conn: &Connection, id: &str, row: &TherapyTaskUpdateRow) -> rusqlite::Result<Option<TherapyTask>> {
    let affected = conn.execute(
        "UPDATE therapy_tasks SET goal_id = ?1, description = ?2, review_due_at = ?3 WHERE id = ?4 AND deleted_at IS NULL",
        params![row.goal_id, row.description, row.review_due_at, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

/// Cambia el estado. Si `reviewed_in_session_id` viene informado, también
/// fija `reviewed_in_session_id`/`reviewed_at = now()`. Si no viene
/// informado, ninguno de los dos se toca — permite marcar `descartada` (u
/// otro estado) sin necesidad de estar dentro del contexto de una sesión.
/// Sin efecto sobre una tarea archivada.
pub fn set_review(conn: &Connection, id: &str, status: &str, reviewed_in_session_id: Option<&str>) -> rusqlite::Result<Option<TherapyTask>> {
    let affected = if let Some(session_id) = reviewed_in_session_id {
        conn.execute(
            "UPDATE therapy_tasks SET status = ?1, reviewed_in_session_id = ?2, reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ?3 AND deleted_at IS NULL",
            params![status, session_id, id],
        )?
    } else {
        conn.execute("UPDATE therapy_tasks SET status = ?1 WHERE id = ?2 AND deleted_at IS NULL", params![status, id])?
    };
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

/// Soft delete únicamente. No existe, en ningún punto de este módulo, una
/// operación de borrado físico alcanzable desde un comando normal.
pub fn soft_delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE therapy_tasks SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("UPDATE therapy_tasks SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::goals::{self, NewGoalRow};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::sessions::{self, NewSessionRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-therapy-tasks-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x51u8; VAULT_KEY_LEN]);
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
                episode_id: None,
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

    fn create_test_goal(conn: &Connection, patient_id: &str, title: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        goals::insert(conn, &NewGoalRow { id: &id, patient_id, episode_id: None, title, description: None, status: "activo", target_date: None }).unwrap();
        id
    }

    #[test]
    fn inserts_and_finds_a_task_defaulting_to_pendiente() {
        let conn = test_conn("insert-find");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let t = insert(
            &conn,
            &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Registro de pensamientos", review_due_at: None },
        )
        .unwrap();
        assert_eq!(t.status, "pendiente");
        assert_eq!(t.description, "Registro de pensamientos");
        assert!(t.reviewed_at.is_none());
        assert_eq!(find_by_id(&conn, "t1").unwrap().unwrap().id, "t1");
    }

    #[test]
    fn find_by_id_returns_none_for_unknown_id() {
        let conn = test_conn("find-unknown");
        assert!(find_by_id(&conn, "no-existe").unwrap().is_none());
    }

    #[test]
    fn list_item_includes_goal_title_when_linked() {
        let conn = test_conn("goal-title");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let goal_id = create_test_goal(&conn, &patient_id, "Reducir ansiedad");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: Some(&goal_id), description: "Tarea", review_due_at: None }).unwrap();

        let items = list_active_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(items[0].goal_title.as_deref(), Some("Reducir ansiedad"));
    }

    #[test]
    fn list_item_has_no_goal_title_when_unlinked() {
        let conn = test_conn("no-goal");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Tarea", review_due_at: None }).unwrap();

        let items = list_active_by_patient(&conn, &patient_id).unwrap();
        assert!(items[0].goal_title.is_none());
    }

    #[test]
    fn a_task_linked_to_an_archived_goal_still_shows_its_title() {
        let conn = test_conn("archived-goal-title");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let goal_id = create_test_goal(&conn, &patient_id, "Objetivo a archivar");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: Some(&goal_id), description: "Tarea", review_due_at: None }).unwrap();
        goals::soft_delete(&conn, &goal_id).unwrap();

        let items = list_active_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(items[0].goal_title.as_deref(), Some("Objetivo a archivar"), "archivar el objetivo no rompe el vínculo ni oculta su título");
        assert_eq!(items[0].goal_id.as_deref(), Some(goal_id.as_str()));
    }

    #[test]
    fn list_pending_excludes_resolved_and_archived_tasks() {
        let conn = test_conn("list-pending");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Pendiente", review_due_at: None }).unwrap();
        insert(&conn, &NewTherapyTaskRow { id: "t2", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Realizada", review_due_at: None }).unwrap();
        insert(&conn, &NewTherapyTaskRow { id: "t3", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Archivada", review_due_at: None }).unwrap();
        set_review(&conn, "t2", "realizada", None).unwrap();
        soft_delete(&conn, "t3").unwrap();

        let pending = list_pending_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "t1");
    }

    #[test]
    fn list_pending_or_partial_includes_partial_but_excludes_resolved_and_archived() {
        let conn = test_conn("list-pending-or-partial");
        let patient_id = create_test_patient(&conn, "Paciente Cinco Bis");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Pendiente", review_due_at: None }).unwrap();
        insert(&conn, &NewTherapyTaskRow { id: "t2", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Parcial", review_due_at: None }).unwrap();
        insert(&conn, &NewTherapyTaskRow { id: "t3", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Realizada", review_due_at: None }).unwrap();
        insert(&conn, &NewTherapyTaskRow { id: "t4", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Archivada", review_due_at: None }).unwrap();
        set_review(&conn, "t2", "parcial", None).unwrap();
        set_review(&conn, "t3", "realizada", None).unwrap();
        soft_delete(&conn, "t4").unwrap();

        let mut ids: Vec<String> = list_pending_or_partial_by_patient(&conn, &patient_id).unwrap().into_iter().map(|t| t.id).collect();
        ids.sort();
        assert_eq!(ids, vec!["t1".to_string(), "t2".to_string()]);
    }

    #[test]
    fn set_review_with_session_records_reviewed_in_session_and_timestamp() {
        let conn = test_conn("review-with-session");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let session_id = create_test_session(&conn, &patient_id);
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Tarea", review_due_at: None }).unwrap();

        let reviewed = set_review(&conn, "t1", "parcial", Some(&session_id)).unwrap().unwrap();
        assert_eq!(reviewed.status, "parcial");
        assert_eq!(reviewed.reviewed_in_session_id.as_deref(), Some(session_id.as_str()));
        assert!(reviewed.reviewed_at.is_some());
    }

    #[test]
    fn set_review_without_session_leaves_review_fields_untouched() {
        let conn = test_conn("review-without-session");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Tarea", review_due_at: None }).unwrap();

        let discarded = set_review(&conn, "t1", "descartada", None).unwrap().unwrap();
        assert_eq!(discarded.status, "descartada");
        assert!(discarded.reviewed_in_session_id.is_none());
        assert!(discarded.reviewed_at.is_none());
    }

    #[test]
    fn set_review_on_archived_task_does_nothing() {
        let conn = test_conn("review-archived");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Tarea", review_due_at: None }).unwrap();
        soft_delete(&conn, "t1").unwrap();

        let result = set_review(&conn, "t1", "realizada", None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn update_fields_changes_description_goal_and_review_due_but_not_status() {
        let conn = test_conn("update-fields");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let goal_id = create_test_goal(&conn, &patient_id, "Objetivo");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Original", review_due_at: None }).unwrap();
        set_review(&conn, "t1", "parcial", None).unwrap();

        let updated = update_fields(&conn, "t1", &TherapyTaskUpdateRow { goal_id: Some(&goal_id), description: "Editada", review_due_at: Some("2026-09-15") }).unwrap().unwrap();
        assert_eq!(updated.description, "Editada");
        assert_eq!(updated.goal_id.as_deref(), Some(goal_id.as_str()));
        assert_eq!(updated.review_due_at.as_deref(), Some("2026-09-15"));
        assert_eq!(updated.status, "parcial", "editar campos no debe tocar el estado");
    }

    #[test]
    fn update_fields_on_archived_task_does_nothing() {
        let conn = test_conn("update-archived");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Original", review_due_at: None }).unwrap();
        soft_delete(&conn, "t1").unwrap();

        let result = update_fields(&conn, "t1", &TherapyTaskUpdateRow { goal_id: None, description: "Editada", review_due_at: None }).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn list_active_and_archived_are_mutually_exclusive() {
        let conn = test_conn("active-archived");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Tarea", review_due_at: None }).unwrap();

        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 1);
        assert_eq!(list_archived_by_patient(&conn, &patient_id).unwrap().len(), 0);

        assert!(soft_delete(&conn, "t1").unwrap());
        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 0);
        assert_eq!(list_archived_by_patient(&conn, &patient_id).unwrap().len(), 1);

        assert!(restore(&conn, "t1").unwrap());
        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 1);
    }

    #[test]
    fn restoring_a_never_archived_task_reports_nothing_changed() {
        let conn = test_conn("restore-noop");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Tarea", review_due_at: None }).unwrap();
        assert!(!restore(&conn, "t1").unwrap());
    }

    #[test]
    fn count_pending_counts_across_patients_and_excludes_archived() {
        let conn = test_conn("count-pending");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_a, assigned_in_session_id: None, goal_id: None, description: "Uno", review_due_at: None }).unwrap();
        insert(&conn, &NewTherapyTaskRow { id: "t2", patient_id: &patient_b, assigned_in_session_id: None, goal_id: None, description: "Dos", review_due_at: None }).unwrap();
        insert(&conn, &NewTherapyTaskRow { id: "t3", patient_id: &patient_b, assigned_in_session_id: None, goal_id: None, description: "Tres archivada", review_due_at: None }).unwrap();
        soft_delete(&conn, "t3").unwrap();

        assert_eq!(count_pending(&conn).unwrap(), 2);
    }

    #[test]
    fn count_pending_excludes_non_pending_statuses() {
        let conn = test_conn("count-pending-statuses");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        insert(&conn, &NewTherapyTaskRow { id: "t1", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Pendiente", review_due_at: None }).unwrap();
        insert(&conn, &NewTherapyTaskRow { id: "t2", patient_id: &patient_id, assigned_in_session_id: None, goal_id: None, description: "Realizada", review_due_at: None }).unwrap();
        set_review(&conn, "t2", "realizada", None).unwrap();

        assert_eq!(count_pending(&conn).unwrap(), 1);
    }
}
