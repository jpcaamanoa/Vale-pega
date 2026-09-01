//! Acceso a datos de `patient_clinical_profile`. SQL puro — sin reglas de
//! negocio (eso vive en `services::patient_clinical_profile`) y sin ninguna
//! noción de si el vault está desbloqueado.
//!
//! A diferencia de `session_notes` (Fase 4), este registro es **mutable sin
//! versionado**: un único registro por paciente (`patient_id` es la propia
//! `PRIMARY KEY` de la tabla, definida desde Fase 1.3), y `UPDATE` reemplaza
//! el contenido actual. No hay `version`, `is_current`, `is_locked` ni
//! historial — decisión de producto explícita de la aprobación de Fase 6.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClinicalProfile {
    pub patient_id: String,
    pub presenting_problem: Option<String>,
    pub primary_diagnosis_code: Option<String>,
    pub diagnosis_notes: Option<String>,
    pub risk_flags: Option<String>,
    pub relevant_medical_notes: Option<String>,
    pub updated_at: String,
}

pub struct NewClinicalProfileRow<'a> {
    pub patient_id: &'a str,
    pub presenting_problem: Option<&'a str>,
    pub primary_diagnosis_code: Option<&'a str>,
    pub diagnosis_notes: Option<&'a str>,
    pub risk_flags: Option<&'a str>,
    pub relevant_medical_notes: Option<&'a str>,
}

/// Mismos campos que `NewClinicalProfileRow` — no hay diferencia entre lo
/// que se puede fijar al crear y lo que se puede cambiar al editar, a
/// propósito: es un registro mutable simple, no un flujo con campos que se
/// "cierran" con el tiempo.
pub struct ClinicalProfileUpdateRow<'a> {
    pub presenting_problem: Option<&'a str>,
    pub primary_diagnosis_code: Option<&'a str>,
    pub diagnosis_notes: Option<&'a str>,
    pub risk_flags: Option<&'a str>,
    pub relevant_medical_notes: Option<&'a str>,
}

const PROFILE_COLUMNS: &str =
    "patient_id, presenting_problem, primary_diagnosis_code, diagnosis_notes, risk_flags, relevant_medical_notes, updated_at";

