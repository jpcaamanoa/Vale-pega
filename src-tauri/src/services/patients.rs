//! Reglas de negocio de pacientes: validación autoritativa (la de Zod en el
//! frontend es solo para UX) y orquestación del repositorio. No sabe nada
//! de Tauri ni de si el vault está desbloqueado — eso ya se resolvió antes
//! de llegar aquí (`security::session::VaultSession::with_connection`).

use std::fmt;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::repositories::patients::{self, NewPatientRow, Patient, PatientSummary, PatientUpdateRow};

use super::rut::{self, RutError};

pub const VALID_STATUSES: &[&str] = &["activo", "inactivo", "alta", "archivado"];
const DEFAULT_STATUS: &str = "activo";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatientInput {
    pub full_name: String,
    pub preferred_name: Option<String>,
    pub rut: Option<String>,
    pub birth_date: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub emergency_contact_name: Option<String>,
    pub emergency_contact_phone: Option<String>,
    pub emergency_contact_relationship: Option<String>,
    pub status: Option<String>,
    pub referred_by: Option<String>,
    pub intake_date: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatientListItem {
    pub id: String,
    pub full_name: String,
    pub preferred_name: Option<String>,
    pub status: String,
    pub intake_date: Option<String>,
}

impl From<PatientSummary> for PatientListItem {
    fn from(p: PatientSummary) -> Self {
        Self {
            id: p.id,
            full_name: p.full_name,
            preferred_name: p.preferred_name,
            status: p.status,
            intake_date: p.intake_date,
        }
    }
}

#[derive(Debug)]
pub enum PatientValidationError {
    EmptyFullName,
    InvalidStatus(String),
    InvalidRut(RutError),
    InvalidDate { field: &'static str },
}

impl fmt::Display for PatientValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatientValidationError::EmptyFullName => write!(f, "el nombre completo es obligatorio"),
            PatientValidationError::InvalidStatus(s) => {
                write!(f, "estado inválido: '{s}' (debe ser uno de: {})", VALID_STATUSES.join(", "))
            }
            PatientValidationError::InvalidRut(e) => write!(f, "RUT inválido: {e}"),
            PatientValidationError::InvalidDate { field } => {
                write!(f, "fecha inválida en '{field}' (formato esperado: AAAA-MM-DD)")
            }
        }
    }
}
impl std::error::Error for PatientValidationError {}

#[derive(Debug)]
pub enum PatientError {
    Validation(PatientValidationError),
    NotFound,
    Database(rusqlite::Error),
}
impl fmt::Display for PatientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatientError::Validation(e) => write!(f, "{e}"),
            PatientError::NotFound => write!(f, "paciente no encontrado"),
            // Nunca se interpola el error de rusqlite con datos de la fila
            // (podría incluir valores) — solo un mensaje genérico técnico.
            PatientError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for PatientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PatientError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for PatientError {
    fn from(e: rusqlite::Error) -> Self {
        PatientError::Database(e)
    }
}
impl From<PatientValidationError> for PatientError {
    fn from(e: PatientValidationError) -> Self {
        PatientError::Validation(e)
    }
}

