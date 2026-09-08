//! Reglas de negocio de evaluaciones clínicas/psicométricas (Fase 13). Ver
//! `docs/assessments.md` para el diseño completo.
//!
//! Regla de copyright, no negociable: esta aplicación **nunca** almacena ni
//! reproduce el contenido real de un instrumento (ítems, preguntas,
//! opciones, tablas de normas propietarias) — únicamente metadatos del
//! catálogo (nombre/abreviatura/descripción/categoría, todo texto libre
//! escrito por la usuaria) y resultados de una administración concreta
//! (puntaje total, subescalas, interpretación redactada por la
//! profesional). `raw_responses` existe en la tabla desde `SCHEMA_V1` pero
//! este servicio **nunca** la lee ni la escribe — ver
//! `repositories::assessments` y `docs/assessments.md`.
//!
//! Ninguna función de este servicio calcula, normaliza (percentiles,
//! T-scores, Z-scores) ni interpreta automáticamente nada: `total_score`,
//! `subscale_scores` e `interpretation_text` son siempre lo que la
//! profesional escribió, nunca un cómputo derivado. `subscale_scores` se
//! valida únicamente como JSON sintácticamente correcto (mismo criterio que
//! `services::patient_clinical_profile::validate_risk_flags` con
//! `risk_flags`) — nunca se interpreta su estructura.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::assessments::{
    self, AdministrationUpdateRow, AssessmentAdministration, AssessmentAdministrationSummary, AssessmentInstrument, InstrumentUpdateRow, NewAdministrationRow, NewInstrumentRow,
};
use crate::repositories::patients;
use crate::services::treatment_episodes::{self, TreatmentEpisodeError};

pub const VALID_CONTEXTS: &[&str] = &["ingreso", "seguimiento", "alta"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentInput {
    pub name: String,
    pub abbreviation: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdministrationInput {
    pub patient_id: String,
    pub instrument_id: String,
    pub episode_id: Option<String>,
    pub administered_at: String,
    pub context: Option<String>,
    pub total_score: Option<f64>,
    pub subscale_scores: Option<String>,
    pub interpretation_text: Option<String>,
}

/// Igual que `AdministrationInput` salvo `patientId`/`instrumentId`, que
/// nunca se reasignan una vez creada la administración (mismo criterio que
/// `TherapyTaskUpdateInput` respecto a `patientId`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdministrationUpdateInput {
    pub episode_id: Option<String>,
    pub administered_at: String,
    pub context: Option<String>,
    pub total_score: Option<f64>,
    pub subscale_scores: Option<String>,
    pub interpretation_text: Option<String>,
}

#[derive(Debug)]
pub enum AssessmentValidationError {
    NameRequired,
    DuplicateInstrumentName(String),
    InvalidDate,
    InvalidContext(String),
    SubscaleScoresInvalidJson,
}

impl fmt::Display for AssessmentValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssessmentValidationError::NameRequired => write!(f, "el nombre del instrumento es obligatorio"),
            AssessmentValidationError::DuplicateInstrumentName(name) => write!(f, "ya existe un instrumento llamado '{name}' en el catálogo"),
            AssessmentValidationError::InvalidDate => write!(f, "fecha inválida (formato esperado: AAAA-MM-DD)"),
            AssessmentValidationError::InvalidContext(c) => write!(f, "contexto inválido: '{c}' (debe ser uno de: {})", VALID_CONTEXTS.join(", ")),
            AssessmentValidationError::SubscaleScoresInvalidJson => write!(f, "el contenido de subescalas no es JSON válido"),
        }
    }
}
impl std::error::Error for AssessmentValidationError {}

#[derive(Debug)]
pub enum AssessmentError {
    Validation(AssessmentValidationError),
    InstrumentNotFound,
    AdministrationNotFound,
    PatientNotFound,
    PatientArchived,
    EpisodeNotFound,
    EpisodeArchived,
    EpisodeNotAssignable,
    EpisodePatientMismatch,
    Database(rusqlite::Error),
}

