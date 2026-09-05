//! Reglas de negocio de objetivos terapéuticos, sus indicadores, y su
//! vínculo N:M con sesiones. Ver `docs/goals.md` para el diseño completo.
//!
//! A diferencia de `session_notes` (Fase 4), los objetivos son registros
//! **mutables** — no hay versionado append-only aquí, es una decisión de
//! producto explícita y deliberada (ver aprobación de Fase 5).
//!
//! Regla de integridad no negociable de este módulo: un vínculo
//! sesión↔objetivo solo puede crearse si `session.patient_id ==
//! goal.patient_id`. La FK de `session_goals` no lo garantiza por sí sola
//! — se valida aquí explícitamente (`link_session_goal`).
//!
//! Esta capa nunca sabe nada de Tauri, del estado de bloqueo del vault, ni
//! toca Google Calendar en ningún punto: los objetivos no se sincronizan,
//! nunca.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::goal_indicators::{self, GoalIndicator, GoalIndicatorUpdateRow, NewGoalIndicatorRow};
use crate::repositories::goals::{self, Goal, GoalListItem, GoalUpdateRow, NewGoalRow};
use crate::repositories::patients;
use crate::repositories::session_goals::{self, GoalSessionRow, SessionGoalRow};
use crate::repositories::sessions;
use crate::services::treatment_episodes::{self, TreatmentEpisodeError};

pub const VALID_STATUSES: &[&str] = &["activo", "logrado", "pausado", "descartado"];
const DEFAULT_STATUS: &str = "activo";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalInput {
    pub patient_id: String,
    /// Opcional (Fase 9) — el proceso terapéutico al que se vincula este
    /// objetivo. `None` es válido: un objetivo puede existir sin proceso
    /// formal.
    pub episode_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub target_date: Option<String>,
}

/// Deliberadamente sin `patientId` (ver `repositories::goals::GoalUpdateRow`).
/// Incluye `status`: a diferencia de la creación (que siempre parte en
/// `activo`), la edición es donde la usuaria cambia el estado.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalUpdateInput {
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub target_date: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalIndicatorInput {
    pub description: String,
    pub baseline_value: Option<String>,
    pub target_value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionGoalLinkInput {
    pub session_id: String,
    pub goal_id: String,
    pub progress_note: Option<String>,
}

#[derive(Debug)]
pub enum GoalValidationError {
    TitleRequired,
    DateFormat,
    Status(String),
    IndicatorDescriptionRequired,
}

impl fmt::Display for GoalValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoalValidationError::TitleRequired => write!(f, "el título del objetivo es obligatorio"),
            GoalValidationError::DateFormat => write!(f, "fecha inválida (formato esperado: AAAA-MM-DD)"),
            GoalValidationError::Status(s) => {
                write!(f, "estado inválido: '{s}' (debe ser uno de: {})", VALID_STATUSES.join(", "))
            }
            GoalValidationError::IndicatorDescriptionRequired => write!(f, "la descripción del indicador es obligatoria"),
        }
    }
}
impl std::error::Error for GoalValidationError {}

#[derive(Debug)]
pub enum GoalError {
    Validation(GoalValidationError),
    NotFound,
    PatientNotFound,
    PatientArchived,
    IndicatorNotFound,
    SessionNotFound,
    PatientMismatch,
    LinkAlreadyExists,
    LinkNotFound,
    EpisodeNotFound,
    EpisodeArchived,
    EpisodeNotAssignable,
    EpisodePatientMismatch,
    /// La sesión y el objetivo que se intentan vincular tienen ambos
    /// `episode_id`, pero apuntan a procesos distintos (§9 de la
    /// aprobación de Fase 9).
    LinkEpisodeMismatch,
    Database(rusqlite::Error),
}

