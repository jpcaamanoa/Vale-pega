//! Reglas de negocio de la Formulación Clínica Textual Versionada (Fase
//! 15). Ver `docs/formulation.md` para el diseño completo y
//! `Plan-Fase-15-pendiente-de-aprobacion.md` para el análisis de esquema
//! que motivó `SCHEMA_V8`.
//!
//! La formulación es **una hipótesis clínica de trabajo, escrita por la
//! profesional, que puede cambiar con nueva información**. Por eso: se
//! versiona (nunca se sobrescribe), nunca se genera automáticamente, nunca
//! se confunde con un diagnóstico, y nunca se copia silenciosamente entre
//! procesos.
//!
//! **Una formulación siempre pertenece a un proceso terapéutico concreto**
//! (`episode_id` es obligatorio en este servicio, aunque nullable en el
//! esquema por consistencia con `sessions.episode_id`/
//! `assessment_administrations.episode_id`): permitir una formulación
//! "suelta", sin proceso, reintroduciría exactamente la ambigüedad que
//! motivó agregar la columna — que la última formulación del paciente
//! parezca vigente sin importar qué proceso esté activo. Como máximo una
//! formulación principal por proceso, garantizado también a nivel de base
//! de datos (`idx_case_formulations_one_per_episode`).
//!
//! Nunca hay estado de "borrador": cada "Actualizar formulación" crea
//! directamente una versión nueva confirmada. La versión actual es siempre
//! `MAX(version_number)` — nunca hay una columna `status`/`is_current`
//! separada, porque el propio esquema de `formulation_versions` no la
//! tiene y agregarla no se justificó (ver `docs/formulation.md`).
//!
//! `formulation_nodes`/`formulation_edges` nunca se leen ni se escriben
//! desde este servicio — reservadas para una futura Formulación Visual.
//!
//! Esta capa nunca sabe nada de Tauri, del estado de bloqueo del vault, ni
//! toca Google Calendar en ningún punto.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::formulations::{self, CaseFormulation, FormulationSummary, FormulationVersion, NewFormulationRow, NewFormulationVersionRow};
use crate::repositories::patients;
use crate::services::treatment_episodes::{self, TreatmentEpisodeError};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulationInput {
    pub patient_id: String,
    pub episode_id: String,
    pub title: String,
    pub model_type: Option<String>,
    /// Contenido de la primera versión (`summary_text`) — el frontend lo
    /// serializa a partir de sus secciones estructuradas; este servicio lo
    /// trata como texto libre, sin interpretar ni exigir ninguna sección.
    pub summary_text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewVersionInput {
    pub summary_text: Option<String>,
}

#[derive(Debug)]
pub enum FormulationValidationError {
    TitleRequired,
}

impl fmt::Display for FormulationValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormulationValidationError::TitleRequired => write!(f, "el título de la formulación es obligatorio"),
        }
    }
}
impl std::error::Error for FormulationValidationError {}

#[derive(Debug)]
pub enum FormulationError {
    Validation(FormulationValidationError),
    NotFound,
    PatientNotFound,
    PatientArchived,
    EpisodeNotFound,
    EpisodeArchived,
    EpisodeNotAssignable,
    EpisodePatientMismatch,
    /// El proceso indicado ya tiene una formulación principal — `SCHEMA_V8`
    /// también lo garantiza con un índice único parcial; este error da un
    /// mensaje claro antes de llegar a esa violación cruda de SQL.
    FormulationAlreadyExistsForEpisode,
    Database(rusqlite::Error),
}

impl fmt::Display for FormulationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormulationError::Validation(e) => write!(f, "{e}"),
            FormulationError::NotFound => write!(f, "formulación no encontrada"),
            FormulationError::PatientNotFound => write!(f, "paciente no encontrado"),
            FormulationError::PatientArchived => write!(f, "no se pueden crear ni actualizar formulaciones para un paciente archivado"),
            FormulationError::EpisodeNotFound => write!(f, "proceso terapéutico no encontrado"),
            FormulationError::EpisodeArchived => write!(f, "este proceso está archivado y no puede recibir una formulación nueva"),
            FormulationError::EpisodeNotAssignable => write!(f, "este proceso está cerrado y no puede recibir una formulación nueva ni una versión nueva"),
            FormulationError::EpisodePatientMismatch => write!(f, "el proceso indicado pertenece a otro paciente"),
            FormulationError::FormulationAlreadyExistsForEpisode => write!(f, "este proceso ya tiene una formulación principal"),
            FormulationError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for FormulationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FormulationError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for FormulationError {
    fn from(e: rusqlite::Error) -> Self {
        FormulationError::Database(e)
    }
}
impl From<FormulationValidationError> for FormulationError {
    fn from(e: FormulationValidationError) -> Self {
        FormulationError::Validation(e)
    }
}
/// Traduce los errores de `treatment_episodes::check_episode_assignable` a
/// sus equivalentes de dominio de formulación — mismo criterio que
/// `impl From<TreatmentEpisodeError> for AssessmentError`.
impl From<TreatmentEpisodeError> for FormulationError {
    fn from(e: TreatmentEpisodeError) -> Self {
        match e {
            TreatmentEpisodeError::NotFound => FormulationError::EpisodeNotFound,
            TreatmentEpisodeError::EpisodeArchived => FormulationError::EpisodeArchived,
            TreatmentEpisodeError::EpisodeNotAssignable => FormulationError::EpisodeNotAssignable,
            TreatmentEpisodeError::EpisodePatientMismatch => FormulationError::EpisodePatientMismatch,
            TreatmentEpisodeError::Database(err) => FormulationError::Database(err),
            _ => FormulationError::EpisodeNotFound,
        }
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

fn validate_title(title: String) -> Result<String, FormulationValidationError> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(FormulationValidationError::TitleRequired);
    }
    Ok(title)
}

