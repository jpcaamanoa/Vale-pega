//! Reglas de negocio de tareas terapéuticas entre sesiones (Fase 8). Ver
//! `docs/session-continuity.md` para el diseño completo.
//!
//! Distinto de `session_notes.homework_tasks`: una tarea es un registro
//! **operativo** con ciclo de vida propio (`pendiente` → `parcial` /
//! `realizada` / `no_realizada` / `descartada`), independiente de cualquier
//! nota clínica concreta. También distinto de `reminders` (no implementado
//! todavía): una tarea pertenece al proceso clínico, no es una alerta
//! temporal genérica.
//!
//! Reglas de integridad no negociables:
//! - Si se informa `assigned_in_session_id`/`reviewed_in_session_id`, esa
//!   sesión debe pertenecer al mismo paciente de la tarea.
//! - Si se informa `goal_id`, ese objetivo debe pertenecer al mismo
//!   paciente — nunca se permite vincular la tarea de un paciente con el
//!   objetivo de otro, aunque las FK individuales por separado sean
//!   válidas.
//!
//! Ninguna de estas comprobaciones se confía al frontend.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::goals;
use crate::repositories::patients;
use crate::repositories::sessions;
use crate::repositories::therapy_tasks::{self, NewTherapyTaskRow, TherapyTask, TherapyTaskListItem, TherapyTaskUpdateRow};

/// Los cuatro estados pedidos explícitamente más `descartada` — agregado
/// porque cubre un caso real distinto de `no_realizada`: una tarea que deja
/// de tener sentido *antes* de llegar a revisarse en ninguna sesión (el
/// paciente cambió de foco, la tarea era un duplicado, etc.), sin que eso
/// implique que hubo una revisión con resultado negativo. `no_realizada`
/// siempre se asocia, en el flujo normal, a una revisión real dentro de una
/// sesión; `descartada` explícitamente no la requiere.
pub const VALID_STATUSES: &[&str] = &["pendiente", "parcial", "realizada", "no_realizada", "descartada"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TherapyTaskInput {
    pub patient_id: String,
    pub description: String,
    pub assigned_in_session_id: Option<String>,
    pub goal_id: Option<String>,
    pub review_due_at: Option<String>,
}

/// Deliberadamente sin `patientId` ni `status`/campos de revisión —
/// reasignar una tarea a otro paciente no es una operación de este MVP
/// (mismo criterio que `GoalUpdateInput`), y el estado cambia únicamente a
/// través de `review_task`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TherapyTaskUpdateInput {
    pub description: String,
    pub goal_id: Option<String>,
    pub review_due_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TherapyTaskReviewInput {
    pub status: String,
    pub reviewed_in_session_id: Option<String>,
}

#[derive(Debug)]
pub enum TherapyTaskValidationError {
    DescriptionRequired,
    InvalidStatus(String),
    InvalidDate,
}

impl fmt::Display for TherapyTaskValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TherapyTaskValidationError::DescriptionRequired => write!(f, "la descripción de la tarea es obligatoria"),
            TherapyTaskValidationError::InvalidStatus(s) => {
                write!(f, "estado inválido: '{s}' (debe ser uno de: {})", VALID_STATUSES.join(", "))
            }
            TherapyTaskValidationError::InvalidDate => write!(f, "fecha inválida (formato esperado: AAAA-MM-DD)"),
        }
    }
}
impl std::error::Error for TherapyTaskValidationError {}

#[derive(Debug)]
pub enum TherapyTaskError {
    Validation(TherapyTaskValidationError),
    NotFound,
    PatientNotFound,
    PatientArchived,
    SessionNotFound,
    SessionPatientMismatch,
    GoalNotFound,
    GoalPatientMismatch,
    Database(rusqlite::Error),
}

