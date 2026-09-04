//! Acceso a datos de `episode_clinical_profile`. SQL puro — sin reglas de
//! negocio (eso vive en `services::episode_clinical_profile`) y sin
//! ninguna noción de si el vault está desbloqueado.
//!
//! Mismo patrón que `repositories::patient_clinical_profile` (Fase 6):
//! registro **mutable sin versionado**, un único registro por proceso
//! (`episode_id` es la propia `PRIMARY KEY`, definida en `SCHEMA_V4`). A
//! diferencia de `patient_clinical_profile`, esta tabla solo lleva los tres
//! campos que la auditoría post Fase 8 clasificó como específicos de
//! proceso — ver `docs/treatment-episodes.md`.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeClinicalProfile {
    pub episode_id: String,
    pub presenting_problem: Option<String>,
    pub primary_diagnosis_code: Option<String>,
    pub diagnosis_notes: Option<String>,
    pub updated_at: String,
}

pub struct NewEpisodeClinicalProfileRow<'a> {
    pub episode_id: &'a str,
    pub presenting_problem: Option<&'a str>,
    pub primary_diagnosis_code: Option<&'a str>,
    pub diagnosis_notes: Option<&'a str>,
}

pub struct EpisodeClinicalProfileUpdateRow<'a> {
    pub presenting_problem: Option<&'a str>,
    pub primary_diagnosis_code: Option<&'a str>,
    pub diagnosis_notes: Option<&'a str>,
}

const PROFILE_COLUMNS: &str = "episode_id, presenting_problem, primary_diagnosis_code, diagnosis_notes, updated_at";

fn map_row(row: &Row) -> rusqlite::Result<EpisodeClinicalProfile> {
    Ok(EpisodeClinicalProfile { episode_id: row.get(0)?, presenting_problem: row.get(1)?, primary_diagnosis_code: row.get(2)?, diagnosis_notes: row.get(3)?, updated_at: row.get(4)? })
}

pub fn find_by_episode_id(conn: &Connection, episode_id: &str) -> rusqlite::Result<Option<EpisodeClinicalProfile>> {
    conn.query_row(&format!("SELECT {PROFILE_COLUMNS} FROM episode_clinical_profile WHERE episode_id = ?1"), params![episode_id], map_row).optional()
}

/// Falla con `SQLITE_CONSTRAINT` si ya existe un perfil para ese proceso —
/// la capa de servicio decide qué hacer con ese caso.
pub fn insert(conn: &Connection, row: &NewEpisodeClinicalProfileRow) -> rusqlite::Result<EpisodeClinicalProfile> {
    conn.execute(
        "INSERT INTO episode_clinical_profile (episode_id, presenting_problem, primary_diagnosis_code, diagnosis_notes) VALUES (?1, ?2, ?3, ?4)",
        params![row.episode_id, row.presenting_problem, row.primary_diagnosis_code, row.diagnosis_notes],
    )?;
    find_by_episode_id(conn, row.episode_id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn update(conn: &Connection, episode_id: &str, row: &EpisodeClinicalProfileUpdateRow) -> rusqlite::Result<Option<EpisodeClinicalProfile>> {
    let affected = conn.execute(
        "UPDATE episode_clinical_profile SET presenting_problem = ?1, primary_diagnosis_code = ?2, diagnosis_notes = ?3 WHERE episode_id = ?4",
        params![row.presenting_problem, row.primary_diagnosis_code, row.diagnosis_notes, episode_id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_episode_id(conn, episode_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::treatment_episodes::{self, NewTreatmentEpisodeRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-episode-profile-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x44u8; VAULT_KEY_LEN]);
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
    fn find_by_episode_id_returns_none_when_no_profile_exists() {
        let conn = test_conn("find-none");
        let episode_id = create_test_episode(&conn, "Paciente Uno");
        assert!(find_by_episode_id(&conn, &episode_id).unwrap().is_none());
    }

    #[test]
    fn inserts_and_finds_a_profile() {
        let conn = test_conn("insert-find");
        let episode_id = create_test_episode(&conn, "Paciente Dos");
        let profile = insert(&conn, &NewEpisodeClinicalProfileRow { episode_id: &episode_id, presenting_problem: Some("Duelo"), primary_diagnosis_code: Some("F43.2"), diagnosis_notes: Some("Notas") }).unwrap();
        assert_eq!(profile.presenting_problem.as_deref(), Some("Duelo"));
        let found = find_by_episode_id(&conn, &episode_id).unwrap().unwrap();
        assert_eq!(found.primary_diagnosis_code.as_deref(), Some("F43.2"));
    }

    #[test]
    fn a_second_insert_for_the_same_episode_violates_the_primary_key() {
        let conn = test_conn("duplicate-insert");
        let episode_id = create_test_episode(&conn, "Paciente Tres");
        insert(&conn, &NewEpisodeClinicalProfileRow { episode_id: &episode_id, presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None }).unwrap();
        let err = insert(&conn, &NewEpisodeClinicalProfileRow { episode_id: &episode_id, presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None }).unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    #[test]
    fn update_replaces_all_fields() {
        let conn = test_conn("update-replaces");
        let episode_id = create_test_episode(&conn, "Paciente Cuatro");
        insert(&conn, &NewEpisodeClinicalProfileRow { episode_id: &episode_id, presenting_problem: Some("Original"), primary_diagnosis_code: None, diagnosis_notes: None }).unwrap();
        let updated = update(&conn, &episode_id, &EpisodeClinicalProfileUpdateRow { presenting_problem: Some("Editado"), primary_diagnosis_code: Some("F32.0"), diagnosis_notes: Some("Notas editadas") }).unwrap().unwrap();
        assert_eq!(updated.presenting_problem.as_deref(), Some("Editado"));
        assert_eq!(updated.primary_diagnosis_code.as_deref(), Some("F32.0"));
    }

    #[test]
    fn update_on_a_nonexistent_profile_returns_none() {
        let conn = test_conn("update-nonexistent");
        let episode_id = create_test_episode(&conn, "Paciente Cinco");
        let result = update(&conn, &episode_id, &EpisodeClinicalProfileUpdateRow { presenting_problem: Some("X"), primary_diagnosis_code: None, diagnosis_notes: None }).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn update_touches_updated_at() {
        let conn = test_conn("update-touches-updated-at");
        let episode_id = create_test_episode(&conn, "Paciente Seis");
        let created = insert(&conn, &NewEpisodeClinicalProfileRow { episode_id: &episode_id, presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None }).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let updated = update(&conn, &episode_id, &EpisodeClinicalProfileUpdateRow { presenting_problem: Some("Cambio"), primary_diagnosis_code: None, diagnosis_notes: None }).unwrap().unwrap();
        assert!(updated.updated_at >= created.updated_at);
    }
}