impl fmt::Display for AssessmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssessmentError::Validation(e) => write!(f, "{e}"),
            AssessmentError::InstrumentNotFound => write!(f, "instrumento no encontrado"),
            AssessmentError::AdministrationNotFound => write!(f, "administración no encontrada"),
            AssessmentError::PatientNotFound => write!(f, "paciente no encontrado"),
            AssessmentError::PatientArchived => write!(f, "no se pueden registrar evaluaciones nuevas para un paciente archivado"),
            AssessmentError::EpisodeNotFound => write!(f, "proceso terapéutico no encontrado"),
            AssessmentError::EpisodeArchived => write!(f, "este proceso está archivado y no puede recibir evaluaciones nuevas"),
            AssessmentError::EpisodeNotAssignable => write!(f, "este proceso no puede recibir evaluaciones nuevas"),
            AssessmentError::EpisodePatientMismatch => write!(f, "el proceso indicado pertenece a otro paciente"),
            AssessmentError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for AssessmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AssessmentError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for AssessmentError {
    fn from(e: rusqlite::Error) -> Self {
        AssessmentError::Database(e)
    }
}
impl From<AssessmentValidationError> for AssessmentError {
    fn from(e: AssessmentValidationError) -> Self {
        AssessmentError::Validation(e)
    }
}
/// Traduce los errores de `treatment_episodes::check_episode_assignable` a
/// sus equivalentes de dominio de evaluaciones — mismo criterio que
/// `impl From<TreatmentEpisodeError> for SessionError`.
impl From<TreatmentEpisodeError> for AssessmentError {
    fn from(e: TreatmentEpisodeError) -> Self {
        match e {
            TreatmentEpisodeError::NotFound => AssessmentError::EpisodeNotFound,
            TreatmentEpisodeError::EpisodeArchived => AssessmentError::EpisodeArchived,
            TreatmentEpisodeError::EpisodeNotAssignable => AssessmentError::EpisodeNotAssignable,
            TreatmentEpisodeError::EpisodePatientMismatch => AssessmentError::EpisodePatientMismatch,
            TreatmentEpisodeError::Database(err) => AssessmentError::Database(err),
            // El resto de las variantes no las produce `check_episode_assignable`.
            _ => AssessmentError::EpisodeNotFound,
        }
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// Mismo formato y misma forma de validación (estructural, no calendárica)
/// que `services::therapy_tasks::validate_date_format`.
fn validate_date_format(value: &str) -> Result<(), AssessmentValidationError> {
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
        Err(AssessmentValidationError::InvalidDate)
    }
}

fn validate_context(value: Option<String>) -> Result<Option<String>, AssessmentValidationError> {
    let value = none_if_blank(value);
    if let Some(ref c) = value {
        if !VALID_CONTEXTS.contains(&c.as_str()) {
            return Err(AssessmentValidationError::InvalidContext(c.clone()));
        }
    }
    Ok(value)
}

/// Validación únicamente sintáctica — nunca se interpreta la estructura del
/// JSON. Mismo criterio que `services::patient_clinical_profile::validate_risk_flags`.
fn validate_subscale_scores(value: Option<String>) -> Result<Option<String>, AssessmentValidationError> {
    let value = none_if_blank(value);
    if let Some(ref s) = value {
        serde_json::from_str::<serde_json::Value>(s).map_err(|_| AssessmentValidationError::SubscaleScoresInvalidJson)?;
    }
    Ok(value)
}

fn validate_instrument_name(name: String) -> Result<String, AssessmentValidationError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AssessmentValidationError::NameRequired);
    }
    Ok(name)
}

/// Rechaza un nombre ya usado por otro instrumento del catálogo, con un
/// error de dominio claro — nunca deja pasar la violación cruda de `UNIQUE`.
/// `ignore_id` permite reutilizar esta comprobación en `update_instrument`
/// sin que un instrumento choque consigo mismo.
fn check_name_not_taken(conn: &Connection, name: &str, ignore_id: Option<&str>) -> Result<(), AssessmentError> {
    if let Some(existing) = assessments::find_instrument_by_name(conn, name)? {
        if Some(existing.id.as_str()) != ignore_id {
            return Err(AssessmentValidationError::DuplicateInstrumentName(name.to_string()).into());
        }
    }
    Ok(())
}