impl fmt::Display for GoalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoalError::Validation(e) => write!(f, "{e}"),
            GoalError::NotFound => write!(f, "objetivo no encontrado"),
            GoalError::PatientNotFound => write!(f, "paciente no encontrado"),
            GoalError::PatientArchived => write!(f, "no se pueden crear objetivos ni vínculos nuevos para un paciente archivado"),
            GoalError::IndicatorNotFound => write!(f, "indicador no encontrado"),
            GoalError::SessionNotFound => write!(f, "sesión no encontrada"),
            GoalError::PatientMismatch => write!(f, "el objetivo pertenece a otro paciente"),
            GoalError::LinkAlreadyExists => write!(f, "esta sesión ya tiene vinculado este objetivo"),
            GoalError::LinkNotFound => write!(f, "este objetivo no está vinculado a esta sesión"),
            GoalError::EpisodeNotFound => write!(f, "proceso terapéutico no encontrado"),
            GoalError::EpisodeArchived => write!(f, "este proceso está archivado y no puede recibir objetivos nuevos"),
            GoalError::EpisodeNotAssignable => write!(f, "este proceso está cerrado y no puede recibir objetivos nuevos"),
            GoalError::EpisodePatientMismatch => write!(f, "el proceso indicado no pertenece a este paciente"),
            GoalError::LinkEpisodeMismatch => write!(f, "la sesión y el objetivo pertenecen a procesos terapéuticos distintos"),
            GoalError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for GoalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GoalError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for GoalError {
    fn from(e: rusqlite::Error) -> Self {
        GoalError::Database(e)
    }
}
impl From<GoalValidationError> for GoalError {
    fn from(e: GoalValidationError) -> Self {
        GoalError::Validation(e)
    }
}
/// Traduce los errores de `treatment_episodes::check_episode_assignable` —
/// mismo criterio que `impl From<TreatmentEpisodeError> for SessionError`.
impl From<TreatmentEpisodeError> for GoalError {
    fn from(e: TreatmentEpisodeError) -> Self {
        match e {
            TreatmentEpisodeError::NotFound => GoalError::EpisodeNotFound,
            TreatmentEpisodeError::EpisodeArchived => GoalError::EpisodeArchived,
            TreatmentEpisodeError::EpisodeNotAssignable => GoalError::EpisodeNotAssignable,
            TreatmentEpisodeError::EpisodePatientMismatch => GoalError::EpisodePatientMismatch,
            TreatmentEpisodeError::Database(err) => GoalError::Database(err),
            _ => GoalError::EpisodeNotFound,
        }
    }
}

/// Mismo formato y misma forma de validación (estructural, no calendárica)
/// que `services::sessions::validate_date_format` — AAAA-MM-DD.
fn validate_date_format(value: &str) -> Result<(), GoalValidationError> {
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
        Err(GoalValidationError::DateFormat)
    }
}

fn validate_status(status: &str) -> Result<(), GoalValidationError> {
    if VALID_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(GoalValidationError::Status(status.to_string()))
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

struct ValidatedGoalFields {
    title: String,
    description: Option<String>,
    target_date: Option<String>,
}

fn validate_common(title: String, description: Option<String>, target_date: Option<String>) -> Result<ValidatedGoalFields, GoalValidationError> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(GoalValidationError::TitleRequired);
    }
    let target_date = none_if_blank(target_date);
    if let Some(ref d) = target_date {
        validate_date_format(d)?;
    }
    Ok(ValidatedGoalFields { title, description: none_if_blank(description), target_date })
}

/// Crea un objetivo. Siempre parte en estado `activo` — cambiar el estado
/// es una operación de edición posterior (`update_goal`), igual criterio
/// que `services::sessions::create_session` (que siempre crea en
/// `programada`). Rechaza la creación para un paciente inexistente o
/// archivado.
pub fn create_goal(conn: &Connection, input: GoalInput) -> Result<Goal, GoalError> {
    let patient = patients::find_by_id(conn, &input.patient_id)?.ok_or(GoalError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(GoalError::PatientArchived);
    }

    treatment_episodes::check_episode_assignable(conn, &input.episode_id, &input.patient_id)?;

    let f = validate_common(input.title, input.description, input.target_date)?;
    let id = uuid::Uuid::new_v4().to_string();
    Ok(goals::insert(
        conn,
        &NewGoalRow { id: &id, patient_id: &input.patient_id, episode_id: input.episode_id.as_deref(), title: &f.title, description: f.description.as_deref(), status: DEFAULT_STATUS, target_date: f.target_date.as_deref() },
    )?)
}

pub fn get_goal(conn: &Connection, id: &str) -> Result<Goal, GoalError> {
    goals::find_by_id(conn, id)?.ok_or(GoalError::NotFound)
}

pub fn list_goals(conn: &Connection, patient_id: &str) -> Result<Vec<GoalListItem>, GoalError> {
    Ok(goals::list_active_by_patient(conn, patient_id)?)
}

pub fn list_archived_goals(conn: &Connection, patient_id: &str) -> Result<Vec<GoalListItem>, GoalError> {
    Ok(goals::list_deleted_by_patient(conn, patient_id)?)
}

/// Objetivos relacionados con un proceso terapéutico — usada por la vista
/// de un proceso (Fase 11), con su estado actual en vivo, nunca una copia
/// congelada.
pub fn list_goals_by_episode(conn: &Connection, episode_id: &str) -> Result<Vec<GoalListItem>, GoalError> {
    Ok(goals::list_by_episode(conn, episode_id)?)
}

/// Cambia título, descripción, estado y fecha objetivo. `logrado` no es un
/// estado terminal — cualquier transición entre los cuatro estados válidos
/// es aceptada, incluida `logrado` → cualquier otro (regla explícita de la
/// aprobación de Fase 5).
pub fn update_goal(conn: &Connection, id: &str, input: GoalUpdateInput) -> Result<Goal, GoalError> {
    validate_status(&input.status)?;
    let f = validate_common(input.title, input.description, input.target_date)?;
    let row = GoalUpdateRow { title: &f.title, description: f.description.as_deref(), status: &input.status, target_date: f.target_date.as_deref() };
    goals::update(conn, id, &row)?.ok_or(GoalError::NotFound)
}

