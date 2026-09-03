//! Reglas de negocio de los antecedentes clínicos de un paciente. Ver
//! `docs/clinical-profile.md` para el diseño completo.
//!
//! A diferencia de `session_notes` (Fase 4), este es un registro **mutable
//! sin versionado** — decisión de producto explícita y deliberada (ver
//! aprobación de Fase 6). No hay historial, no hay snapshots: `UPDATE`
//! reemplaza el contenido actual.
//!
//! `risk_flags` se trata únicamente como una representación JSON almacenada
//! en `TEXT` (la propia columna de `SCHEMA_V1`, sin cambios). Esta capa no
//! interpreta su contenido, no impone una taxonomía clínica, y no calcula
//! nada a partir de él — solo valida que, si se completa, sea JSON
//! sintácticamente válido.
//!
//! Esta capa nunca sabe nada de Tauri, del estado de bloqueo del vault, ni
//! toca Google Calendar en ningún punto.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::patient_clinical_profile::{self, ClinicalProfile, ClinicalProfileUpdateRow, NewClinicalProfileRow};
use crate::repositories::patients;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClinicalProfileInput {
    pub presenting_problem: Option<String>,
    pub primary_diagnosis_code: Option<String>,
    pub diagnosis_notes: Option<String>,
    pub risk_flags: Option<String>,
    pub relevant_medical_notes: Option<String>,
}

#[derive(Debug)]
pub enum ClinicalProfileValidationError {
    RiskFlagsInvalidJson,
}

impl fmt::Display for ClinicalProfileValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClinicalProfileValidationError::RiskFlagsInvalidJson => write!(f, "el contenido de factores de riesgo no es JSON válido"),
        }
    }
}
impl std::error::Error for ClinicalProfileValidationError {}

#[derive(Debug)]
pub enum ClinicalProfileError {
    Validation(ClinicalProfileValidationError),
    PatientNotFound,
    PatientArchived,
    AlreadyExists,
    NotFound,
    Database(rusqlite::Error),
}

impl fmt::Display for ClinicalProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClinicalProfileError::Validation(e) => write!(f, "{e}"),
            ClinicalProfileError::PatientNotFound => write!(f, "paciente no encontrado"),
            ClinicalProfileError::PatientArchived => write!(f, "no se pueden crear antecedentes nuevos para un paciente archivado"),
            ClinicalProfileError::AlreadyExists => write!(f, "este paciente ya tiene antecedentes clínicos registrados"),
            ClinicalProfileError::NotFound => write!(f, "este paciente todavía no tiene antecedentes clínicos registrados"),
            ClinicalProfileError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for ClinicalProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ClinicalProfileError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for ClinicalProfileError {
    fn from(e: rusqlite::Error) -> Self {
        ClinicalProfileError::Database(e)
    }
}
impl From<ClinicalProfileValidationError> for ClinicalProfileError {
    fn from(e: ClinicalProfileValidationError) -> Self {
        ClinicalProfileError::Validation(e)
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// Solo valida forma JSON (sintaxis), nunca contenido — no hay ninguna
/// noción de qué forma "debería" tener (objeto, lista, etc.), a propósito:
/// esta fase no define ninguna taxonomía clínica de riesgo.
fn validate_risk_flags(value: &str) -> Result<(), ClinicalProfileValidationError> {
    serde_json::from_str::<serde_json::Value>(value).map(|_| ()).map_err(|_| ClinicalProfileValidationError::RiskFlagsInvalidJson)
}

struct ValidatedFields {
    presenting_problem: Option<String>,
    primary_diagnosis_code: Option<String>,
    diagnosis_notes: Option<String>,
    risk_flags: Option<String>,
    relevant_medical_notes: Option<String>,
}

fn validate_common(input: ClinicalProfileInput) -> Result<ValidatedFields, ClinicalProfileValidationError> {
    let risk_flags = none_if_blank(input.risk_flags);
    if let Some(ref r) = risk_flags {
        validate_risk_flags(r)?;
    }
    Ok(ValidatedFields {
        presenting_problem: none_if_blank(input.presenting_problem),
        primary_diagnosis_code: none_if_blank(input.primary_diagnosis_code),
        diagnosis_notes: none_if_blank(input.diagnosis_notes),
        risk_flags,
        relevant_medical_notes: none_if_blank(input.relevant_medical_notes),
    })
}

fn require_existing_patient(conn: &Connection, patient_id: &str) -> Result<patients::Patient, ClinicalProfileError> {
    patients::find_by_id(conn, patient_id)?.ok_or(ClinicalProfileError::PatientNotFound)
}

/// `None` significa "el paciente existe pero todavía no tiene antecedentes
/// registrados" — no es un error, es el estado inicial esperado de la
/// mayoría de los pacientes.
pub fn get_clinical_profile(conn: &Connection, patient_id: &str) -> Result<Option<ClinicalProfile>, ClinicalProfileError> {
    require_existing_patient(conn, patient_id)?;
    Ok(patient_clinical_profile::find_by_patient_id(conn, patient_id)?)
}

/// Rechaza la creación para un paciente inexistente o archivado (mismo
/// criterio que `services::goals::create_goal`), y para un paciente que ya
/// tiene antecedentes registrados — un único registro por paciente, la
/// propia `PRIMARY KEY` de la tabla ya lo garantiza a nivel de esquema,
/// esta capa lo verifica antes para devolver un error de dominio claro en
/// vez de un error crudo de `SQLite`.
pub fn create_clinical_profile(conn: &Connection, patient_id: &str, input: ClinicalProfileInput) -> Result<ClinicalProfile, ClinicalProfileError> {
    let patient = require_existing_patient(conn, patient_id)?;
    if patient.deleted_at.is_some() {
        return Err(ClinicalProfileError::PatientArchived);
    }
    if patient_clinical_profile::find_by_patient_id(conn, patient_id)?.is_some() {
        return Err(ClinicalProfileError::AlreadyExists);
    }

    let f = validate_common(input)?;
    Ok(patient_clinical_profile::insert(
        conn,
        &NewClinicalProfileRow {
            patient_id,
            presenting_problem: f.presenting_problem.as_deref(),
            primary_diagnosis_code: f.primary_diagnosis_code.as_deref(),
            diagnosis_notes: f.diagnosis_notes.as_deref(),
            risk_flags: f.risk_flags.as_deref(),
            relevant_medical_notes: f.relevant_medical_notes.as_deref(),
        },
    )?)
}

/// Reemplaza el contenido actual del perfil. A diferencia de la creación,
/// no revisa si el paciente está archivado — mismo criterio que editar
/// indicadores u objetivos ya existentes (archivar no bloquea la edición de
/// datos ya registrados, solo la creación de datos nuevos).
pub fn update_clinical_profile(conn: &Connection, patient_id: &str, input: ClinicalProfileInput) -> Result<ClinicalProfile, ClinicalProfileError> {
    require_existing_patient(conn, patient_id)?;
    let f = validate_common(input)?;
    let row = ClinicalProfileUpdateRow {
        presenting_problem: f.presenting_problem.as_deref(),
        primary_diagnosis_code: f.primary_diagnosis_code.as_deref(),
        diagnosis_notes: f.diagnosis_notes.as_deref(),
        risk_flags: f.risk_flags.as_deref(),
        relevant_medical_notes: f.relevant_medical_notes.as_deref(),
    };
    patient_clinical_profile::update(conn, patient_id, &row)?.ok_or(ClinicalProfileError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::services::patients::{self as patients_service, PatientInput};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-clinical-profile-service-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x32u8; VAULT_KEY_LEN]);
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
        patients_service::create_patient(conn, input).unwrap().id
    }