impl fmt::Display for TherapyTaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TherapyTaskError::Validation(e) => write!(f, "{e}"),
            TherapyTaskError::NotFound => write!(f, "tarea no encontrada"),
            TherapyTaskError::PatientNotFound => write!(f, "paciente no encontrado"),
            TherapyTaskError::PatientArchived => write!(f, "no se pueden crear tareas nuevas para un paciente archivado"),
            TherapyTaskError::SessionNotFound => write!(f, "sesión no encontrada"),
            TherapyTaskError::SessionPatientMismatch => write!(f, "la sesión indicada pertenece a otro paciente"),
            TherapyTaskError::GoalNotFound => write!(f, "objetivo no encontrado"),
            TherapyTaskError::GoalPatientMismatch => write!(f, "el objetivo indicado pertenece a otro paciente"),
            TherapyTaskError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for TherapyTaskError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TherapyTaskError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for TherapyTaskError {
    fn from(e: rusqlite::Error) -> Self {
        TherapyTaskError::Database(e)
    }
}
impl From<TherapyTaskValidationError> for TherapyTaskError {
    fn from(e: TherapyTaskValidationError) -> Self {
        TherapyTaskError::Validation(e)
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// Mismo formato y misma forma de validación (estructural, no calendárica)
/// que `services::goals::validate_date_format`.
fn validate_date_format(value: &str) -> Result<(), TherapyTaskValidationError> {
    let bytes = value.as_bytes();
    let shape_ok = bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-';
    let parse = |s: &str| s.parse::<u32>().ok();
    let ok = shape_ok
        && match (parse(&value[0..4]), parse(&value[5..7]), parse(&value[8..10])) {
            (Some(_year), Some(month), Some(day)) => (1..=12).contains(&month) && (1..=31).contains(&day),
            _ => false,
        };
    if ok {
        Ok(())
    } else {
        Err(TherapyTaskValidationError::InvalidDate)
    }
}

fn validate_status(status: &str) -> Result<(), TherapyTaskValidationError> {
    if VALID_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(TherapyTaskValidationError::InvalidStatus(status.to_string()))
    }
}

fn validate_description(description: String) -> Result<String, TherapyTaskValidationError> {
    let description = description.trim().to_string();
    if description.is_empty() {
        return Err(TherapyTaskValidationError::DescriptionRequired);
    }
    Ok(description)
}

fn validate_review_due_at(value: Option<String>) -> Result<Option<String>, TherapyTaskValidationError> {
    let value = none_if_blank(value);
    if let Some(ref d) = value {
        validate_date_format(d)?;
    }
    Ok(value)
}

/// Si `session_id` viene informado, comprueba que la sesión exista y
/// pertenezca al mismo paciente — nunca se confía solo en el `patientId`
/// enviado por React. Mismo patrón que
/// `services::payments::check_session_belongs_to_patient`.
fn check_session_belongs_to_patient(conn: &Connection, session_id: &Option<String>, patient_id: &str) -> Result<(), TherapyTaskError> {
    if let Some(session_id) = session_id {
        let session = sessions::find_by_id(conn, session_id)?.ok_or(TherapyTaskError::SessionNotFound)?;
        if session.patient_id != patient_id {
            return Err(TherapyTaskError::SessionPatientMismatch);
        }
    }
    Ok(())
}

/// Si `goal_id` viene informado, comprueba que el objetivo exista y
/// pertenezca al mismo paciente. `goals::find_by_id` devuelve el objetivo
/// exista o no `deleted_at` — un objetivo archivado sigue siendo un vínculo
/// válido (regla 8 de la aprobación).
fn check_goal_belongs_to_patient(conn: &Connection, goal_id: &Option<String>, patient_id: &str) -> Result<(), TherapyTaskError> {
    if let Some(goal_id) = goal_id {
        let goal = goals::find_by_id(conn, goal_id)?.ok_or(TherapyTaskError::GoalNotFound)?;
        if goal.patient_id != patient_id {
            return Err(TherapyTaskError::GoalPatientMismatch);
        }
    }
    Ok(())
}

/// Rechaza la creación para un paciente inexistente o archivado — mismo
/// criterio que `services::goals::create_goal` / `services::payments::create_payment`.
/// Siempre parte en `pendiente`; `assigned_at` se fija automáticamente al
/// momento de la creación (no es un campo editable por la usuaria).
pub fn create_task(conn: &Connection, input: TherapyTaskInput) -> Result<TherapyTask, TherapyTaskError> {
    let patient = patients::find_by_id(conn, &input.patient_id)?.ok_or(TherapyTaskError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(TherapyTaskError::PatientArchived);
    }

    let description = validate_description(input.description)?;
    let review_due_at = validate_review_due_at(input.review_due_at)?;
    check_session_belongs_to_patient(conn, &input.assigned_in_session_id, &input.patient_id)?;
    check_goal_belongs_to_patient(conn, &input.goal_id, &input.patient_id)?;

    let id = uuid::Uuid::new_v4().to_string();
    Ok(therapy_tasks::insert(
        conn,
        &NewTherapyTaskRow {
            id: &id,
            patient_id: &input.patient_id,
            assigned_in_session_id: input.assigned_in_session_id.as_deref(),
            goal_id: input.goal_id.as_deref(),
            description: &description,
            review_due_at: review_due_at.as_deref(),
        },
    )?)
}

pub fn get_task(conn: &Connection, id: &str) -> Result<TherapyTask, TherapyTaskError> {
    therapy_tasks::find_by_id(conn, id)?.ok_or(TherapyTaskError::NotFound)
}

pub fn list_tasks(conn: &Connection, patient_id: &str) -> Result<Vec<TherapyTaskListItem>, TherapyTaskError> {
    Ok(therapy_tasks::list_active_by_patient(conn, patient_id)?)
}

pub fn list_archived_tasks(conn: &Connection, patient_id: &str) -> Result<Vec<TherapyTaskListItem>, TherapyTaskError> {
    Ok(therapy_tasks::list_archived_by_patient(conn, patient_id)?)
}

pub fn list_pending_tasks(conn: &Connection, patient_id: &str) -> Result<Vec<TherapyTaskListItem>, TherapyTaskError> {
    Ok(therapy_tasks::list_pending_by_patient(conn, patient_id)?)
}

/// `'pendiente'` + `'parcial'` del paciente — usada exclusivamente por la
/// advertencia del flujo de cierre de un proceso (Fase 11). Ver la nota de
/// `repositories::therapy_tasks::list_pending_or_partial_by_patient` sobre
/// por qué es una función separada de `list_pending_tasks`.
pub fn list_pending_or_partial_tasks(conn: &Connection, patient_id: &str) -> Result<Vec<TherapyTaskListItem>, TherapyTaskError> {
    Ok(therapy_tasks::list_pending_or_partial_by_patient(conn, patient_id)?)
}

/// Conteo global de tareas pendientes, para el bloque "Pendientes" del
/// Dashboard.
pub fn pending_task_count(conn: &Connection) -> Result<i64, TherapyTaskError> {
    Ok(therapy_tasks::count_pending(conn)?)
}

/// Edita descripción/objetivo vinculado/fecha de revisión prevista. No
/// vuelve a comprobar si el paciente está archivado (corregir datos de una
/// tarea histórica de un paciente archivado sigue permitido, mismo
/// criterio que `services::payments::update_payment`), pero si `goal_id`
/// cambia, sí se revalida contra el paciente de la tarea.
pub fn update_task(conn: &Connection, id: &str, input: TherapyTaskUpdateInput) -> Result<TherapyTask, TherapyTaskError> {
    let existing = therapy_tasks::find_by_id(conn, id)?.ok_or(TherapyTaskError::NotFound)?;
    let description = validate_description(input.description)?;
    let review_due_at = validate_review_due_at(input.review_due_at)?;
    check_goal_belongs_to_patient(conn, &input.goal_id, &existing.patient_id)?;

    let row = TherapyTaskUpdateRow { goal_id: input.goal_id.as_deref(), description: &description, review_due_at: review_due_at.as_deref() };
    therapy_tasks::update_fields(conn, id, &row)?.ok_or(TherapyTaskError::NotFound)
}

/// La acción de resolución: cambia el estado y, si se revisó dentro de una
/// sesión concreta, registra cuál y cuándo. Si `reviewed_in_session_id` no
/// viene informado, el estado cambia igual pero no se toca ningún campo de
/// revisión — cubre, por ejemplo, descartar una tarea desde la pestaña de
/// Tareas del paciente sin estar dentro de ninguna sesión.
pub fn review_task(conn: &Connection, id: &str, input: TherapyTaskReviewInput) -> Result<TherapyTask, TherapyTaskError> {
    validate_status(&input.status)?;
    let existing = therapy_tasks::find_by_id(conn, id)?.ok_or(TherapyTaskError::NotFound)?;
    check_session_belongs_to_patient(conn, &input.reviewed_in_session_id, &existing.patient_id)?;

    therapy_tasks::set_review(conn, id, &input.status, input.reviewed_in_session_id.as_deref())?.ok_or(TherapyTaskError::NotFound)
}

/// Soft delete únicamente. No existe, en ningún punto de este servicio ni
/// del repositorio, una operación de borrado físico alcanzable desde un
/// comando normal.
pub fn archive_task(conn: &Connection, id: &str) -> Result<(), TherapyTaskError> {
    if therapy_tasks::soft_delete(conn, id)? {
        Ok(())
    } else {
        Err(TherapyTaskError::NotFound)
    }
}

pub fn restore_task(conn: &Connection, id: &str) -> Result<TherapyTask, TherapyTaskError> {
    if therapy_tasks::restore(conn, id)? {
        get_task(conn, id)
    } else {
        Err(TherapyTaskError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::goals::{self as goals_repo, NewGoalRow};
    use crate::repositories::patients::{self as patients_repo, NewPatientRow};
    use crate::repositories::sessions::{self as sessions_repo, NewSessionRow};
    use crate::services::patients as patients_service;

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-therapy-tasks-svc-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x61u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_patient(conn: &Connection, name: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        patients_repo::insert(
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
        sessions_repo::insert(
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
        goals_repo::insert(conn, &NewGoalRow { id: &id, patient_id, episode_id: None, title, description: None, status: "activo", target_date: None }).unwrap();
        id
    }

    fn minimal_input(patient_id: &str) -> TherapyTaskInput {
        TherapyTaskInput { patient_id: patient_id.to_string(), description: "Registro de pensamientos".into(), assigned_in_session_id: None, goal_id: None, review_due_at: None }
    }

    #[test]
    fn creates_a_task_without_a_goal() {
        let conn = test_conn("create-no-goal");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let t = create_task(&conn, minimal_input(&patient_id)).unwrap();
        assert_eq!(t.status, "pendiente");
        assert!(t.goal_id.is_none());
    }

    #[test]
    fn creates_a_task_with_a_valid_goal() {
        let conn = test_conn("create-with-goal");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let goal_id = create_test_goal(&conn, &patient_id, "Reducir ansiedad");
        let input = TherapyTaskInput { goal_id: Some(goal_id.clone()), ..minimal_input(&patient_id) };
        let t = create_task(&conn, input).unwrap();
        assert_eq!(t.goal_id.as_deref(), Some(goal_id.as_str()));
    }

    #[test]
    fn rejects_a_goal_belonging_to_a_different_patient() {
        let conn = test_conn("goal-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let goal_of_b = create_test_goal(&conn, &patient_b, "Objetivo de B");

        let input = TherapyTaskInput { goal_id: Some(goal_of_b), ..minimal_input(&patient_a) };
        let err = create_task(&conn, input).unwrap_err();
        assert!(matches!(err, TherapyTaskError::GoalPatientMismatch));
    }

    #[test]
    fn rejects_a_nonexistent_goal() {
        let conn = test_conn("goal-not-found");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let input = TherapyTaskInput { goal_id: Some("no-existe".into()), ..minimal_input(&patient_id) };
        let err = create_task(&conn, input).unwrap_err();
        assert!(matches!(err, TherapyTaskError::GoalNotFound));
    }

    #[test]
    fn assigns_from_a_valid_session() {
        let conn = test_conn("assign-from-session");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let session_id = create_test_session(&conn, &patient_id);
        let input = TherapyTaskInput { assigned_in_session_id: Some(session_id.clone()), ..minimal_input(&patient_id) };
        let t = create_task(&conn, input).unwrap();
        assert_eq!(t.assigned_in_session_id.as_deref(), Some(session_id.as_str()));
    }

    #[test]
    fn rejects_a_session_belonging_to_a_different_patient() {
        let conn = test_conn("session-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let session_of_b = create_test_session(&conn, &patient_b);

        let input = TherapyTaskInput { assigned_in_session_id: Some(session_of_b), ..minimal_input(&patient_a) };
        let err = create_task(&conn, input).unwrap_err();
        assert!(matches!(err, TherapyTaskError::SessionPatientMismatch));
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let conn = test_conn("patient-not-found");
        let err = create_task(&conn, minimal_input("no-existe")).unwrap_err();
        assert!(matches!(err, TherapyTaskError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let conn = test_conn("patient-archived");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = create_task(&conn, minimal_input(&patient_id)).unwrap_err();
        assert!(matches!(err, TherapyTaskError::PatientArchived));
    }

    #[test]
    fn rejects_empty_description() {
        let conn = test_conn("empty-description");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let input = TherapyTaskInput { description: "   ".into(), ..minimal_input(&patient_id) };
        let err = create_task(&conn, input).unwrap_err();
        assert!(matches!(err, TherapyTaskError::Validation(TherapyTaskValidationError::DescriptionRequired)));
    }

    #[test]
    fn rejects_invalid_review_due_at_format() {
        let conn = test_conn("invalid-date");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let input = TherapyTaskInput { review_due_at: Some("no-es-fecha".into()), ..minimal_input(&patient_id) };
        let err = create_task(&conn, input).unwrap_err();
        assert!(matches!(err, TherapyTaskError::Validation(TherapyTaskValidationError::InvalidDate)));
    }

    #[test]
    fn full_lifecycle_pendiente_to_parcial_reviewed_in_a_session() {
        let conn = test_conn("lifecycle-parcial");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let session4 = create_test_session(&conn, &patient_id);
        let session5 = create_test_session(&conn, &patient_id);
        let input = TherapyTaskInput { assigned_in_session_id: Some(session4), ..minimal_input(&patient_id) };
        let t = create_task(&conn, input).unwrap();
        assert_eq!(t.status, "pendiente");

        let reviewed = review_task(&conn, &t.id, TherapyTaskReviewInput { status: "parcial".into(), reviewed_in_session_id: Some(session5.clone()) }).unwrap();
        assert_eq!(reviewed.status, "parcial");
        assert_eq!(reviewed.reviewed_in_session_id.as_deref(), Some(session5.as_str()));
        assert!(reviewed.reviewed_at.is_some());
    }

    #[test]
    fn pendiente_to_realizada() {
        let conn = test_conn("lifecycle-realizada");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let t = create_task(&conn, minimal_input(&patient_id)).unwrap();
        let reviewed = review_task(&conn, &t.id, TherapyTaskReviewInput { status: "realizada".into(), reviewed_in_session_id: None }).unwrap();
        assert_eq!(reviewed.status, "realizada");
    }

    #[test]
    fn pendiente_to_no_realizada() {
        let conn = test_conn("lifecycle-no-realizada");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let t = create_task(&conn, minimal_input(&patient_id)).unwrap();
        let reviewed = review_task(&conn, &t.id, TherapyTaskReviewInput { status: "no_realizada".into(), reviewed_in_session_id: None }).unwrap();
        assert_eq!(reviewed.status, "no_realizada");
    }

    #[test]
    fn pendiente_to_descartada_without_ever_being_reviewed_in_a_session() {
        let conn = test_conn("lifecycle-descartada");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let t = create_task(&conn, minimal_input(&patient_id)).unwrap();
        let discarded = review_task(&conn, &t.id, TherapyTaskReviewInput { status: "descartada".into(), reviewed_in_session_id: None }).unwrap();
        assert_eq!(discarded.status, "descartada");
        assert!(discarded.reviewed_in_session_id.is_none(), "descartar no exige haber pasado por una sesión de revisión");
    }

    #[test]
    fn review_rejects_a_reviewing_session_of_a_different_patient() {
        let conn = test_conn("review-session-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let t = create_task(&conn, minimal_input(&patient_a)).unwrap();
        let session_of_b = create_test_session(&conn, &patient_b);

        let err = review_task(&conn, &t.id, TherapyTaskReviewInput { status: "realizada".into(), reviewed_in_session_id: Some(session_of_b) }).unwrap_err();
        assert!(matches!(err, TherapyTaskError::SessionPatientMismatch));
    }

    #[test]
    fn review_rejects_invalid_status() {
        let conn = test_conn("review-invalid-status");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let t = create_task(&conn, minimal_input(&patient_id)).unwrap();
        let err = review_task(&conn, &t.id, TherapyTaskReviewInput { status: "inventado".into(), reviewed_in_session_id: None }).unwrap_err();
        assert!(matches!(err, TherapyTaskError::Validation(TherapyTaskValidationError::InvalidStatus(_))));
    }

    #[test]
    fn review_rejects_a_nonexistent_reviewing_session() {
        let conn = test_conn("review-session-not-found");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        let t = create_task(&conn, minimal_input(&patient_id)).unwrap();
        let err = review_task(&conn, &t.id, TherapyTaskReviewInput { status: "realizada".into(), reviewed_in_session_id: Some("no-existe".into()) }).unwrap_err();
        assert!(matches!(err, TherapyTaskError::SessionNotFound));
    }

    #[test]
    fn update_task_edits_description_and_review_due_at() {
        let conn = test_conn("update-task");
        let patient_id = create_test_patient(&conn, "Paciente Catorce");
        let t = create_task(&conn, minimal_input(&patient_id)).unwrap();
        let updated = update_task(&conn, &t.id, TherapyTaskUpdateInput { description: "Editada".into(), goal_id: None, review_due_at: Some("2026-10-01".into()) }).unwrap();
        assert_eq!(updated.description, "Editada");
        assert_eq!(updated.review_due_at.as_deref(), Some("2026-10-01"));
    }

    #[test]
    fn update_task_rejects_a_goal_belonging_to_a_different_patient() {
        let conn = test_conn("update-goal-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let t = create_task(&conn, minimal_input(&patient_a)).unwrap();
        let goal_of_b = create_test_goal(&conn, &patient_b, "Objetivo de B");

        let err = update_task(&conn, &t.id, TherapyTaskUpdateInput { description: "Editada".into(), goal_id: Some(goal_of_b), review_due_at: None }).unwrap_err();
        assert!(matches!(err, TherapyTaskError::GoalPatientMismatch));
    }

    #[test]
    fn editing_a_historical_task_of_an_archived_patient_is_allowed() {
        let conn = test_conn("edit-archived-patient-task");
        let patient_id = create_test_patient(&conn, "Paciente Quince");
        let t = create_task(&conn, minimal_input(&patient_id)).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();

        let updated = update_task(&conn, &t.id, TherapyTaskUpdateInput { description: "Corregida tras archivar".into(), goal_id: None, review_due_at: None }).unwrap();
        assert_eq!(updated.description, "Corregida tras archivar");
    }

    #[test]
    fn archive_and_restore_round_trip() {
        let conn = test_conn("archive-restore");
        let patient_id = create_test_patient(&conn, "Paciente Dieciséis");
        let t = create_task(&conn, minimal_input(&patient_id)).unwrap();

        archive_task(&conn, &t.id).unwrap();
        assert_eq!(list_tasks(&conn, &patient_id).unwrap().len(), 0);
        assert_eq!(list_archived_tasks(&conn, &patient_id).unwrap().len(), 1);

        let restored = restore_task(&conn, &t.id).unwrap();
        assert_eq!(restored.id, t.id);
        assert_eq!(list_tasks(&conn, &patient_id).unwrap().len(), 1);
    }

    #[test]
    fn list_pending_excludes_resolved_tasks_but_history_keeps_them() {
        let conn = test_conn("list-pending-history");
        let patient_id = create_test_patient(&conn, "Paciente Diecisiete");
        let t1 = create_task(&conn, minimal_input(&patient_id)).unwrap();
        create_task(&conn, minimal_input(&patient_id)).unwrap();
        review_task(&conn, &t1.id, TherapyTaskReviewInput { status: "realizada".into(), reviewed_in_session_id: None }).unwrap();

        assert_eq!(list_pending_tasks(&conn, &patient_id).unwrap().len(), 1);
        assert_eq!(list_tasks(&conn, &patient_id).unwrap().len(), 2, "el listado activo conserva la tarea resuelta");
    }

    #[test]
    fn pending_task_count_is_global_across_patients() {
        let conn = test_conn("pending-count");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        create_task(&conn, minimal_input(&patient_a)).unwrap();
        create_task(&conn, minimal_input(&patient_b)).unwrap();
        assert_eq!(pending_task_count(&conn).unwrap(), 2);
    }
}