/// Crea un instrumento nuevo en el catálogo que la usuaria mantiene —
/// siempre `is_custom = true` (ver `repositories::assessments::insert_instrument`).
/// Nunca almacena contenido protegido del instrumento: solo estos cuatro
/// campos de metadatos, todo texto libre.
pub fn create_instrument(conn: &Connection, input: InstrumentInput) -> Result<AssessmentInstrument, AssessmentError> {
    let name = validate_instrument_name(input.name)?;
    check_name_not_taken(conn, &name, None)?;

    let id = uuid::Uuid::new_v4().to_string();
    let abbreviation = none_if_blank(input.abbreviation);
    let description = none_if_blank(input.description);
    let category = none_if_blank(input.category);
    Ok(assessments::insert_instrument(
        conn,
        &NewInstrumentRow { id: &id, name: &name, abbreviation: abbreviation.as_deref(), description: description.as_deref(), category: category.as_deref() },
    )?)
}

pub fn get_instrument(conn: &Connection, id: &str) -> Result<AssessmentInstrument, AssessmentError> {
    assessments::find_instrument_by_id(conn, id)?.ok_or(AssessmentError::InstrumentNotFound)
}

pub fn list_instruments(conn: &Connection) -> Result<Vec<AssessmentInstrument>, AssessmentError> {
    Ok(assessments::list_instruments(conn)?)
}

pub fn update_instrument(conn: &Connection, id: &str, input: InstrumentInput) -> Result<AssessmentInstrument, AssessmentError> {
    let name = validate_instrument_name(input.name)?;
    check_name_not_taken(conn, &name, Some(id))?;

    let abbreviation = none_if_blank(input.abbreviation);
    let description = none_if_blank(input.description);
    let category = none_if_blank(input.category);
    let row = InstrumentUpdateRow { name: &name, abbreviation: abbreviation.as_deref(), description: description.as_deref(), category: category.as_deref() };
    assessments::update_instrument(conn, id, &row)?.ok_or(AssessmentError::InstrumentNotFound)
}

/// Rechaza el registro para un paciente inexistente o archivado — mismo
/// criterio que `services::therapy_tasks::create_task`. Si `episode_id`
/// viene informado, delega en `treatment_episodes::check_episode_assignable`
/// (mismo vínculo opcional que ya usan sesiones/objetivos).
pub fn create_administration(conn: &Connection, input: AdministrationInput) -> Result<AssessmentAdministration, AssessmentError> {
    let patient = patients::find_by_id(conn, &input.patient_id)?.ok_or(AssessmentError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(AssessmentError::PatientArchived);
    }
    assessments::find_instrument_by_id(conn, &input.instrument_id)?.ok_or(AssessmentError::InstrumentNotFound)?;

    validate_date_format(&input.administered_at)?;
    let context = validate_context(input.context)?;
    let subscale_scores = validate_subscale_scores(input.subscale_scores)?;
    let interpretation_text = none_if_blank(input.interpretation_text);
    treatment_episodes::check_episode_assignable(conn, &input.episode_id, &input.patient_id)?;

    let id = uuid::Uuid::new_v4().to_string();
    Ok(assessments::insert_administration(
        conn,
        &NewAdministrationRow {
            id: &id,
            patient_id: &input.patient_id,
            instrument_id: &input.instrument_id,
            episode_id: input.episode_id.as_deref(),
            administered_at: &input.administered_at,
            context: context.as_deref(),
            total_score: input.total_score,
            subscale_scores: subscale_scores.as_deref(),
            interpretation_text: interpretation_text.as_deref(),
        },
    )?)
}

pub fn get_administration(conn: &Connection, id: &str) -> Result<AssessmentAdministration, AssessmentError> {
    assessments::find_administration_by_id(conn, id)?.ok_or(AssessmentError::AdministrationNotFound)
}

pub fn list_administrations(conn: &Connection, patient_id: &str) -> Result<Vec<AssessmentAdministrationSummary>, AssessmentError> {
    Ok(assessments::list_active_by_patient(conn, patient_id)?)
}

pub fn list_archived_administrations(conn: &Connection, patient_id: &str) -> Result<Vec<AssessmentAdministrationSummary>, AssessmentError> {
    Ok(assessments::list_archived_by_patient(conn, patient_id)?)
}

/// Evolución longitudinal de un mismo instrumento para un paciente — nunca
/// mezcla instrumentos distintos (ver `docs/assessments.md`).
pub fn list_administrations_for_instrument(conn: &Connection, patient_id: &str, instrument_id: &str) -> Result<Vec<AssessmentAdministrationSummary>, AssessmentError> {
    Ok(assessments::list_by_patient_and_instrument(conn, patient_id, instrument_id)?)
}