/// Crea la formulación principal de un proceso, con su primera versión
/// (`version_number = 1`), en una única transacción atómica.
///
/// Rechaza: paciente inexistente o archivado, proceso inexistente/de otro
/// paciente/archivado/cerrado (vía `check_episode_assignable`), y un
/// proceso que ya tenga una formulación principal.
pub fn create_formulation(conn: &Connection, input: FormulationInput) -> Result<(CaseFormulation, FormulationVersion), FormulationError> {
    let patient = patients::find_by_id(conn, &input.patient_id)?.ok_or(FormulationError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(FormulationError::PatientArchived);
    }
    treatment_episodes::check_episode_assignable(conn, &Some(input.episode_id.clone()), &input.patient_id)?;
    if formulations::find_formulation_by_episode(conn, &input.episode_id)?.is_some() {
        return Err(FormulationError::FormulationAlreadyExistsForEpisode);
    }

    let title = validate_title(input.title)?;
    let model_type = none_if_blank(input.model_type);
    let summary_text = none_if_blank(input.summary_text);

    let formulation_id = uuid::Uuid::new_v4().to_string();
    let version_id = uuid::Uuid::new_v4().to_string();
    let tx = conn.unchecked_transaction()?;
    let formulation = formulations::insert_formulation(
        &tx,
        &NewFormulationRow { id: &formulation_id, patient_id: &input.patient_id, episode_id: &input.episode_id, title: &title, model_type: model_type.as_deref() },
    )?;
    let version = formulations::insert_version(&tx, &NewFormulationVersionRow { id: &version_id, formulation_id: &formulation_id, version_number: 1, summary_text: summary_text.as_deref() })?;
    tx.commit()?;

    Ok((formulation, version))
}

pub fn get_formulation(conn: &Connection, id: &str) -> Result<CaseFormulation, FormulationError> {
    formulations::find_formulation_by_id(conn, id)?.ok_or(FormulationError::NotFound)
}

pub fn get_formulation_by_episode(conn: &Connection, episode_id: &str) -> Result<Option<CaseFormulation>, FormulationError> {
    Ok(formulations::find_formulation_by_episode(conn, episode_id)?)
}

pub fn list_formulations(conn: &Connection, patient_id: &str) -> Result<Vec<FormulationSummary>, FormulationError> {
    Ok(formulations::list_formulations_by_patient(conn, patient_id)?)
}

pub fn get_current_version(conn: &Connection, formulation_id: &str) -> Result<FormulationVersion, FormulationError> {
    formulations::latest_version(conn, formulation_id)?.ok_or(FormulationError::NotFound)
}

pub fn get_version(conn: &Connection, version_id: &str) -> Result<FormulationVersion, FormulationError> {
    formulations::find_version_by_id(conn, version_id)?.ok_or(FormulationError::NotFound)
}

/// Historial completo de versiones, más reciente primero. Todas las
/// versiones anteriores son de solo lectura desde el punto de vista de
/// este servicio: no existe ninguna función de `update` sobre
/// `summary_text` de una versión ya creada.
pub fn list_versions(conn: &Connection, formulation_id: &str) -> Result<Vec<FormulationVersion>, FormulationError> {
    let _ = get_formulation(conn, formulation_id)?;
    Ok(formulations::list_versions(conn, formulation_id)?)
}