fn map_row(row: &Row) -> rusqlite::Result<ClinicalProfile> {
    Ok(ClinicalProfile {
        patient_id: row.get(0)?,
        presenting_problem: row.get(1)?,
        primary_diagnosis_code: row.get(2)?,
        diagnosis_notes: row.get(3)?,
        risk_flags: row.get(4)?,
        relevant_medical_notes: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn find_by_patient_id(conn: &Connection, patient_id: &str) -> rusqlite::Result<Option<ClinicalProfile>> {
    conn.query_row(
        &format!("SELECT {PROFILE_COLUMNS} FROM patient_clinical_profile WHERE patient_id = ?1"),
        params![patient_id],
        map_row,
    )
    .optional()
}

/// Falla con `SQLITE_CONSTRAINT` (violación de `PRIMARY KEY`) si ya existe
/// un perfil para ese paciente — la capa de servicio decide qué hacer con
/// ese caso, este módulo no lo interpreta.
pub fn insert(conn: &Connection, row: &NewClinicalProfileRow) -> rusqlite::Result<ClinicalProfile> {
    conn.execute(
        "INSERT INTO patient_clinical_profile \
         (patient_id, presenting_problem, primary_diagnosis_code, diagnosis_notes, risk_flags, relevant_medical_notes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            row.patient_id,
            row.presenting_problem,
            row.primary_diagnosis_code,
            row.diagnosis_notes,
            row.risk_flags,
            row.relevant_medical_notes
        ],
    )?;
    find_by_patient_id(conn, row.patient_id).map(|opt| opt.expect("se acaba de insertar"))
}

/// `UPDATE` directo — reemplaza el contenido actual. Devuelve `None` si no
/// existe perfil para ese paciente (nunca crea uno implícitamente).
pub fn update(conn: &Connection, patient_id: &str, row: &ClinicalProfileUpdateRow) -> rusqlite::Result<Option<ClinicalProfile>> {
    let affected = conn.execute(
        "UPDATE patient_clinical_profile SET \
         presenting_problem = ?1, primary_diagnosis_code = ?2, diagnosis_notes = ?3, risk_flags = ?4, relevant_medical_notes = ?5 \
         WHERE patient_id = ?6",
        params![row.presenting_problem, row.primary_diagnosis_code, row.diagnosis_notes, row.risk_flags, row.relevant_medical_notes, patient_id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_patient_id(conn, patient_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-clinical-profile-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x31u8; VAULT_KEY_LEN]);
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
            },
        )
        .unwrap();
        id
    }

    #[test]
    fn find_by_patient_id_returns_none_when_no_profile_exists() {
        let conn = test_conn("find-none");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        assert!(find_by_patient_id(&conn, &patient_id).unwrap().is_none());
    }

    #[test]
    fn inserts_and_finds_a_profile() {
        let conn = test_conn("insert-find");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let profile = insert(
            &conn,
            &NewClinicalProfileRow {
                patient_id: &patient_id,
                presenting_problem: Some("Ansiedad generalizada"),
                primary_diagnosis_code: Some("F41.1"),
                diagnosis_notes: Some("Notas"),
                risk_flags: Some(r#"["insomnio"]"#),
                relevant_medical_notes: Some("Sin antecedentes relevantes"),
            },
        )
        .unwrap();
        assert_eq!(profile.patient_id, patient_id);
        assert_eq!(profile.presenting_problem.as_deref(), Some("Ansiedad generalizada"));

        let found = find_by_patient_id(&conn, &patient_id).unwrap().unwrap();
        assert_eq!(found.primary_diagnosis_code.as_deref(), Some("F41.1"));
    }

    #[test]
    fn inserts_a_profile_with_all_fields_empty() {
        let conn = test_conn("insert-empty");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let profile = insert(
            &conn,
            &NewClinicalProfileRow { patient_id: &patient_id, presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None },
        )
        .unwrap();
        assert!(profile.presenting_problem.is_none());
        assert!(profile.risk_flags.is_none());
    }

    #[test]
    fn a_second_insert_for_the_same_patient_violates_the_primary_key() {
        let conn = test_conn("duplicate-insert");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        insert(&conn, &NewClinicalProfileRow { patient_id: &patient_id, presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None }).unwrap();

        let err = insert(&conn, &NewClinicalProfileRow { patient_id: &patient_id, presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None }).unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    #[test]
    fn update_replaces_all_fields() {
        let conn = test_conn("update-replaces");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        insert(&conn, &NewClinicalProfileRow { patient_id: &patient_id, presenting_problem: Some("Original"), primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None }).unwrap();

        let updated = update(
            &conn,
            &patient_id,
            &ClinicalProfileUpdateRow {
                presenting_problem: Some("Editado"),
                primary_diagnosis_code: Some("F32.0"),
                diagnosis_notes: Some("Notas editadas"),
                risk_flags: Some(r#"["riesgo editado"]"#),
                relevant_medical_notes: Some("Notas médicas editadas"),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.presenting_problem.as_deref(), Some("Editado"));
        assert_eq!(updated.primary_diagnosis_code.as_deref(), Some("F32.0"));
        assert_eq!(updated.risk_flags.as_deref(), Some(r#"["riesgo editado"]"#));
    }

    #[test]
    fn update_can_clear_a_previously_set_field() {
        let conn = test_conn("update-clears");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        insert(&conn, &NewClinicalProfileRow { patient_id: &patient_id, presenting_problem: Some("Original"), primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None }).unwrap();

        let updated = update(&conn, &patient_id, &ClinicalProfileUpdateRow { presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None })
            .unwrap()
            .unwrap();
        assert!(updated.presenting_problem.is_none());
    }

    #[test]
    fn update_on_a_nonexistent_profile_returns_none() {
        let conn = test_conn("update-nonexistent");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let result = update(&conn, &patient_id, &ClinicalProfileUpdateRow { presenting_problem: Some("X"), primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None }).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn update_touches_updated_at() {
        let conn = test_conn("update-touches-updated-at");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let created = insert(&conn, &NewClinicalProfileRow { patient_id: &patient_id, presenting_problem: None, primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None }).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let updated = update(&conn, &patient_id, &ClinicalProfileUpdateRow { presenting_problem: Some("Cambio"), primary_diagnosis_code: None, diagnosis_notes: None, risk_flags: None, relevant_medical_notes: None })
            .unwrap()
            .unwrap();
        assert!(updated.updated_at >= created.updated_at);
    }
}