/// Edita una administración ya registrada. No vuelve a comprobar si el
/// paciente está archivado (corregir un registro histórico de un paciente
/// archivado sigue permitido, mismo criterio que
/// `services::therapy_tasks::update_task`), pero si `episode_id` cambia, sí
/// se revalida contra el paciente de la administración.
pub fn update_administration(conn: &Connection, id: &str, input: AdministrationUpdateInput) -> Result<AssessmentAdministration, AssessmentError> {
    let existing = assessments::find_administration_by_id(conn, id)?.ok_or(AssessmentError::AdministrationNotFound)?;
    validate_date_format(&input.administered_at)?;
    let context = validate_context(input.context)?;
    let subscale_scores = validate_subscale_scores(input.subscale_scores)?;
    let interpretation_text = none_if_blank(input.interpretation_text);
    treatment_episodes::check_episode_assignable(conn, &input.episode_id, &existing.patient_id)?;

    let row = AdministrationUpdateRow {
        episode_id: input.episode_id.as_deref(),
        administered_at: &input.administered_at,
        context: context.as_deref(),
        total_score: input.total_score,
        subscale_scores: subscale_scores.as_deref(),
        interpretation_text: interpretation_text.as_deref(),
    };
    assessments::update_administration(conn, id, &row)?.ok_or(AssessmentError::AdministrationNotFound)
}

/// Soft delete únicamente. No existe, en ningún punto de este servicio ni
/// del repositorio, una operación de borrado físico alcanzable desde un
/// comando normal.
pub fn archive_administration(conn: &Connection, id: &str) -> Result<(), AssessmentError> {
    if assessments::soft_delete_administration(conn, id)? {
        Ok(())
    } else {
        Err(AssessmentError::AdministrationNotFound)
    }
}