/// "Actualizar formulación": crea una versión nueva, nunca sobrescribe la
/// anterior. No vuelve a comprobar el archivado del paciente por sí solo
/// aquí — la comprobación explícita vive en esta función porque, a
/// diferencia de una simple edición de contenido mutable, cada nueva
/// versión es contenido clínico nuevo (mismo criterio que
/// `services::safety_plans::require_editable_draft`): un paciente
/// archivado no puede recibir versiones nuevas de ninguna formulación.
/// Tampoco puede crearse una versión nueva si el proceso vinculado está
/// cerrado o archivado — reutiliza `check_episode_assignable` sobre el
/// `episode_id` ya almacenado en la formulación.
pub fn create_new_version(conn: &Connection, formulation_id: &str, input: NewVersionInput) -> Result<(CaseFormulation, FormulationVersion), FormulationError> {
    let formulation = formulations::find_formulation_by_id(conn, formulation_id)?.ok_or(FormulationError::NotFound)?;
    let patient = patients::find_by_id(conn, &formulation.patient_id)?.ok_or(FormulationError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(FormulationError::PatientArchived);
    }
    treatment_episodes::check_episode_assignable(conn, &formulation.episode_id, &formulation.patient_id)?;

    let summary_text = none_if_blank(input.summary_text);
    let current = formulations::latest_version(conn, formulation_id)?.ok_or(FormulationError::NotFound)?;
    let version_id = uuid::Uuid::new_v4().to_string();
    let version = formulations::insert_version(
        conn,
        &NewFormulationVersionRow { id: &version_id, formulation_id, version_number: current.version_number + 1, summary_text: summary_text.as_deref() },
    )?;
    Ok((formulation, version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self as patients_repo, NewPatientRow};
    use crate::repositories::treatment_episodes::{self as episodes_repo, NewTreatmentEpisodeRow};
    use crate::services::patients as patients_service;
    use crate::services::treatment_episodes as episodes_service;

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-formulations-svc-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x92u8; VAULT_KEY_LEN]);
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

    fn create_test_episode(conn: &Connection, patient_id: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        episodes_repo::insert(conn, &NewTreatmentEpisodeRow { id: &id, patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        id
    }

    fn minimal_input(patient_id: &str, episode_id: &str) -> FormulationInput {
        FormulationInput { patient_id: patient_id.to_string(), episode_id: episode_id.to_string(), title: "Formulación inicial".to_string(), model_type: None, summary_text: Some("Contenido v1".to_string()) }
    }

    #[test]
    fn creates_a_formulation_with_its_first_version() {
        let conn = test_conn("create");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let episode_id = create_test_episode(&conn, &patient_id);
        let (formulation, version) = create_formulation(&conn, minimal_input(&patient_id, &episode_id)).unwrap();
        assert_eq!(formulation.episode_id.as_deref(), Some(episode_id.as_str()));
        assert_eq!(version.version_number, 1);
        assert_eq!(version.summary_text.as_deref(), Some("Contenido v1"));
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let conn = test_conn("patient-not-found");
        let err = create_formulation(&conn, minimal_input("no-existe", "no-existe")).unwrap_err();
        assert!(matches!(err, FormulationError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let conn = test_conn("patient-archived");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let episode_id = create_test_episode(&conn, &patient_id);
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = create_formulation(&conn, minimal_input(&patient_id, &episode_id)).unwrap_err();
        assert!(matches!(err, FormulationError::PatientArchived));
    }

    #[test]
    fn rejects_a_nonexistent_episode() {
        let conn = test_conn("episode-not-found");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let err = create_formulation(&conn, minimal_input(&patient_id, "no-existe")).unwrap_err();
        assert!(matches!(err, FormulationError::EpisodeNotFound));
    }

    #[test]
    fn rejects_an_episode_belonging_to_a_different_patient() {
        let conn = test_conn("episode-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let episode_of_b = create_test_episode(&conn, &patient_b);
        let err = create_formulation(&conn, minimal_input(&patient_a, &episode_of_b)).unwrap_err();
        assert!(matches!(err, FormulationError::EpisodePatientMismatch));
    }

    #[test]
    fn rejects_a_closed_episode() {
        let conn = test_conn("episode-closed");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let episode_id = create_test_episode(&conn, &patient_id);
        conn.execute("UPDATE treatment_episodes SET status = 'cerrado' WHERE id = ?1", [&episode_id]).unwrap();
        let err = create_formulation(&conn, minimal_input(&patient_id, &episode_id)).unwrap_err();
        assert!(matches!(err, FormulationError::EpisodeNotAssignable));
    }

    #[test]
    fn rejects_a_second_formulation_for_the_same_episode() {
        let conn = test_conn("duplicate-formulation");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let episode_id = create_test_episode(&conn, &patient_id);
        create_formulation(&conn, minimal_input(&patient_id, &episode_id)).unwrap();
        let err = create_formulation(&conn, minimal_input(&patient_id, &episode_id)).unwrap_err();
        assert!(matches!(err, FormulationError::FormulationAlreadyExistsForEpisode));
    }

    #[test]
    fn rejects_empty_title() {
        let conn = test_conn("empty-title");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let episode_id = create_test_episode(&conn, &patient_id);
        let input = FormulationInput { title: "   ".to_string(), ..minimal_input(&patient_id, &episode_id) };
        let err = create_formulation(&conn, input).unwrap_err();
        assert!(matches!(err, FormulationError::Validation(FormulationValidationError::TitleRequired)));
    }

    #[test]
    fn update_creates_a_new_version_and_never_touches_the_previous_one() {
        let conn = test_conn("update-new-version");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let episode_id = create_test_episode(&conn, &patient_id);
        let (formulation, v1) = create_formulation(&conn, minimal_input(&patient_id, &episode_id)).unwrap();

        let (_f, v2) = create_new_version(&conn, &formulation.id, NewVersionInput { summary_text: Some("Contenido v2".to_string()) }).unwrap();
        assert_eq!(v2.version_number, 2);
        assert_eq!(v2.summary_text.as_deref(), Some("Contenido v2"));

        let history = list_versions(&conn, &formulation.id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].id, v2.id, "la más reciente va primero");
        assert_eq!(history[1].id, v1.id);
        assert_eq!(history[1].summary_text.as_deref(), Some("Contenido v1"), "v1 permanece intacta, nunca se sobrescribe");
    }

    #[test]
    fn rejects_a_new_version_for_an_archived_patient() {
        let conn = test_conn("update-archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let episode_id = create_test_episode(&conn, &patient_id);
        let (formulation, _v1) = create_formulation(&conn, minimal_input(&patient_id, &episode_id)).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();

        let err = create_new_version(&conn, &formulation.id, NewVersionInput { summary_text: Some("Intento".to_string()) }).unwrap_err();
        assert!(matches!(err, FormulationError::PatientArchived));
    }

    #[test]
    fn allows_a_new_version_again_after_the_patient_is_restored() {
        let conn = test_conn("update-restored-patient");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let episode_id = create_test_episode(&conn, &patient_id);
        let (formulation, _v1) = create_formulation(&conn, minimal_input(&patient_id, &episode_id)).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        patients_service::restore_patient(&conn, &patient_id).unwrap();

        let (_f, v2) = create_new_version(&conn, &formulation.id, NewVersionInput { summary_text: Some("Contenido v2".to_string()) }).unwrap();
        assert_eq!(v2.version_number, 2);
    }

    #[test]
    fn rejects_a_new_version_when_the_episode_is_closed() {
        let conn = test_conn("update-closed-episode");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let episode_id = create_test_episode(&conn, &patient_id);
        let (formulation, _v1) = create_formulation(&conn, minimal_input(&patient_id, &episode_id)).unwrap();
        conn.execute("UPDATE treatment_episodes SET status = 'cerrado' WHERE id = ?1", [&episode_id]).unwrap();

        let err = create_new_version(&conn, &formulation.id, NewVersionInput { summary_text: Some("Intento".to_string()) }).unwrap_err();
        assert!(matches!(err, FormulationError::EpisodeNotAssignable));
    }

    #[test]
    fn reingreso_never_copies_the_previous_episodes_formulation() {
        let conn = test_conn("reingreso-no-copy");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let episode_a = create_test_episode(&conn, &patient_id);
        create_formulation(&conn, minimal_input(&patient_id, &episode_a)).unwrap();
        conn.execute("UPDATE treatment_episodes SET status = 'cerrado' WHERE id = ?1", [&episode_a]).unwrap();

        let episode_b = episodes_service::create_episode(&conn, episodes_service::TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        assert!(
            get_formulation_by_episode(&conn, &episode_b.id).unwrap().is_none(),
            "el proceso nuevo nunca hereda automáticamente la formulación del proceso anterior"
        );
    }

    #[test]
    fn list_formulations_includes_formulations_from_previous_episodes() {
        let conn = test_conn("list-across-episodes");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let episode_a = create_test_episode(&conn, &patient_id);
        create_formulation(&conn, minimal_input(&patient_id, &episode_a)).unwrap();
        conn.execute("UPDATE treatment_episodes SET status = 'cerrado' WHERE id = ?1", [&episode_a]).unwrap();

        let episode_b = episodes_service::create_episode(&conn, episodes_service::TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        create_formulation(&conn, minimal_input(&patient_id, &episode_b.id)).unwrap();

        let summaries = list_formulations(&conn, &patient_id).unwrap();
        assert_eq!(summaries.len(), 2, "el historial de formulaciones incluye las de procesos anteriores");
    }
}