    fn empty_input() -> ClinicalProfileInput {
        ClinicalProfileInput { presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None }
    }

    fn full_input() -> ClinicalProfileInput {
        ClinicalProfileInput {
            presenting_problem: Some("Ansiedad generalizada".to_string()),
            primary_diagnosis_code: Some("F41.1".to_string()),
            diagnosis_notes: Some("Notas diagnósticas".to_string()),
            risk_flags: Some(r#"["insomnio","irritabilidad"]"#.to_string()),
            relevant_medical_notes: Some("Sin antecedentes médicos relevantes".to_string()),
        }
    }

    // ---- obtener ----

    #[test]
    fn getting_profile_of_a_patient_without_one_returns_none() {
        let conn = test_conn("get-none");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        assert!(get_clinical_profile(&conn, &patient_id).unwrap().is_none());
    }

    #[test]
    fn getting_profile_of_a_nonexistent_patient_is_rejected() {
        let conn = test_conn("get-nonexistent-patient");
        let err = get_clinical_profile(&conn, "no-existe").unwrap_err();
        assert!(matches!(err, ClinicalProfileError::PatientNotFound));
    }

    // ---- crear ----

    #[test]
    fn creates_a_profile_with_all_fields() {
        let conn = test_conn("create-full");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let profile = create_clinical_profile(&conn, &patient_id, full_input()).unwrap();
        assert_eq!(profile.patient_id, patient_id);
        assert_eq!(profile.presenting_problem.as_deref(), Some("Ansiedad generalizada"));
        assert_eq!(profile.risk_flags.as_deref(), Some(r#"["insomnio","irritabilidad"]"#));
    }

    #[test]
    fn creates_a_profile_with_all_fields_empty() {
        let conn = test_conn("create-empty");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let profile = create_clinical_profile(&conn, &patient_id, empty_input()).unwrap();
        assert!(profile.presenting_problem.is_none());
        assert!(profile.risk_flags.is_none());
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let conn = test_conn("create-nonexistent-patient");
        let err = create_clinical_profile(&conn, "no-existe", empty_input()).unwrap_err();
        assert!(matches!(err, ClinicalProfileError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let conn = test_conn("create-archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Archivado");
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = create_clinical_profile(&conn, &patient_id, empty_input()).unwrap_err();
        assert!(matches!(err, ClinicalProfileError::PatientArchived));
    }

    #[test]
    fn rejects_creating_a_second_profile_for_the_same_patient() {
        let conn = test_conn("create-duplicate");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        create_clinical_profile(&conn, &patient_id, empty_input()).unwrap();
        let err = create_clinical_profile(&conn, &patient_id, empty_input()).unwrap_err();
        assert!(matches!(err, ClinicalProfileError::AlreadyExists));
    }

    #[test]
    fn rejects_invalid_json_in_risk_flags_on_create() {
        let conn = test_conn("create-invalid-json");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let mut input = empty_input();
        input.risk_flags = Some("{no es json".to_string());
        let err = create_clinical_profile(&conn, &patient_id, input).unwrap_err();
        assert!(matches!(err, ClinicalProfileError::Validation(ClinicalProfileValidationError::RiskFlagsInvalidJson)));
    }

    #[test]
    fn accepts_blank_risk_flags_as_absent() {
        let conn = test_conn("create-blank-risk-flags");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let mut input = empty_input();
        input.risk_flags = Some("   ".to_string());
        let profile = create_clinical_profile(&conn, &patient_id, input).unwrap();
        assert!(profile.risk_flags.is_none());
    }

    // ---- actualizar ----

    #[test]
    fn updates_an_existing_profile_replacing_its_fields() {
        let conn = test_conn("update-existing");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        create_clinical_profile(&conn, &patient_id, empty_input()).unwrap();

        let updated = update_clinical_profile(&conn, &patient_id, full_input()).unwrap();
        assert_eq!(updated.presenting_problem.as_deref(), Some("Ansiedad generalizada"));
        assert_eq!(updated.risk_flags.as_deref(), Some(r#"["insomnio","irritabilidad"]"#));
    }

    #[test]
    fn updating_a_profile_that_does_not_exist_yet_is_rejected() {
        let conn = test_conn("update-nonexistent-profile");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let err = update_clinical_profile(&conn, &patient_id, full_input()).unwrap_err();
        assert!(matches!(err, ClinicalProfileError::NotFound));
    }

    #[test]
    fn updating_for_a_nonexistent_patient_is_rejected() {
        let conn = test_conn("update-nonexistent-patient");
        let err = update_clinical_profile(&conn, "no-existe", full_input()).unwrap_err();
        assert!(matches!(err, ClinicalProfileError::PatientNotFound));
    }

    #[test]
    fn updating_an_archived_patients_profile_is_allowed() {
        let conn = test_conn("update-archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        create_clinical_profile(&conn, &patient_id, empty_input()).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();

        let updated = update_clinical_profile(&conn, &patient_id, full_input()).unwrap();
        assert_eq!(updated.presenting_problem.as_deref(), Some("Ansiedad generalizada"));
    }

    #[test]
    fn rejects_invalid_json_in_risk_flags_on_update() {
        let conn = test_conn("update-invalid-json");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        create_clinical_profile(&conn, &patient_id, empty_input()).unwrap();
        let mut input = empty_input();
        input.risk_flags = Some("[not valid".to_string());
        let err = update_clinical_profile(&conn, &patient_id, input).unwrap_err();
        assert!(matches!(err, ClinicalProfileError::Validation(ClinicalProfileValidationError::RiskFlagsInvalidJson)));
    }

    // ---- aislamiento entre pacientes ----

    #[test]
    fn a_patients_profile_cannot_be_read_through_another_patients_id() {
        let conn = test_conn("isolation-read");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        create_clinical_profile(&conn, &patient_a, full_input()).unwrap();

        // El "perfil de B" simplemente no existe — no hay forma de que una
        // operación con patient_id = B devuelva o toque el registro de A.
        assert!(get_clinical_profile(&conn, &patient_b).unwrap().is_none());
    }

    #[test]
    fn updating_with_another_patients_id_never_touches_the_first_patients_profile() {
        let conn = test_conn("isolation-update");
        let patient_a = create_test_patient(&conn, "Paciente C");
        let patient_b = create_test_patient(&conn, "Paciente D");
        create_clinical_profile(&conn, &patient_a, full_input()).unwrap();

        // Intentar "editar" con el patient_id de B falla con NotFound (B no
        // tiene perfil) y el UPDATE, ligado a patient_id = B en la cláusula
        // WHERE, es estructuralmente incapaz de afectar la fila de A.
        let err = update_clinical_profile(&conn, &patient_b, empty_input()).unwrap_err();
        assert!(matches!(err, ClinicalProfileError::NotFound));

        let profile_a = get_clinical_profile(&conn, &patient_a).unwrap().unwrap();
        assert_eq!(profile_a.presenting_problem.as_deref(), Some("Ansiedad generalizada"));
    }
}
