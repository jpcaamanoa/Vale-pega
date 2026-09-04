//! Reglas de negocio de los antecedentes clínicos específicos de un
//! proceso terapéutico (Fase 9). Mismo patrón que
//! `services::patient_clinical_profile` (Fase 6): registro **mutable sin
//! versionado**, un único registro por proceso.
//!
//! `presenting_problem`/`primary_diagnosis_code`/`diagnosis_notes` — nunca
//! `risk_flags` ni `relevant_medical_notes`, que permanecen exclusivamente
//! en `patient_clinical_profile` (longitudinales, ver
//! `docs/treatment-episodes.md`).
//!
//! Esta capa nunca sabe nada de Tauri, del estado de bloqueo del vault, ni
//! toca Google Calendar en ningún punto.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::episode_clinical_profile::{self, EpisodeClinicalProfile, EpisodeClinicalProfileUpdateRow, NewEpisodeClinicalProfileRow};
use crate::repositories::treatment_episodes;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeClinicalProfileInput {
    pub presenting_problem: Option<String>,
    pub primary_diagnosis_code: Option<String>,
    pub diagnosis_notes: Option<String>,
}

#[derive(Debug)]
pub enum EpisodeClinicalProfileError {
    EpisodeNotFound,
    AlreadyExists,
    NotFound,
    Database(rusqlite::Error),
}

