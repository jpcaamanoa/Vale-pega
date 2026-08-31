//! Reglas de negocio de citas: validación autoritativa y orquestación del
//! repositorio. No sabe nada de Tauri, del estado de bloqueo del vault, ni
//! de Google Calendar — la sincronización vive en el módulo `calendar` y se
//! invoca desde `commands::appointments`, nunca desde aquí. Mismo principio
//! que `services::patients`: esta capa nunca envía nada fuera del proceso.

use std::fmt;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::repositories::appointments::{
    self, Appointment, AppointmentUpdateRow, NewAppointmentRow,
};

pub const VALID_MODALITIES: &[&str] = &["presencial", "online", "telefonico"];
const DEFAULT_TITLE_WITH_PATIENT: &str = "Sesión clínica";
const DEFAULT_TITLE_WITHOUT_PATIENT: &str = "Bloqueo personal";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppointmentInput {
    pub patient_id: Option<String>,
    pub starts_at: String,
    pub ends_at: String,
    pub modality: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlapWarning {
    pub starts_at: String,
    pub ends_at: String,
    pub has_patient: bool,
}

impl From<Appointment> for OverlapWarning {
    fn from(a: Appointment) -> Self {
        // Minimización de exposición (ARCHITECTURE.md 13.A): la advertencia
        // de solapamiento nunca revela el nombre del otro paciente — solo el
        // horario y si esa otra cita tiene o no paciente asociado. Es
        // suficiente para que la usuaria entienda el conflicto sin exponer
        // la agenda de un paciente distinto al que está agendando ahora.
        Self { starts_at: a.starts_at, ends_at: a.ends_at, has_patient: a.patient_id.is_some() }
    }
}

#[derive(Debug)]
pub enum AppointmentValidationError {
    EndBeforeOrEqualStart,
    InvalidDateTime { field: &'static str },
    InvalidModality(String),
}

impl fmt::Display for AppointmentValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppointmentValidationError::EndBeforeOrEqualStart => {
                write!(f, "la hora de término debe ser posterior a la hora de inicio")
            }
            AppointmentValidationError::InvalidDateTime { field } => {
                write!(f, "fecha/hora inválida en '{field}' (formato esperado: AAAA-MM-DDTHH:MM:SSZ)")
            }
            AppointmentValidationError::InvalidModality(m) => {
                write!(f, "modalidad inválida: '{m}' (debe ser una de: {})", VALID_MODALITIES.join(", "))
            }
        }
    }
}
impl std::error::Error for AppointmentValidationError {}

#[derive(Debug)]
pub enum AppointmentError {
    Validation(AppointmentValidationError),
    NotFound,
    Database(rusqlite::Error),
}
impl fmt::Display for AppointmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppointmentError::Validation(e) => write!(f, "{e}"),
            AppointmentError::NotFound => write!(f, "cita no encontrada"),
            AppointmentError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for AppointmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppointmentError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for AppointmentError {
    fn from(e: rusqlite::Error) -> Self {
        // Una violación de la foreign key hacia `patients` (paciente
        // inexistente) también termina aquí como un error genérico — el
        // mensaje no distingue el motivo exacto, igual que el resto de la
        // capa de servicios.
        AppointmentError::Database(e)
    }
}
impl From<AppointmentValidationError> for AppointmentError {
    fn from(e: AppointmentValidationError) -> Self {
        AppointmentError::Validation(e)
    }
}

/// Formato esperado: `AAAA-MM-DDTHH:MM:SSZ` (UTC, ISO-8601), el mismo que ya
/// usan `created_at`/`updated_at` en todo el esquema. No se valida
/// calendáricamente (igual que las fechas de pacientes) — solo la forma.
fn validate_datetime_format(value: &str, field: &'static str) -> Result<(), AppointmentValidationError> {
    let bytes = value.as_bytes();
    let shape_ok = bytes.len() >= 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[value.len() - 1] == b'Z';
    let parse = |s: &str| s.parse::<u32>().ok();
    let parts_ok = shape_ok
        && match (parse(&value[0..4]), parse(&value[5..7]), parse(&value[8..10]), parse(&value[11..13]), parse(&value[14..16])) {
            (Some(_year), Some(month), Some(day), Some(hour), Some(minute)) => {
                (1..=12).contains(&month) && (1..=31).contains(&day) && hour <= 23 && minute <= 59
            }
            _ => false,
        };
    if parts_ok {
        Ok(())
    } else {
        Err(AppointmentValidationError::InvalidDateTime { field })
    }
}