fn none_if_blank(s: Option<String>) -> Option<String> {
    let trimmed = s.map(|v| v.trim().to_string());
    match trimmed {
        Some(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

fn validate_status(status: &str) -> Result<(), PatientValidationError> {
    if VALID_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(PatientValidationError::InvalidStatus(status.to_string()))
    }
}

fn validate_date_format(value: &str, field: &'static str) -> Result<(), PatientValidationError> {
    let bytes = value.as_bytes();
    let valid_shape = bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-';
    let parse = |s: &str| s.parse::<u32>().ok();
    let parts_ok = valid_shape
        && match (parse(&value[0..4]), parse(&value[5..7]), parse(&value[8..10])) {
            (Some(_year), Some(month), Some(day)) => (1..=12).contains(&month) && (1..=31).contains(&day),
            _ => false,
        };
    if parts_ok {
        Ok(())
    } else {
        Err(PatientValidationError::InvalidDate { field })
    }
}

struct ValidatedFields {
    full_name: String,
    preferred_name: Option<String>,
    rut: Option<String>,
    birth_date: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    address: Option<String>,
    emergency_contact_name: Option<String>,
    emergency_contact_phone: Option<String>,
    emergency_contact_relationship: Option<String>,
    status: String,
    referred_by: Option<String>,
    intake_date: Option<String>,
}

/// Validación autoritativa: se ejecuta siempre en Rust, sin importar lo que
/// haya validado (o no) el formulario en React.
fn validate(input: PatientInput) -> Result<ValidatedFields, PatientValidationError> {
    let full_name = input.full_name.trim().to_string();
    if full_name.is_empty() {
        return Err(PatientValidationError::EmptyFullName);
    }

    let status = input.status.unwrap_or_else(|| DEFAULT_STATUS.to_string());
    validate_status(&status)?;

    let rut = none_if_blank(input.rut);
    let rut = match rut {
        Some(value) => {
            rut::validate_chilean_rut(&value).map_err(PatientValidationError::InvalidRut)?;
            Some(rut::normalize_chilean_rut(&value))
        }
        None => None,
    };

    let birth_date = none_if_blank(input.birth_date);
    if let Some(ref d) = birth_date {
        validate_date_format(d, "birthDate")?;
    }
    let intake_date = none_if_blank(input.intake_date);
    if let Some(ref d) = intake_date {
        validate_date_format(d, "intakeDate")?;
    }

    Ok(ValidatedFields {
        full_name,
        preferred_name: none_if_blank(input.preferred_name),
        rut,
        birth_date,
        phone: none_if_blank(input.phone),
        email: none_if_blank(input.email),
        address: none_if_blank(input.address),
        emergency_contact_name: none_if_blank(input.emergency_contact_name),
        emergency_contact_phone: none_if_blank(input.emergency_contact_phone),
        emergency_contact_relationship: none_if_blank(input.emergency_contact_relationship),
        status,
        referred_by: none_if_blank(input.referred_by),
        intake_date,
    })
}

pub fn create_patient(conn: &Connection, input: PatientInput) -> Result<Patient, PatientError> {
    let f = validate(input)?;
    let id = uuid::Uuid::new_v4().to_string();
    let row = NewPatientRow {
        id: &id,
        full_name: &f.full_name,
        preferred_name: f.preferred_name.as_deref(),
        rut: f.rut.as_deref(),
        birth_date: f.birth_date.as_deref(),
        phone: f.phone.as_deref(),
        email: f.email.as_deref(),
        address: f.address.as_deref(),
        emergency_contact_name: f.emergency_contact_name.as_deref(),
        emergency_contact_phone: f.emergency_contact_phone.as_deref(),
        emergency_contact_relationship: f.emergency_contact_relationship.as_deref(),
        status: &f.status,
        referred_by: f.referred_by.as_deref(),
        intake_date: f.intake_date.as_deref(),
    };
    Ok(patients::insert(conn, &row)?)
}

pub fn get_patient(conn: &Connection, id: &str) -> Result<Patient, PatientError> {
    patients::find_by_id(conn, id)?.ok_or(PatientError::NotFound)
}

pub fn list_patients(conn: &Connection, search: Option<String>) -> Result<Vec<PatientListItem>, PatientError> {
    let search = search.filter(|s| !s.trim().is_empty());
    let rows = patients::list_active(conn, search.as_deref())?;
    Ok(rows.into_iter().map(PatientListItem::from).collect())
}

/// Vista de "archivados": pacientes con soft delete aplicado, para la
/// papelera desde la que se pueden revisar y restaurar. Nunca se mezcla con
/// `list_patients` (que solo devuelve pacientes activos) — son dos
/// consultas explícitamente separadas, igual que en el repositorio.
pub fn list_archived_patients(conn: &Connection, search: Option<String>) -> Result<Vec<PatientListItem>, PatientError> {
    let search = search.filter(|s| !s.trim().is_empty());
    let rows = patients::list_deleted(conn, search.as_deref())?;
    Ok(rows.into_iter().map(PatientListItem::from).collect())
}

pub fn update_patient(conn: &Connection, id: &str, input: PatientInput) -> Result<Patient, PatientError> {
    let f = validate(input)?;
    let row = PatientUpdateRow {
        full_name: &f.full_name,
        preferred_name: f.preferred_name.as_deref(),
        rut: f.rut.as_deref(),
        birth_date: f.birth_date.as_deref(),
        phone: f.phone.as_deref(),
        email: f.email.as_deref(),
        address: f.address.as_deref(),
        emergency_contact_name: f.emergency_contact_name.as_deref(),
        emergency_contact_phone: f.emergency_contact_phone.as_deref(),
        emergency_contact_relationship: f.emergency_contact_relationship.as_deref(),
        status: &f.status,
        referred_by: f.referred_by.as_deref(),
        intake_date: f.intake_date.as_deref(),
    };
    patients::update(conn, id, &row)?.ok_or(PatientError::NotFound)
}

/// Soft delete únicamente. No existe, en ningún punto de este servicio ni
/// del repositorio, una operación de borrado físico alcanzable desde un
/// comando normal de la aplicación.
pub fn archive_patient(conn: &Connection, id: &str) -> Result<(), PatientError> {
    if patients::soft_delete(conn, id)? {
        Ok(())
    } else {
        Err(PatientError::NotFound)
    }
}

pub fn restore_patient(conn: &Connection, id: &str) -> Result<(), PatientError> {
    if patients::restore(conn, id)? {
        Ok(())
    } else {
        Err(PatientError::NotFound)
    }
}

#[allow(dead_code)] // se usa desde los tests de este módulo para inspeccionar un registro eliminado
pub fn get_patient_including_deleted(conn: &Connection, id: &str) -> Result<Patient, PatientError> {
    patients::find_by_id(conn, id)?.ok_or(PatientError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-patients-service-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x42u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn minimal_input(name: &str) -> PatientInput {
        PatientInput {
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
        }
    }

    #[test]
    fn creates_a_patient_with_defaults() {
        let conn = test_conn("create-defaults");
        let p = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        assert_eq!(p.full_name, "Ana Pérez");
        assert_eq!(p.status, "activo");
        assert!(p.deleted_at.is_none());
    }

    #[test]
    fn rejects_empty_full_name() {
        let conn = test_conn("reject-empty-name");
        let err = create_patient(&conn, minimal_input("   ")).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::EmptyFullName)
        ));
    }

    #[test]
    fn rejects_invalid_status() {
        let conn = test_conn("reject-invalid-status");
        let mut input = minimal_input("Ana Pérez");
        input.status = Some("no_existe".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::InvalidStatus(_))
        ));
    }

    #[test]
    fn rejects_invalid_rut() {
        let conn = test_conn("reject-invalid-rut");
        let mut input = minimal_input("Ana Pérez");
        input.rut = Some("12345678-9".to_string()); // dígito verificador incorrecto
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::InvalidRut(_))
        ));
    }

    #[test]
    fn accepts_and_normalizes_a_valid_rut() {
        let conn = test_conn("accept-valid-rut");
        let mut input = minimal_input("Ana Pérez");
        input.rut = Some("12.345.678-5".to_string());
        let p = create_patient(&conn, input).unwrap();
        assert_eq!(p.rut.as_deref(), Some("12345678-5"));
    }

    #[test]
    fn rejects_malformed_birth_date() {
        let conn = test_conn("reject-bad-birthdate");
        let mut input = minimal_input("Ana Pérez");
        input.birth_date = Some("31-02-2000".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::InvalidDate { field: "birthDate" })
        ));
    }

    #[test]
    fn reads_a_created_patient_back() {
        let conn = test_conn("read-back");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        let fetched = get_patient(&conn, &created.id).unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.full_name, "Ana Pérez");
    }

    #[test]
    fn updates_a_patient() {
        let conn = test_conn("update");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        let mut update = minimal_input("Ana María Pérez");
        update.phone = Some("+56911112222".to_string());
        let updated = update_patient(&conn, &created.id, update).unwrap();
        assert_eq!(updated.full_name, "Ana María Pérez");
        assert_eq!(updated.phone.as_deref(), Some("+56911112222"));
    }

    #[test]
    fn archiving_soft_deletes_and_hides_from_listing_but_keeps_the_row() {
        let conn = test_conn("archive-soft-delete");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();

        archive_patient(&conn, &created.id).unwrap();

        let listed = list_patients(&conn, None).unwrap();
        assert!(!listed.iter().any(|p| p.id == created.id), "no debe aparecer en el listado normal");

        let still_in_db = get_patient_including_deleted(&conn, &created.id).unwrap();
        assert!(still_in_db.deleted_at.is_some(), "el registro debe seguir existiendo en la base");
    }

    #[test]
    fn restoring_a_soft_deleted_patient_brings_it_back_to_the_listing() {
        let conn = test_conn("restore");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        archive_patient(&conn, &created.id).unwrap();

        restore_patient(&conn, &created.id).unwrap();

        let listed = list_patients(&conn, None).unwrap();
        assert!(listed.iter().any(|p| p.id == created.id));
    }

    #[test]
    fn searches_patients_by_name_against_the_real_database() {
        let conn = test_conn("search");
        create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        create_patient(&conn, minimal_input("Bruno Soto")).unwrap();

        let results = list_patients(&conn, Some("ana".to_string())).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].full_name, "Ana Pérez");

        let no_results = list_patients(&conn, Some("zzz_no_deberia_existir".to_string())).unwrap();
        assert!(no_results.is_empty());
    }

    #[test]
    fn list_items_never_include_the_rut_field() {
        // Verificación estructural: PatientListItem ni siquiera tiene un
        // campo `rut` — este test documenta esa garantía y falla en
        // tiempo de compilación (no en runtime) si alguien lo agrega sin
        // querer, porque el struct-literal de abajo dejaría de compilar
        // por campos inesperados si `PatientListItem` cambiara de forma
        // incompatible en otro lugar del archivo.
        let item = PatientListItem {
            id: "x".into(),
            full_name: "x".into(),
            preferred_name: None,
            status: "activo".into(),
            intake_date: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("rut"));
    }

    #[test]
    fn archiving_a_nonexistent_patient_reports_not_found() {
        let conn = test_conn("archive-not-found");
        let err = archive_patient(&conn, "no-existe").unwrap_err();
        assert!(matches!(err, PatientError::NotFound));
    }

    #[test]
    fn archived_list_shows_only_soft_deleted_patients_and_hides_active_ones() {
        let conn = test_conn("archived-list");
        let active = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        let archived = create_patient(&conn, minimal_input("Bruno Soto")).unwrap();
        archive_patient(&conn, &archived.id).unwrap();

        let archived_list = list_archived_patients(&conn, None).unwrap();
        assert_eq!(archived_list.len(), 1);
        assert_eq!(archived_list[0].id, archived.id);
        assert!(!archived_list.iter().any(|p| p.id == active.id));

        let active_list = list_patients(&conn, None).unwrap();
        assert!(active_list.iter().any(|p| p.id == active.id));
        assert!(!active_list.iter().any(|p| p.id == archived.id));
    }

    #[test]
    fn restoring_a_patient_removes_it_from_the_archived_list() {
        let conn = test_conn("archived-list-restore");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        archive_patient(&conn, &created.id).unwrap();
        assert_eq!(list_archived_patients(&conn, None).unwrap().len(), 1);

        restore_patient(&conn, &created.id).unwrap();

        assert!(list_archived_patients(&conn, None).unwrap().is_empty());
    }

    #[test]
    fn searches_archived_patients_by_name() {
        let conn = test_conn("archived-search");
        let ana = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        let bruno = create_patient(&conn, minimal_input("Bruno Soto")).unwrap();
        archive_patient(&conn, &ana.id).unwrap();
        archive_patient(&conn, &bruno.id).unwrap();

        let results = list_archived_patients(&conn, Some("ana".to_string())).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].full_name, "Ana Pérez");
    }
}