impl fmt::Display for EpisodeClinicalProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EpisodeClinicalProfileError::EpisodeNotFound => write!(f, "proceso terapéutico no encontrado"),
            EpisodeClinicalProfileError::AlreadyExists => write!(f, "este proceso ya tiene antecedentes específicos registrados"),
            EpisodeClinicalProfileError::NotFound => write!(f, "este proceso todavía no tiene antecedentes específicos registrados"),
            EpisodeClinicalProfileError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for EpisodeClinicalProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EpisodeClinicalProfileError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for EpisodeClinicalProfileError {
    fn from(e: rusqlite::Error) -> Self {
        EpisodeClinicalProfileError::Database(e)
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

fn require_existing_episode(conn: &Connection, episode_id: &str) -> Result<(), EpisodeClinicalProfileError> {
    treatment_episodes::find_by_id(conn, episode_id)?.ok_or(EpisodeClinicalProfileError::EpisodeNotFound)?;
    Ok(())
}

/// `None` significa "el proceso existe pero todavía no tiene antecedentes
/// específicos registrados" — no es un error.
pub fn get_episode_clinical_profile(conn: &Connection, episode_id: &str) -> Result<Option<EpisodeClinicalProfile>, EpisodeClinicalProfileError> {
    require_existing_episode(conn, episode_id)?;
    Ok(episode_clinical_profile::find_by_episode_id(conn, episode_id)?)
}

/// Rechaza la creación para un proceso inexistente o que ya tiene
/// antecedentes registrados. Deliberadamente **no** revisa si el proceso
/// está archivado o cerrado — completar el motivo/diagnóstico de un
/// proceso ya finalizado sigue siendo información histórica válida que se
/// puede terminar de registrar después.
pub fn create_episode_clinical_profile(conn: &Connection, episode_id: &str, input: EpisodeClinicalProfileInput) -> Result<EpisodeClinicalProfile, EpisodeClinicalProfileError> {
    require_existing_episode(conn, episode_id)?;
    if episode_clinical_profile::find_by_episode_id(conn, episode_id)?.is_some() {
        return Err(EpisodeClinicalProfileError::AlreadyExists);
    }
    Ok(episode_clinical_profile::insert(
        conn,
        &NewEpisodeClinicalProfileRow {
            episode_id,
            presenting_problem: none_if_blank(input.presenting_problem).as_deref(),
            primary_diagnosis_code: none_if_blank(input.primary_diagnosis_code).as_deref(),
            diagnosis_notes: none_if_blank(input.diagnosis_notes).as_deref(),
        },
    )?)
}

pub fn update_episode_clinical_profile(conn: &Connection, episode_id: &str, input: EpisodeClinicalProfileInput) -> Result<EpisodeClinicalProfile, EpisodeClinicalProfileError> {
    require_existing_episode(conn, episode_id)?;
    let presenting_problem = none_if_blank(input.presenting_problem);
    let primary_diagnosis_code = none_if_blank(input.primary_diagnosis_code);
    let diagnosis_notes = none_if_blank(input.diagnosis_notes);
    let row = EpisodeClinicalProfileUpdateRow {
        presenting_problem: presenting_problem.as_deref(),
        primary_diagnosis_code: primary_diagnosis_code.as_deref(),
        diagnosis_notes: diagnosis_notes.as_deref(),
    };
    episode_clinical_profile::update(conn, episode_id, &row)?.ok_or(EpisodeClinicalProfileError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::treatment_episodes::{self, NewTreatmentEpisodeRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-episode-profile-svc-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x45u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_episode(conn: &Connection, patient_name: &str) -> String {
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
        treatment_episodes::insert(conn, &NewTreatmentEpisodeRow { id: &episode_id, patient_id: &patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        episode_id
    }

    #[test]
    fn get_returns_none_when_no_profile_exists() {
        let conn = test_conn("get-none");
        let episode_id = create_test_episode(&conn, "Paciente Uno");
        assert!(get_episode_clinical_profile(&conn, &episode_id).unwrap().is_none());
    }

    #[test]
    fn get_rejects_a_nonexistent_episode() {
        let conn = test_conn("get-no-episode");
        let err = get_episode_clinical_profile(&conn, "no-existe").unwrap_err();
        assert!(matches!(err, EpisodeClinicalProfileError::EpisodeNotFound));
    }

    #[test]
    fn creates_a_profile() {
        let conn = test_conn("create");
        let episode_id = create_test_episode(&conn, "Paciente Dos");
        let profile = create_episode_clinical_profile(&conn, &episode_id, EpisodeClinicalProfileInput { presenting_problem: Some("Ansiedad".into()), primary_diagnosis_code: Some("F41.1".into()), diagnosis_notes: None }).unwrap();
        assert_eq!(profile.presenting_problem.as_deref(), Some("Ansiedad"));
    }

    #[test]
    fn creation_rejects_a_nonexistent_episode() {
        let conn = test_conn("create-no-episode");
        let err = create_episode_clinical_profile(&conn, "no-existe", EpisodeClinicalProfileInput { presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None }).unwrap_err();
        assert!(matches!(err, EpisodeClinicalProfileError::EpisodeNotFound));
    }

    #[test]
    fn creating_a_second_profile_for_the_same_episode_is_rejected() {
        let conn = test_conn("create-duplicate");
        let episode_id = create_test_episode(&conn, "Paciente Tres");
        create_episode_clinical_profile(&conn, &episode_id, EpisodeClinicalProfileInput { presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None }).unwrap();
        let err = create_episode_clinical_profile(&conn, &episode_id, EpisodeClinicalProfileInput { presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None }).unwrap_err();
        assert!(matches!(err, EpisodeClinicalProfileError::AlreadyExists));
    }

    #[test]
    fn blank_fields_are_stored_as_none() {
        let conn = test_conn("blank-fields");
        let episode_id = create_test_episode(&conn, "Paciente Cuatro");
        let profile = create_episode_clinical_profile(&conn, &episode_id, EpisodeClinicalProfileInput { presenting_problem: Some("   ".into()), primary_diagnosis_code: None, diagnosis_notes: None }).unwrap();
        assert!(profile.presenting_problem.is_none());
    }

    #[test]
    fn updates_a_profile() {
        let conn = test_conn("update");
        let episode_id = create_test_episode(&conn, "Paciente Cinco");
        create_episode_clinical_profile(&conn, &episode_id, EpisodeClinicalProfileInput { presenting_problem: Some("Original".into()), primary_diagnosis_code: None, diagnosis_notes: None }).unwrap();
        let updated = update_episode_clinical_profile(&conn, &episode_id, EpisodeClinicalProfileInput { presenting_problem: Some("Editado".into()), primary_diagnosis_code: Some("F32.0".into()), diagnosis_notes: None }).unwrap();
        assert_eq!(updated.presenting_problem.as_deref(), Some("Editado"));
    }

    #[test]
    fn update_on_a_profile_that_does_not_exist_yet_is_rejected() {
        let conn = test_conn("update-nonexistent");
        let episode_id = create_test_episode(&conn, "Paciente Seis");
        let err = update_episode_clinical_profile(&conn, &episode_id, EpisodeClinicalProfileInput { presenting_problem: Some("X".into()), primary_diagnosis_code: None, diagnosis_notes: None }).unwrap_err();
        assert!(matches!(err, EpisodeClinicalProfileError::NotFound));
    }
}