fn validate_modality(modality: &Option<String>) -> Result<(), AppointmentValidationError> {
    match modality {
        Some(m) if !VALID_MODALITIES.contains(&m.as_str()) => {
            Err(AppointmentValidationError::InvalidModality(m.clone()))
        }
        _ => Ok(()),
    }
}

struct ValidatedFields {
    patient_id: Option<String>,
    starts_at: String,
    ends_at: String,
    modality: Option<String>,
}

fn validate(input: AppointmentInput) -> Result<ValidatedFields, AppointmentValidationError> {
    validate_datetime_format(&input.starts_at, "startsAt")?;
    validate_datetime_format(&input.ends_at, "endsAt")?;
    // Redundante con el CHECK de la base de datos a propósito: da un
    // mensaje de error claro en Rust en vez de depender únicamente de que
    // SQLite rechace el INSERT/UPDATE — el CHECK sigue siendo la barrera
    // real e inevitable, esto es solo una validación temprana.
    if input.ends_at.as_str() <= input.starts_at.as_str() {
        return Err(AppointmentValidationError::EndBeforeOrEqualStart);
    }
    validate_modality(&input.modality)?;

    let patient_id = input.patient_id.filter(|s| !s.trim().is_empty());
    let modality = input.modality.filter(|s| !s.trim().is_empty());

    Ok(ValidatedFields { patient_id, starts_at: input.starts_at, ends_at: input.ends_at, modality })
}

pub fn create_appointment(conn: &Connection, input: AppointmentInput) -> Result<Appointment, AppointmentError> {
    let f = validate(input)?;
    let id = uuid::Uuid::new_v4().to_string();
    let title = if f.patient_id.is_some() { DEFAULT_TITLE_WITH_PATIENT } else { DEFAULT_TITLE_WITHOUT_PATIENT };
    let row = NewAppointmentRow {
        id: &id,
        patient_id: f.patient_id.as_deref(),
        title,
        starts_at: &f.starts_at,
        ends_at: &f.ends_at,
        modality: f.modality.as_deref(),
    };
    Ok(appointments::insert(conn, &row)?)
}

pub fn get_appointment(conn: &Connection, id: &str) -> Result<Appointment, AppointmentError> {
    appointments::find_by_id(conn, id)?.ok_or(AppointmentError::NotFound)
}