pub fn restore_administration(conn: &Connection, id: &str) -> Result<AssessmentAdministration, AssessmentError> {
    if assessments::restore_administration(conn, id)? {
        get_administration(conn, id)
    } else {
        Err(AssessmentError::AdministrationNotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self as patients_repo, NewPatientRow};
    use crate::repositories::treatment_episodes::{self as episodes_repo, NewTreatmentEpisodeRow};
    use crate::services::patients as patients_service;

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-assessments-svc-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x81u8; VAULT_KEY_LEN]);
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

    fn minimal_instrument_input(name: &str) -> InstrumentInput {
        InstrumentInput { name: name.to_string(), abbreviation: None, description: None, category: None }
    }

    fn minimal_administration_input(patient_id: &str, instrument_id: &str) -> AdministrationInput {
        AdministrationInput { patient_id: patient_id.to_string(), instrument_id: instrument_id.to_string(), episode_id: None, administered_at: "2026-01-10".to_string(), context: None, total_score: None, subscale_scores: None, interpretation_text: None }
    }

    // -- catálogo de instrumentos -----------------------------------

    #[test]
    fn creates_an_instrument_with_metadata() {
        let conn = test_conn("create-instrument");
        let i = create_instrument(&conn, InstrumentInput { name: "BDI-II".into(), abbreviation: Some("BDI-II".into()), description: Some("Inventario de depresión de Beck".into()), category: Some("Depresión".into()) }).unwrap();
        assert_eq!(i.name, "BDI-II");
        assert_eq!(i.category.as_deref(), Some("Depresión"));
        assert!(i.is_custom);
    }

    #[test]
    fn rejects_empty_instrument_name() {
        let conn = test_conn("empty-instrument-name");
        let err = create_instrument(&conn, minimal_instrument_input("   ")).unwrap_err();
        assert!(matches!(err, AssessmentError::Validation(AssessmentValidationError::NameRequired)));
    }

    #[test]
    fn rejects_a_duplicate_instrument_name() {
        let conn = test_conn("duplicate-instrument-name");
        create_instrument(&conn, minimal_instrument_input("PHQ-9")).unwrap();
        let err = create_instrument(&conn, minimal_instrument_input("PHQ-9")).unwrap_err();
        assert!(matches!(err, AssessmentError::Validation(AssessmentValidationError::DuplicateInstrumentName(_))));
    }

    #[test]
    fn update_instrument_can_reuse_its_own_name() {
        let conn = test_conn("update-instrument-same-name");
        let i = create_instrument(&conn, minimal_instrument_input("STAI")).unwrap();
        let updated = update_instrument(&conn, &i.id, InstrumentInput { name: "STAI".into(), abbreviation: Some("STAI".into()), description: None, category: None }).unwrap();
        assert_eq!(updated.abbreviation.as_deref(), Some("STAI"));
    }

    #[test]
    fn update_instrument_rejects_renaming_to_another_instruments_name() {
        let conn = test_conn("update-instrument-name-clash");
        create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let i2 = create_instrument(&conn, minimal_instrument_input("STAI")).unwrap();
        let err = update_instrument(&conn, &i2.id, minimal_instrument_input("BDI-II")).unwrap_err();
        assert!(matches!(err, AssessmentError::Validation(AssessmentValidationError::DuplicateInstrumentName(_))));
    }

    #[test]
    fn list_instruments_returns_the_full_catalog() {
        let conn = test_conn("list-instruments");
        create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        create_instrument(&conn, minimal_instrument_input("STAI")).unwrap();
        assert_eq!(list_instruments(&conn).unwrap().len(), 2);
    }

    // -- administraciones ---------------------------------------------

    #[test]
    fn creates_an_administration_with_score_and_interpretation() {
        let conn = test_conn("create-admin");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let input = AdministrationInput { total_score: Some(18.0), interpretation_text: Some("Depresión moderada".into()), ..minimal_administration_input(&patient_id, &instrument.id) };
        let a = create_administration(&conn, input).unwrap();
        assert_eq!(a.total_score, Some(18.0));
        assert_eq!(a.interpretation_text.as_deref(), Some("Depresión moderada"));
    }

    #[test]
    fn accepts_a_negative_total_score_for_standardized_scales() {
        let conn = test_conn("negative-score");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let instrument = create_instrument(&conn, minimal_instrument_input("Z-Scale")).unwrap();
        let input = AdministrationInput { total_score: Some(-1.5), ..minimal_administration_input(&patient_id, &instrument.id) };
        let a = create_administration(&conn, input).unwrap();
        assert_eq!(a.total_score, Some(-1.5));
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let conn = test_conn("patient-not-found");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let err = create_administration(&conn, minimal_administration_input("no-existe", &instrument.id)).unwrap_err();
        assert!(matches!(err, AssessmentError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let conn = test_conn("patient-archived");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = create_administration(&conn, minimal_administration_input(&patient_id, &instrument.id)).unwrap_err();
        assert!(matches!(err, AssessmentError::PatientArchived));
    }

    #[test]
    fn rejects_a_nonexistent_instrument() {
        let conn = test_conn("instrument-not-found");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let err = create_administration(&conn, minimal_administration_input(&patient_id, "no-existe")).unwrap_err();
        assert!(matches!(err, AssessmentError::InstrumentNotFound));
    }

    #[test]
    fn rejects_invalid_administered_at_format() {
        let conn = test_conn("invalid-date");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let input = AdministrationInput { administered_at: "no-es-fecha".into(), ..minimal_administration_input(&patient_id, &instrument.id) };
        let err = create_administration(&conn, input).unwrap_err();
        assert!(matches!(err, AssessmentError::Validation(AssessmentValidationError::InvalidDate)));
    }

    #[test]
    fn rejects_invalid_context() {
        let conn = test_conn("invalid-context");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let input = AdministrationInput { context: Some("inventado".into()), ..minimal_administration_input(&patient_id, &instrument.id) };
        let err = create_administration(&conn, input).unwrap_err();
        assert!(matches!(err, AssessmentError::Validation(AssessmentValidationError::InvalidContext(_))));
    }

    #[test]
    fn rejects_invalid_json_in_subscale_scores() {
        let conn = test_conn("invalid-subscale-json");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let input = AdministrationInput { subscale_scores: Some("{no es json".into()), ..minimal_administration_input(&patient_id, &instrument.id) };
        let err = create_administration(&conn, input).unwrap_err();
        assert!(matches!(err, AssessmentError::Validation(AssessmentValidationError::SubscaleScoresInvalidJson)));
    }

    #[test]
    fn accepts_valid_json_in_subscale_scores() {
        let conn = test_conn("valid-subscale-json");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let input = AdministrationInput { subscale_scores: Some(r#"{"cognitivo":10,"somatico":8}"#.into()), ..minimal_administration_input(&patient_id, &instrument.id) };
        let a = create_administration(&conn, input).unwrap();
        assert_eq!(a.subscale_scores.as_deref(), Some(r#"{"cognitivo":10,"somatico":8}"#));
    }

    #[test]
    fn links_to_an_active_episode_of_the_same_patient() {
        let conn = test_conn("link-episode");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let episode_id = create_test_episode(&conn, &patient_id);
        let input = AdministrationInput { episode_id: Some(episode_id.clone()), ..minimal_administration_input(&patient_id, &instrument.id) };
        let a = create_administration(&conn, input).unwrap();
        assert_eq!(a.episode_id.as_deref(), Some(episode_id.as_str()));
    }

    #[test]
    fn rejects_an_episode_belonging_to_a_different_patient() {
        let conn = test_conn("episode-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let episode_of_b = create_test_episode(&conn, &patient_b);
        let input = AdministrationInput { episode_id: Some(episode_of_b), ..minimal_administration_input(&patient_a, &instrument.id) };
        let err = create_administration(&conn, input).unwrap_err();
        assert!(matches!(err, AssessmentError::EpisodePatientMismatch));
    }

    #[test]
    fn update_administration_edits_score_interpretation_and_context() {
        let conn = test_conn("update-admin");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let a = create_administration(&conn, minimal_administration_input(&patient_id, &instrument.id)).unwrap();

        let updated = update_administration(&conn, &a.id, AdministrationUpdateInput { episode_id: None, administered_at: "2026-01-11".into(), context: Some("seguimiento".into()), total_score: Some(12.0), subscale_scores: None, interpretation_text: Some("Mejoría".into()) }).unwrap();
        assert_eq!(updated.administered_at, "2026-01-11");
        assert_eq!(updated.context.as_deref(), Some("seguimiento"));
        assert_eq!(updated.total_score, Some(12.0));
    }

    #[test]
    fn editing_a_historical_administration_of_an_archived_patient_is_allowed() {
        let conn = test_conn("edit-archived-patient-admin");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let a = create_administration(&conn, minimal_administration_input(&patient_id, &instrument.id)).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();

        let updated = update_administration(&conn, &a.id, AdministrationUpdateInput { episode_id: None, administered_at: "2026-01-12".into(), context: None, total_score: None, subscale_scores: None, interpretation_text: Some("Corregido tras archivar".into()) }).unwrap();
        assert_eq!(updated.interpretation_text.as_deref(), Some("Corregido tras archivar"));
    }

    #[test]
    fn archive_and_restore_round_trip() {
        let conn = test_conn("archive-restore");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let instrument = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let a = create_administration(&conn, minimal_administration_input(&patient_id, &instrument.id)).unwrap();

        archive_administration(&conn, &a.id).unwrap();
        assert_eq!(list_administrations(&conn, &patient_id).unwrap().len(), 0);
        assert_eq!(list_archived_administrations(&conn, &patient_id).unwrap().len(), 1);

        let restored = restore_administration(&conn, &a.id).unwrap();
        assert_eq!(restored.id, a.id);
        assert_eq!(list_administrations(&conn, &patient_id).unwrap().len(), 1);
    }

    #[test]
    fn evolution_for_instrument_excludes_other_instruments_and_orders_oldest_first() {
        let conn = test_conn("evolution");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        let bdi = create_instrument(&conn, minimal_instrument_input("BDI-II")).unwrap();
        let stai = create_instrument(&conn, minimal_instrument_input("STAI")).unwrap();
        create_administration(&conn, AdministrationInput { administered_at: "2026-03-01".into(), ..minimal_administration_input(&patient_id, &bdi.id) }).unwrap();
        create_administration(&conn, AdministrationInput { administered_at: "2026-01-01".into(), ..minimal_administration_input(&patient_id, &bdi.id) }).unwrap();
        create_administration(&conn, AdministrationInput { administered_at: "2026-02-01".into(), ..minimal_administration_input(&patient_id, &stai.id) }).unwrap();

        let evolution = list_administrations_for_instrument(&conn, &patient_id, &bdi.id).unwrap();
        assert_eq!(evolution.len(), 2);
        assert_eq!(evolution[0].administered_at, "2026-01-01");
        assert_eq!(evolution[1].administered_at, "2026-03-01");
    }
}