/// Soft delete únicamente. No toca indicadores ni vínculos con sesiones —
/// permanecen exactamente como estaban, consultables desde el objetivo
/// archivado.
pub fn archive_goal(conn: &Connection, id: &str) -> Result<(), GoalError> {
    if goals::soft_delete(conn, id)? {
        Ok(())
    } else {
        Err(GoalError::NotFound)
    }
}

pub fn restore_goal(conn: &Connection, id: &str) -> Result<Goal, GoalError> {
    if goals::restore(conn, id)? {
        get_goal(conn, id)
    } else {
        Err(GoalError::NotFound)
    }
}

pub fn list_indicators(conn: &Connection, goal_id: &str) -> Result<Vec<GoalIndicator>, GoalError> {
    goals::find_by_id(conn, goal_id)?.ok_or(GoalError::NotFound)?;
    Ok(goal_indicators::list_by_goal(conn, goal_id)?)
}

/// Un objetivo puede existir sin indicadores — esto no es un requisito de
/// creación, solo una acción que se puede hacer en cualquier momento
/// después, incluso sobre un objetivo archivado (archivar no bloquea la
/// edición de sus datos hijos, mismo criterio que
/// `services::sessions::autosave_note_draft` no revisa si la sesión está
/// archivada).
pub fn create_indicator(conn: &Connection, goal_id: &str, input: GoalIndicatorInput) -> Result<GoalIndicator, GoalError> {
    goals::find_by_id(conn, goal_id)?.ok_or(GoalError::NotFound)?;
    let description = input.description.trim().to_string();
    if description.is_empty() {
        return Err(GoalValidationError::IndicatorDescriptionRequired.into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    Ok(goal_indicators::insert(
        conn,
        &NewGoalIndicatorRow {
            id: &id,
            goal_id,
            description: &description,
            baseline_value: none_if_blank(input.baseline_value).as_deref(),
            target_value: none_if_blank(input.target_value).as_deref(),
        },
    )?)
}

pub fn update_indicator(conn: &Connection, indicator_id: &str, input: GoalIndicatorInput) -> Result<GoalIndicator, GoalError> {
    let description = input.description.trim().to_string();
    if description.is_empty() {
        return Err(GoalValidationError::IndicatorDescriptionRequired.into());
    }
    let baseline_value = none_if_blank(input.baseline_value);
    let target_value = none_if_blank(input.target_value);
    let row = GoalIndicatorUpdateRow {
        description: &description,
        baseline_value: baseline_value.as_deref(),
        target_value: target_value.as_deref(),
    };
    goal_indicators::update(conn, indicator_id, &row)?.ok_or(GoalError::IndicatorNotFound)
}

pub fn delete_indicator(conn: &Connection, indicator_id: &str) -> Result<(), GoalError> {
    if goal_indicators::delete(conn, indicator_id)? {
        Ok(())
    } else {
        Err(GoalError::IndicatorNotFound)
    }
}

/// El único punto del sistema donde se crea un vínculo sesión↔objetivo.
/// Verifica, en este orden: la sesión existe, el objetivo existe, ambos
/// pertenecen al mismo paciente (`PatientMismatch` si no), ese paciente no
/// está archivado (`PatientArchived` — regla 18 de la aprobación: no
/// crear vínculos *nuevos* para un paciente archivado), y que el vínculo
/// no exista ya (`LinkAlreadyExists`).
pub fn link_session_goal(conn: &Connection, input: SessionGoalLinkInput) -> Result<(), GoalError> {
    let session = sessions::find_by_id(conn, &input.session_id)?.ok_or(GoalError::SessionNotFound)?;
    let goal = goals::find_by_id(conn, &input.goal_id)?.ok_or(GoalError::NotFound)?;
    if session.patient_id != goal.patient_id {
        return Err(GoalError::PatientMismatch);
    }
    // Fase 9: si ambos tienen episode_id, deben coincidir. Si alguno no
    // tiene (el caso más común hoy, antes de que episode_id se generalice),
    // se mantiene exactamente el comportamiento previo — sin rechazar nada
    // nuevo (§9 de la aprobación de Fase 9).
    if let (Some(session_episode), Some(goal_episode)) = (&session.episode_id, &goal.episode_id) {
        if session_episode != goal_episode {
            return Err(GoalError::LinkEpisodeMismatch);
        }
    }
    let patient = patients::find_by_id(conn, &goal.patient_id)?.ok_or(GoalError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(GoalError::PatientArchived);
    }
    if session_goals::exists(conn, &input.session_id, &input.goal_id)? {
        return Err(GoalError::LinkAlreadyExists);
    }
    session_goals::link(conn, &input.session_id, &input.goal_id, none_if_blank(input.progress_note).as_deref())?;
    Ok(())
}

/// Quitar un vínculo no es "crear algo nuevo" — se permite aunque el
/// paciente esté archivado, mismo criterio que archivar/restaurar una
/// sesión no revisa el estado del paciente.
pub fn unlink_session_goal(conn: &Connection, session_id: &str, goal_id: &str) -> Result<(), GoalError> {
    if session_goals::unlink(conn, session_id, goal_id)? {
        Ok(())
    } else {
        Err(GoalError::LinkNotFound)
    }
}

pub fn update_link_progress_note(conn: &Connection, session_id: &str, goal_id: &str, progress_note: Option<String>) -> Result<(), GoalError> {
    if session_goals::update_progress_note(conn, session_id, goal_id, none_if_blank(progress_note).as_deref())? {
        Ok(())
    } else {
        Err(GoalError::LinkNotFound)
    }
}

pub fn list_goals_for_session(conn: &Connection, session_id: &str) -> Result<Vec<SessionGoalRow>, GoalError> {
    sessions::find_by_id(conn, session_id)?.ok_or(GoalError::SessionNotFound)?;
    Ok(session_goals::list_for_session(conn, session_id)?)
}

pub fn list_sessions_for_goal(conn: &Connection, goal_id: &str) -> Result<Vec<GoalSessionRow>, GoalError> {
    goals::find_by_id(conn, goal_id)?.ok_or(GoalError::NotFound)?;
    Ok(session_goals::list_for_goal(conn, goal_id)?)
}

/// Objetivos activos del paciente de esa sesión que **todavía no** están
/// vinculados a ella — exactamente lo que necesita el selector "Agregar
/// objetivo" de `SessionDetailScreen`, calculado en el backend para que el
/// frontend nunca tenga que filtrar objetivos de otro paciente por su
/// cuenta (regla 14 de la aprobación: nunca mostrar objetivos de otro
/// paciente).
pub fn list_available_goals_for_session(conn: &Connection, session_id: &str) -> Result<Vec<GoalListItem>, GoalError> {
    let session = sessions::find_by_id(conn, session_id)?.ok_or(GoalError::SessionNotFound)?;
    let active_goals = goals::list_active_by_patient(conn, &session.patient_id)?;
    let linked = session_goals::list_for_session(conn, session_id)?;
    let linked_ids: std::collections::HashSet<&str> = linked.iter().map(|l| l.goal_id.as_str()).collect();
    Ok(active_goals.into_iter().filter(|g| !linked_ids.contains(g.id.as_str())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::services::patients::{self, PatientInput};
    use crate::services::sessions::{self as session_service, SessionInput};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-goals-service-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x24u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_patient(conn: &Connection, name: &str) -> String {
        let input = PatientInput {
            full_name: name.to_string(),
            preferred_name: None,
            rut: None,
            birth_date: None,
            phone: None,
            email: None,
            address: None,
            emergency_contact_name: None,
            emergency_contact_phone: None,
            emergency_contact_relationship: None,
            status: None,
            referred_by: None,
            intake_date: None,
            region: None,
            commune: None,
        };
        patients::create_patient(conn, input).unwrap().id
    }

    fn create_test_session(conn: &Connection, patient_id: &str) -> String {
        let input = SessionInput {
            patient_id: patient_id.to_string(),
            appointment_id: None,
            episode_id: None,
            session_date: "2026-09-01".to_string(),
            start_time: Some("15:00".to_string()),
            duration_minutes: Some(50),
            modality: Some("presencial".to_string()),
        };
        session_service::create_session(conn, input).unwrap().session.id
    }

    fn minimal_goal_input(patient_id: &str) -> GoalInput {
        GoalInput { patient_id: patient_id.to_string(), episode_id: None, title: "Reducir ansiedad".to_string(), description: None, target_date: None }
    }

    fn create_test_episode(conn: &Connection, patient_id: &str) -> String {
        treatment_episodes::create_episode(conn, crate::services::treatment_episodes::TreatmentEpisodeInput { patient_id: patient_id.to_string(), started_at: None }).unwrap().id
    }

    // ---- Fase 9: episode_id opcional en objetivos ----

    #[test]
    fn a_goal_can_be_created_without_an_episode() {
        let conn = test_conn("goal-episode-none");
        let patient_id = create_test_patient(&conn, "Paciente Sin Proceso");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        assert!(goal.episode_id.is_none());
    }

    #[test]
    fn a_goal_can_be_created_with_a_valid_episode_of_the_same_patient() {
        let conn = test_conn("goal-episode-valid");
        let patient_id = create_test_patient(&conn, "Paciente Con Proceso");
        let episode_id = create_test_episode(&conn, &patient_id);
        let mut input = minimal_goal_input(&patient_id);
        input.episode_id = Some(episode_id.clone());
        let goal = create_goal(&conn, input).unwrap();
        assert_eq!(goal.episode_id.as_deref(), Some(episode_id.as_str()));
    }

    #[test]
    fn goal_creation_rejects_an_episode_belonging_to_a_different_patient() {
        let conn = test_conn("goal-episode-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let episode_of_a = create_test_episode(&conn, &patient_a);
        let mut input = minimal_goal_input(&patient_b);
        input.episode_id = Some(episode_of_a);
        let err = create_goal(&conn, input).unwrap_err();
        assert!(matches!(err, GoalError::EpisodePatientMismatch));
    }

    #[test]
    fn goal_creation_rejects_a_nonexistent_episode() {
        let conn = test_conn("goal-episode-not-found");
        let patient_id = create_test_patient(&conn, "Paciente C");
        let mut input = minimal_goal_input(&patient_id);
        input.episode_id = Some("no-existe".to_string());
        let err = create_goal(&conn, input).unwrap_err();
        assert!(matches!(err, GoalError::EpisodeNotFound));
    }

    #[test]
    fn goal_creation_rejects_a_closed_episode() {
        let conn = test_conn("goal-episode-closed");
        let patient_id = create_test_patient(&conn, "Paciente D");
        let episode_id = create_test_episode(&conn, &patient_id);
        crate::repositories::treatment_episodes::set_status(&conn, &episode_id, "cerrado").unwrap();
        let mut input = minimal_goal_input(&patient_id);
        input.episode_id = Some(episode_id);
        let err = create_goal(&conn, input).unwrap_err();
        assert!(matches!(err, GoalError::EpisodeNotAssignable));
    }

    #[test]
    fn linking_a_session_and_goal_of_the_same_episode_succeeds() {
        let conn = test_conn("link-same-episode");
        let patient_id = create_test_patient(&conn, "Paciente E");
        let episode_id = create_test_episode(&conn, &patient_id);
        let mut goal_input = minimal_goal_input(&patient_id);
        goal_input.episode_id = Some(episode_id.clone());
        let goal_id = create_goal(&conn, goal_input).unwrap().id;

        let session_input = SessionInput {
            patient_id: patient_id.clone(), appointment_id: None, episode_id: Some(episode_id),
            session_date: "2026-09-01".to_string(), start_time: None, duration_minutes: None, modality: None,
        };
        let session_id = session_service::create_session(&conn, session_input).unwrap().session.id;

        link_session_goal(&conn, SessionGoalLinkInput { session_id, goal_id, progress_note: None }).unwrap();
    }

    #[test]
    fn linking_a_session_and_goal_of_different_episodes_is_rejected() {
        let conn = test_conn("link-different-episodes");
        let patient_id = create_test_patient(&conn, "Paciente F");
        let episode_a = create_test_episode(&conn, &patient_id);
        let mut goal_input = minimal_goal_input(&patient_id);
        goal_input.episode_id = Some(episode_a);
        let goal_id = create_goal(&conn, goal_input).unwrap().id;

        // Pausar el primer proceso para poder abrir uno segundo y así tener
        // dos episode_id distintos y válidos para el mismo paciente.
        treatment_episodes::set_episode_status(&conn, goals::find_by_id(&conn, &goal_id).unwrap().unwrap().episode_id.as_deref().unwrap(), "pausado").unwrap();
        let episode_b = create_test_episode(&conn, &patient_id);

        let session_input = SessionInput {
            patient_id: patient_id.clone(), appointment_id: None, episode_id: Some(episode_b),
            session_date: "2026-09-01".to_string(), start_time: None, duration_minutes: None, modality: None,
        };
        let session_id = session_service::create_session(&conn, session_input).unwrap().session.id;

        let err = link_session_goal(&conn, SessionGoalLinkInput { session_id, goal_id, progress_note: None }).unwrap_err();
        assert!(matches!(err, GoalError::LinkEpisodeMismatch));
    }

    #[test]
    fn linking_still_works_when_only_one_side_has_an_episode() {
        // Compatibilidad explícita con el comportamiento previo a Fase 9:
        // si la sesión o el objetivo no tienen episode_id, el vínculo no se
        // rechaza por eso.
        let conn = test_conn("link-one-sided-episode");
        let patient_id = create_test_patient(&conn, "Paciente G");
        let episode_id = create_test_episode(&conn, &patient_id);
        let mut goal_input = minimal_goal_input(&patient_id);
        goal_input.episode_id = Some(episode_id);
        let goal_id = create_goal(&conn, goal_input).unwrap().id;

        // Sesión SIN episode_id.
        let session_id = create_test_session(&conn, &patient_id);

        link_session_goal(&conn, SessionGoalLinkInput { session_id, goal_id, progress_note: None }).unwrap();
    }

    // ---- creación de objetivos ----

    #[test]
    fn creates_a_goal_defaulting_to_activo() {
        let conn = test_conn("create-defaults");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        assert_eq!(goal.patient_id, patient_id);
        assert_eq!(goal.status, "activo");
        assert!(goal.formulation_id.is_none());
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let conn = test_conn("create-nonexistent-patient");
        let err = create_goal(&conn, minimal_goal_input("no-existe")).unwrap_err();
        assert!(matches!(err, GoalError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let conn = test_conn("create-archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Archivado");
        patients::archive_patient(&conn, &patient_id).unwrap();
        let err = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap_err();
        assert!(matches!(err, GoalError::PatientArchived));
    }

    #[test]
    fn rejects_a_blank_title() {
        let conn = test_conn("create-blank-title");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let mut input = minimal_goal_input(&patient_id);
        input.title = "   ".to_string();
        let err = create_goal(&conn, input).unwrap_err();
        assert!(matches!(err, GoalError::Validation(GoalValidationError::TitleRequired)));
    }

    #[test]
    fn rejects_an_invalid_target_date() {
        let conn = test_conn("create-invalid-date");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let mut input = minimal_goal_input(&patient_id);
        input.target_date = Some("no-es-fecha".to_string());
        let err = create_goal(&conn, input).unwrap_err();
        assert!(matches!(err, GoalError::Validation(GoalValidationError::DateFormat)));
    }

    #[test]
    fn a_goal_can_exist_without_any_indicator() {
        let conn = test_conn("no-indicators");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        assert!(list_indicators(&conn, &goal.id).unwrap().is_empty());
    }

    // ---- edición y estados ----

    #[test]
    fn rejects_an_invalid_status_on_update() {
        let conn = test_conn("update-invalid-status");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        let err = update_goal(&conn, &goal.id, GoalUpdateInput { title: "T".to_string(), description: None, status: "inventado".to_string(), target_date: None }).unwrap_err();
        assert!(matches!(err, GoalError::Validation(GoalValidationError::Status(_))));
    }

    #[test]
    fn logrado_is_not_a_terminal_state() {
        let conn = test_conn("logrado-not-terminal");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();

        let logrado = update_goal(&conn, &goal.id, GoalUpdateInput { title: goal.title.clone(), description: None, status: "logrado".to_string(), target_date: None }).unwrap();
        assert_eq!(logrado.status, "logrado");

        let vuelto_a_activo = update_goal(&conn, &goal.id, GoalUpdateInput { title: goal.title.clone(), description: None, status: "activo".to_string(), target_date: None }).unwrap();
        assert_eq!(vuelto_a_activo.status, "activo");

        let pausado = update_goal(&conn, &goal.id, GoalUpdateInput { title: goal.title.clone(), description: None, status: "pausado".to_string(), target_date: None }).unwrap();
        assert_eq!(pausado.status, "pausado");
    }

    #[test]
    fn updating_a_nonexistent_goal_reports_not_found() {
        let conn = test_conn("update-nonexistent");
        let err = update_goal(&conn, "no-existe", GoalUpdateInput { title: "T".to_string(), description: None, status: "activo".to_string(), target_date: None }).unwrap_err();
        assert!(matches!(err, GoalError::NotFound));
    }

    // ---- archivado ----

    #[test]
    fn archiving_hides_from_active_listing_but_keeps_indicators_and_links_intact() {
        let conn = test_conn("archive-keeps-data");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        create_indicator(&conn, &goal.id, GoalIndicatorInput { description: "Indicador".to_string(), baseline_value: None, target_value: None }).unwrap();
        let session_id = create_test_session(&conn, &patient_id);
        link_session_goal(&conn, SessionGoalLinkInput { session_id: session_id.clone(), goal_id: goal.id.clone(), progress_note: None }).unwrap();

        archive_goal(&conn, &goal.id).unwrap();

        assert!(list_goals(&conn, &patient_id).unwrap().is_empty());
        assert_eq!(list_archived_goals(&conn, &patient_id).unwrap().len(), 1);
        assert_eq!(list_indicators(&conn, &goal.id).unwrap().len(), 1);
        assert_eq!(list_sessions_for_goal(&conn, &goal.id).unwrap().len(), 1);
    }

    #[test]
    fn restoring_brings_it_back_to_the_active_listing_with_everything_intact() {
        let conn = test_conn("restore");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        create_indicator(&conn, &goal.id, GoalIndicatorInput { description: "Indicador".to_string(), baseline_value: None, target_value: None }).unwrap();
        archive_goal(&conn, &goal.id).unwrap();

        let restored = restore_goal(&conn, &goal.id).unwrap();
        assert!(restored.deleted_at.is_none());
        assert_eq!(list_goals(&conn, &patient_id).unwrap().len(), 1);
        assert_eq!(list_indicators(&conn, &goal.id).unwrap().len(), 1);
    }

    #[test]
    fn archiving_an_unknown_goal_reports_not_found() {
        let conn = test_conn("archive-unknown");
        assert!(matches!(archive_goal(&conn, "no-existe").unwrap_err(), GoalError::NotFound));
    }

    // ---- indicadores ----

    #[test]
    fn creates_edits_and_deletes_an_indicator() {
        let conn = test_conn("indicator-crud");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();

        let indicator = create_indicator(&conn, &goal.id, GoalIndicatorInput { description: "Frecuencia de crisis".to_string(), baseline_value: Some("3/semana".to_string()), target_value: Some("0/semana".to_string()) }).unwrap();
        assert_eq!(indicator.goal_id, goal.id);

        let updated = update_indicator(&conn, &indicator.id, GoalIndicatorInput { description: "Frecuencia de crisis (editado)".to_string(), baseline_value: Some("2/semana".to_string()), target_value: Some("0/semana".to_string()) }).unwrap();
        assert_eq!(updated.description, "Frecuencia de crisis (editado)");

        delete_indicator(&conn, &indicator.id).unwrap();
        assert!(list_indicators(&conn, &goal.id).unwrap().is_empty());
    }

    #[test]
    fn rejects_creating_an_indicator_for_a_nonexistent_goal() {
        let conn = test_conn("indicator-nonexistent-goal");
        let err = create_indicator(&conn, "no-existe", GoalIndicatorInput { description: "X".to_string(), baseline_value: None, target_value: None }).unwrap_err();
        assert!(matches!(err, GoalError::NotFound));
    }

    #[test]
    fn rejects_a_blank_indicator_description() {
        let conn = test_conn("indicator-blank-description");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        let err = create_indicator(&conn, &goal.id, GoalIndicatorInput { description: "   ".to_string(), baseline_value: None, target_value: None }).unwrap_err();
        assert!(matches!(err, GoalError::Validation(GoalValidationError::IndicatorDescriptionRequired)));
    }

    #[test]
    fn deleting_an_unknown_indicator_reports_not_found() {
        let conn = test_conn("indicator-delete-unknown");
        assert!(matches!(delete_indicator(&conn, "no-existe").unwrap_err(), GoalError::IndicatorNotFound));
    }

    #[test]
    fn a_goal_can_have_multiple_indicators() {
        let conn = test_conn("indicator-multiple");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        create_indicator(&conn, &goal.id, GoalIndicatorInput { description: "Uno".to_string(), baseline_value: None, target_value: None }).unwrap();
        create_indicator(&conn, &goal.id, GoalIndicatorInput { description: "Dos".to_string(), baseline_value: None, target_value: None }).unwrap();
        assert_eq!(list_indicators(&conn, &goal.id).unwrap().len(), 2);
    }

    // ---- vínculo sesión↔objetivo: la regla de integridad crítica ----

    #[test]
    fn rejects_linking_a_session_of_one_patient_with_a_goal_of_another() {
        let conn = test_conn("cross-patient-link-rejected");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let session_a = create_test_session(&conn, &patient_a);
        let goal_b = create_goal(&conn, minimal_goal_input(&patient_b)).unwrap();

        let err = link_session_goal(&conn, SessionGoalLinkInput { session_id: session_a, goal_id: goal_b.id, progress_note: None }).unwrap_err();
        assert!(matches!(err, GoalError::PatientMismatch));
    }

    #[test]
    fn links_a_session_and_goal_of_the_same_patient() {
        let conn = test_conn("same-patient-link");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let session_id = create_test_session(&conn, &patient_id);
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();

        link_session_goal(&conn, SessionGoalLinkInput { session_id: session_id.clone(), goal_id: goal.id.clone(), progress_note: Some("Buen avance".to_string()) }).unwrap();

        let from_session = list_goals_for_session(&conn, &session_id).unwrap();
        assert_eq!(from_session.len(), 1);
        assert_eq!(from_session[0].progress_note.as_deref(), Some("Buen avance"));

        let from_goal = list_sessions_for_goal(&conn, &goal.id).unwrap();
        assert_eq!(from_goal.len(), 1);
        assert_eq!(from_goal[0].session_id, session_id);
    }

    #[test]
    fn rejects_linking_to_a_nonexistent_session() {
        let conn = test_conn("link-nonexistent-session");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        let err = link_session_goal(&conn, SessionGoalLinkInput { session_id: "no-existe".to_string(), goal_id: goal.id, progress_note: None }).unwrap_err();
        assert!(matches!(err, GoalError::SessionNotFound));
    }

    #[test]
    fn rejects_linking_to_a_nonexistent_goal() {
        let conn = test_conn("link-nonexistent-goal");
        let patient_id = create_test_patient(&conn, "Paciente Catorce");
        let session_id = create_test_session(&conn, &patient_id);
        let err = link_session_goal(&conn, SessionGoalLinkInput { session_id, goal_id: "no-existe".to_string(), progress_note: None }).unwrap_err();
        assert!(matches!(err, GoalError::NotFound));
    }

    #[test]
    fn rejects_a_duplicate_link() {
        let conn = test_conn("link-duplicate");
        let patient_id = create_test_patient(&conn, "Paciente Quince");
        let session_id = create_test_session(&conn, &patient_id);
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        link_session_goal(&conn, SessionGoalLinkInput { session_id: session_id.clone(), goal_id: goal.id.clone(), progress_note: None }).unwrap();

        let err = link_session_goal(&conn, SessionGoalLinkInput { session_id, goal_id: goal.id, progress_note: None }).unwrap_err();
        assert!(matches!(err, GoalError::LinkAlreadyExists));
    }

    #[test]
    fn rejects_creating_a_new_link_for_an_archived_patient() {
        let conn = test_conn("link-archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Dieciséis");
        let session_id = create_test_session(&conn, &patient_id);
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        patients::archive_patient(&conn, &patient_id).unwrap();

        let err = link_session_goal(&conn, SessionGoalLinkInput { session_id, goal_id: goal.id, progress_note: None }).unwrap_err();
        assert!(matches!(err, GoalError::PatientArchived));
    }

    #[test]
    fn unlink_removes_the_link_and_is_reported_as_not_found_when_repeated() {
        let conn = test_conn("unlink");
        let patient_id = create_test_patient(&conn, "Paciente Diecisiete");
        let session_id = create_test_session(&conn, &patient_id);
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        link_session_goal(&conn, SessionGoalLinkInput { session_id: session_id.clone(), goal_id: goal.id.clone(), progress_note: None }).unwrap();

        unlink_session_goal(&conn, &session_id, &goal.id).unwrap();
        assert!(list_goals_for_session(&conn, &session_id).unwrap().is_empty());

        let err = unlink_session_goal(&conn, &session_id, &goal.id).unwrap_err();
        assert!(matches!(err, GoalError::LinkNotFound));
    }

    #[test]
    fn a_session_can_have_multiple_goals_and_a_goal_can_have_multiple_sessions() {
        let conn = test_conn("multi-multi");
        let patient_id = create_test_patient(&conn, "Paciente Dieciocho");
        let session_a = create_test_session(&conn, &patient_id);
        let session_b = create_test_session(&conn, &patient_id);
        let goal_a = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        let goal_b = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();

        link_session_goal(&conn, SessionGoalLinkInput { session_id: session_a.clone(), goal_id: goal_a.id.clone(), progress_note: None }).unwrap();
        link_session_goal(&conn, SessionGoalLinkInput { session_id: session_a.clone(), goal_id: goal_b.id.clone(), progress_note: None }).unwrap();
        link_session_goal(&conn, SessionGoalLinkInput { session_id: session_b.clone(), goal_id: goal_a.id.clone(), progress_note: None }).unwrap();

        assert_eq!(list_goals_for_session(&conn, &session_a).unwrap().len(), 2);
        assert_eq!(list_sessions_for_goal(&conn, &goal_a.id).unwrap().len(), 2);
    }

    #[test]
    fn available_goals_for_session_excludes_already_linked_ones_and_other_patients() {
        let conn = test_conn("available-goals");
        let patient_a = create_test_patient(&conn, "Paciente Diecinueve");
        let patient_b = create_test_patient(&conn, "Paciente Veinte");
        let session_id = create_test_session(&conn, &patient_a);
        let goal_linked = create_goal(&conn, minimal_goal_input(&patient_a)).unwrap();
        let goal_available = create_goal(&conn, minimal_goal_input(&patient_a)).unwrap();
        let _goal_other_patient = create_goal(&conn, minimal_goal_input(&patient_b)).unwrap();
        link_session_goal(&conn, SessionGoalLinkInput { session_id: session_id.clone(), goal_id: goal_linked.id.clone(), progress_note: None }).unwrap();

        let available = list_available_goals_for_session(&conn, &session_id).unwrap();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].id, goal_available.id);
    }

    #[test]
    fn update_link_progress_note_changes_it() {
        let conn = test_conn("update-progress-note");
        let patient_id = create_test_patient(&conn, "Paciente Veintiuno");
        let session_id = create_test_session(&conn, &patient_id);
        let goal = create_goal(&conn, minimal_goal_input(&patient_id)).unwrap();
        link_session_goal(&conn, SessionGoalLinkInput { session_id: session_id.clone(), goal_id: goal.id.clone(), progress_note: None }).unwrap();

        update_link_progress_note(&conn, &session_id, &goal.id, Some("Actualizado".to_string())).unwrap();
        let from_session = list_goals_for_session(&conn, &session_id).unwrap();
        assert_eq!(from_session[0].progress_note.as_deref(), Some("Actualizado"));
    }
}