pub fn list_appointments(
    conn: &Connection,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<Appointment>, AppointmentError> {
    Ok(appointments::list_active(conn, from.as_deref(), to.as_deref())?)
}

pub fn list_archived_appointments(
    conn: &Connection,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<Appointment>, AppointmentError> {
    Ok(appointments::list_deleted(conn, from.as_deref(), to.as_deref())?)
}

/// Nunca bloquea el guardado — solo informa. Ver `OverlapWarning` sobre por
/// qué no revela el paciente de la cita en conflicto.
pub fn check_overlap(
    conn: &Connection,
    starts_at: &str,
    ends_at: &str,
    exclude_id: Option<&str>,
) -> Result<Vec<OverlapWarning>, AppointmentError> {
    let overlapping = appointments::find_overlapping(conn, starts_at, ends_at, exclude_id)?;
    Ok(overlapping.into_iter().map(OverlapWarning::from).collect())
}

pub fn update_appointment(conn: &Connection, id: &str, input: AppointmentInput) -> Result<Appointment, AppointmentError> {
    let f = validate(input)?;
    let row = AppointmentUpdateRow {
        patient_id: f.patient_id.as_deref(),
        starts_at: &f.starts_at,
        ends_at: &f.ends_at,
        modality: f.modality.as_deref(),
    };
    appointments::update(conn, id, &row)?.ok_or(AppointmentError::NotFound)
}

/// Marca la cita como cancelada. Distinto de archivar: el registro sigue
/// siendo un resultado visible en el historial, no una fila oculta. El
/// efecto sobre el evento espejo de Google (si existe) lo decide
/// `commands::appointments`, no este servicio.
pub fn cancel_appointment(conn: &Connection, id: &str) -> Result<Appointment, AppointmentError> {
    if appointments::set_status(conn, id, "cancelada")? {
        get_appointment(conn, id)
    } else {
        Err(AppointmentError::NotFound)
    }
}

/// Soft delete únicamente. No existe, en ningún punto de este servicio ni
/// del repositorio, una operación de borrado físico alcanzable desde un
/// comando normal de la aplicación.
pub fn archive_appointment(conn: &Connection, id: &str) -> Result<(), AppointmentError> {
    if appointments::soft_delete(conn, id)? {
        Ok(())
    } else {
        Err(AppointmentError::NotFound)
    }
}

pub fn restore_appointment(conn: &Connection, id: &str) -> Result<Appointment, AppointmentError> {
    if appointments::restore(conn, id)? {
        get_appointment(conn, id)
    } else {
        Err(AppointmentError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::services::patients::{self, PatientInput};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-appointments-service-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x77u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn minimal_input(starts_at: &str, ends_at: &str) -> AppointmentInput {
        AppointmentInput {
            patient_id: None,
            starts_at: starts_at.to_string(),
            ends_at: ends_at.to_string(),
            modality: None,
        }
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
        };
        patients::create_patient(conn, input).unwrap().id
    }

    #[test]
    fn creates_an_appointment_without_a_patient() {
        let conn = test_conn("create-no-patient");
        let a = create_appointment(&conn, minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z")).unwrap();
        assert!(a.patient_id.is_none());
        assert!(a.deleted_at.is_none());
        assert_eq!(a.status, "programada");
    }

    #[test]
    fn creates_an_appointment_with_a_patient() {
        let conn = test_conn("create-with-patient");
        let patient_id = create_test_patient(&conn, "Ana Pérez");
        let mut input = minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z");
        input.patient_id = Some(patient_id.clone());
        let a = create_appointment(&conn, input).unwrap();
        assert_eq!(a.patient_id.as_deref(), Some(patient_id.as_str()));
        assert_eq!(a.patient_name.as_deref(), Some("Ana Pérez"));
    }

    #[test]
    fn rejects_end_before_start() {
        let conn = test_conn("reject-end-before-start");
        let err = create_appointment(&conn, minimal_input("2026-09-01T16:00:00Z", "2026-09-01T15:00:00Z")).unwrap_err();
        assert!(matches!(err, AppointmentError::Validation(AppointmentValidationError::EndBeforeOrEqualStart)));
    }

    #[test]
    fn rejects_end_equal_to_start() {
        let conn = test_conn("reject-end-equal-start");
        let err = create_appointment(&conn, minimal_input("2026-09-01T15:00:00Z", "2026-09-01T15:00:00Z")).unwrap_err();
        assert!(matches!(err, AppointmentError::Validation(AppointmentValidationError::EndBeforeOrEqualStart)));
    }

    #[test]
    fn rejects_malformed_datetime() {
        let conn = test_conn("reject-malformed-datetime");
        let err = create_appointment(&conn, minimal_input("no-es-fecha", "2026-09-01T15:00:00Z")).unwrap_err();
        assert!(matches!(
            err,
            AppointmentError::Validation(AppointmentValidationError::InvalidDateTime { field: "startsAt" })
        ));
    }

    #[test]
    fn rejects_invalid_modality() {
        let conn = test_conn("reject-invalid-modality");
        let mut input = minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z");
        input.modality = Some("teletransporte".to_string());
        let err = create_appointment(&conn, input).unwrap_err();
        assert!(matches!(err, AppointmentError::Validation(AppointmentValidationError::InvalidModality(_))));
    }

    #[test]
    fn overlapping_appointments_are_detected_but_both_are_saved() {
        let conn = test_conn("overlap-detected-not-blocked");
        create_appointment(&conn, minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z")).unwrap();
        // Se solapa (15:30-16:30 cruza con 15:00-16:00) — igual se crea.
        let second = create_appointment(&conn, minimal_input("2026-09-01T15:30:00Z", "2026-09-01T16:30:00Z")).unwrap();
        assert_eq!(second.status, "programada");

        let warnings = check_overlap(&conn, "2026-09-01T15:30:00Z", "2026-09-01T16:30:00Z", Some(&second.id)).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].starts_at, "2026-09-01T15:00:00Z");
    }

    #[test]
    fn non_overlapping_appointments_report_no_warning() {
        let conn = test_conn("no-overlap");
        create_appointment(&conn, minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z")).unwrap();
        let warnings = check_overlap(&conn, "2026-09-01T16:00:00Z", "2026-09-01T17:00:00Z", None).unwrap();
        assert!(warnings.is_empty(), "citas contiguas (fin=inicio) no deberían considerarse solapadas");
    }

    #[test]
    fn cancelled_appointments_are_excluded_from_overlap_check() {
        let conn = test_conn("overlap-excludes-cancelled");
        let a = create_appointment(&conn, minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z")).unwrap();
        cancel_appointment(&conn, &a.id).unwrap();
        let warnings = check_overlap(&conn, "2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z", None).unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn overlap_warning_never_includes_a_patient_name() {
        let conn = test_conn("overlap-warning-no-name");
        let patient_id = create_test_patient(&conn, "Nombre Muy Identificable");
        let mut input = minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z");
        input.patient_id = Some(patient_id);
        create_appointment(&conn, input).unwrap();

        let warnings = check_overlap(&conn, "2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z", None).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].has_patient);
        let json = serde_json::to_string(&warnings[0]).unwrap();
        assert!(!json.contains("Nombre Muy Identificable"));
    }

    #[test]
    fn cancelling_keeps_the_row_visible_as_a_historical_record() {
        let conn = test_conn("cancel-keeps-record");
        let a = create_appointment(&conn, minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z")).unwrap();
        let cancelled = cancel_appointment(&conn, &a.id).unwrap();
        assert_eq!(cancelled.status, "cancelada");
        assert!(cancelled.deleted_at.is_none(), "cancelar no es archivar");

        let listed = list_appointments(&conn, None, None).unwrap();
        assert!(listed.iter().any(|x| x.id == a.id), "una cita cancelada sigue en el listado activo");
    }

    #[test]
    fn archiving_soft_deletes_and_hides_from_the_active_listing() {
        let conn = test_conn("archive-soft-delete");
        let a = create_appointment(&conn, minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z")).unwrap();
        archive_appointment(&conn, &a.id).unwrap();

        let active = list_appointments(&conn, None, None).unwrap();
        assert!(!active.iter().any(|x| x.id == a.id));

        let archived = list_archived_appointments(&conn, None, None).unwrap();
        assert!(archived.iter().any(|x| x.id == a.id));
    }

    #[test]
    fn restoring_brings_it_back_to_the_active_listing() {
        let conn = test_conn("restore");
        let a = create_appointment(&conn, minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z")).unwrap();
        archive_appointment(&conn, &a.id).unwrap();
        let restored = restore_appointment(&conn, &a.id).unwrap();
        assert!(restored.deleted_at.is_none());

        let active = list_appointments(&conn, None, None).unwrap();
        assert!(active.iter().any(|x| x.id == a.id));
    }

    #[test]
    fn list_by_range_only_returns_appointments_overlapping_the_range() {
        let conn = test_conn("list-by-range");
        create_appointment(&conn, minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z")).unwrap();
        create_appointment(&conn, minimal_input("2026-09-05T15:00:00Z", "2026-09-05T16:00:00Z")).unwrap();

        let today_only = list_appointments(
            &conn,
            Some("2026-09-01T00:00:00Z".to_string()),
            Some("2026-09-01T23:59:59Z".to_string()),
        )
        .unwrap();
        assert_eq!(today_only.len(), 1);
        assert_eq!(today_only[0].starts_at, "2026-09-01T15:00:00Z");
    }

    #[test]
    fn creating_an_appointment_for_a_nonexistent_patient_fails() {
        let conn = test_conn("nonexistent-patient-fk");
        let mut input = minimal_input("2026-09-01T15:00:00Z", "2026-09-01T16:00:00Z");
        input.patient_id = Some("no-existe".to_string());
        let err = create_appointment(&conn, input).unwrap_err();
        assert!(matches!(err, AppointmentError::Database(_)));
    }
}
