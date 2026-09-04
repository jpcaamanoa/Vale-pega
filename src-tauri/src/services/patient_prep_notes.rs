//! Reglas de negocio de "preparación para la próxima sesión" (Fase 8). Ver
//! `docs/session-continuity.md` para el diseño completo.
//!
//! Distinto de `session_notes.next_focus`: una preparación es un registro
//! **operativo** con su propio `status`, independiente de cualquier nota
//! clínica concreta — sobrevive el cierre de la nota que (opcionalmente) la
//! originó, y se resuelve (`abordado`/`descartado`) como una acción propia,
//! nunca editando retroactivamente una nota ya cerrada.
//!
//! Regla de integridad: si se informa `origin_session_id`, esa sesión debe
//! pertenecer al mismo paciente de la preparación — mismo patrón que
//! `services::payments::check_session_belongs_to_patient` /
//! `services::goals::link_session_goal`.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::patient_prep_notes::{self, NewPrepNoteRow, PatientPrepNote};
use crate::repositories::patients;
use crate::repositories::sessions;

pub const VALID_STATUSES: &[&str] = &["pendiente", "abordado", "descartado"];
const DEFAULT_STATUS: &str = "pendiente";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepNoteInput {
    pub patient_id: String,
    pub origin_session_id: Option<String>,
    pub content: String,
}

#[derive(Debug)]
pub enum PrepNoteValidationError {
    ContentRequired,
    InvalidStatus(String),
}

impl fmt::Display for PrepNoteValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrepNoteValidationError::ContentRequired => write!(f, "el contenido de la preparación es obligatorio"),
            PrepNoteValidationError::InvalidStatus(s) => {
                write!(f, "estado inválido: '{s}' (debe ser uno de: {})", VALID_STATUSES.join(", "))
            }
        }
    }
}
impl std::error::Error for PrepNoteValidationError {}

#[derive(Debug)]
pub enum PrepNoteError {
    Validation(PrepNoteValidationError),
    NotFound,
    NotEditable,
    PatientNotFound,
    PatientArchived,
    SessionNotFound,
    PatientMismatch,
    Database(rusqlite::Error),
}

impl fmt::Display for PrepNoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrepNoteError::Validation(e) => write!(f, "{e}"),
            PrepNoteError::NotFound => write!(f, "preparación no encontrada"),
            PrepNoteError::NotEditable => write!(f, "solo se puede editar el contenido mientras la preparación está pendiente"),
            PrepNoteError::PatientNotFound => write!(f, "paciente no encontrado"),
            PrepNoteError::PatientArchived => write!(f, "no se pueden crear preparaciones nuevas para un paciente archivado"),
            PrepNoteError::SessionNotFound => write!(f, "sesión no encontrada"),
            PrepNoteError::PatientMismatch => write!(f, "la sesión de origen pertenece a otro paciente"),
            PrepNoteError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for PrepNoteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PrepNoteError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for PrepNoteError {
    fn from(e: rusqlite::Error) -> Self {
        PrepNoteError::Database(e)
    }
}
impl From<PrepNoteValidationError> for PrepNoteError {
    fn from(e: PrepNoteValidationError) -> Self {
        PrepNoteError::Validation(e)
    }
}

fn validate_status(status: &str) -> Result<(), PrepNoteValidationError> {
    if VALID_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(PrepNoteValidationError::InvalidStatus(status.to_string()))
    }
}

/// Si `session_id` viene informado, comprueba que la sesión exista y
/// pertenezca al mismo paciente — nunca se confía solo en el `patientId`
/// enviado por React.
fn check_session_belongs_to_patient(conn: &Connection, session_id: &Option<String>, patient_id: &str) -> Result<(), PrepNoteError> {
    if let Some(session_id) = session_id {
        let session = sessions::find_by_id(conn, session_id)?.ok_or(PrepNoteError::SessionNotFound)?;
        if session.patient_id != patient_id {
            return Err(PrepNoteError::PatientMismatch);
        }
    }
    Ok(())
}

/// Rechaza la creación para un paciente inexistente o archivado — mismo
/// criterio que `services::goals::create_goal` / `services::payments::create_payment`.
/// El concepto es "quiero recordar esto la próxima vez que vea a este
/// paciente": no depende de que exista una cita futura agendada
/// (`origin_session_id` es opcional).
pub fn create_prep_note(conn: &Connection, input: PrepNoteInput) -> Result<PatientPrepNote, PrepNoteError> {
    let patient = patients::find_by_id(conn, &input.patient_id)?.ok_or(PrepNoteError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(PrepNoteError::PatientArchived);
    }

    let content = input.content.trim().to_string();
    if content.is_empty() {
        return Err(PrepNoteValidationError::ContentRequired.into());
    }
    check_session_belongs_to_patient(conn, &input.origin_session_id, &input.patient_id)?;

    let id = uuid::Uuid::new_v4().to_string();
    Ok(patient_prep_notes::insert(
        conn,
        &NewPrepNoteRow { id: &id, patient_id: &input.patient_id, origin_session_id: input.origin_session_id.as_deref(), content: &content },
    )?)
}

pub fn get_prep_note(conn: &Connection, id: &str) -> Result<PatientPrepNote, PrepNoteError> {
    patient_prep_notes::find_by_id(conn, id)?.ok_or(PrepNoteError::NotFound)
}

pub fn list_prep_notes(conn: &Connection, patient_id: &str) -> Result<Vec<PatientPrepNote>, PrepNoteError> {
    Ok(patient_prep_notes::list_by_patient(conn, patient_id)?)
}

pub fn list_pending_prep_notes(conn: &Connection, patient_id: &str) -> Result<Vec<PatientPrepNote>, PrepNoteError> {
    Ok(patient_prep_notes::list_pending_by_patient(conn, patient_id)?)
}

/// Edita el contenido. Autoritativo en el repositorio: si la preparación ya
/// no está `pendiente`, el `UPDATE` no afecta ninguna fila y esta función
/// lo reporta como `NotEditable` en vez de un `NotFound` engañoso — la fila
/// existe, simplemente ya no es editable.
pub fn update_prep_note(conn: &Connection, id: &str, content: String) -> Result<PatientPrepNote, PrepNoteError> {
    let existing = patient_prep_notes::find_by_id(conn, id)?.ok_or(PrepNoteError::NotFound)?;
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err(PrepNoteValidationError::ContentRequired.into());
    }
    if existing.status != DEFAULT_STATUS {
        return Err(PrepNoteError::NotEditable);
    }
    patient_prep_notes::update_content(conn, id, &content)?.ok_or(PrepNoteError::NotEditable)
}

/// Cambia el estado — transición libre entre los tres valores (incluida
/// "volver a pendiente"), igual criterio que `logrado` en Objetivos: ningún
/// estado es terminal.
pub fn set_prep_note_status(conn: &Connection, id: &str, status: String) -> Result<PatientPrepNote, PrepNoteError> {
    validate_status(&status)?;
    patient_prep_notes::set_status(conn, id, &status)?.ok_or(PrepNoteError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self as patients_repo, NewPatientRow};
    use crate::repositories::sessions::{self as sessions_repo, NewSessionRow};
    use crate::services::patients as patients_service;

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-prep-notes-svc-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x41u8; VAULT_KEY_LEN]);
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

    #[test]
    fn creates_a_prep_note_for_an_existing_patient() {
        let conn = test_conn("create");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let n = create_prep_note(&conn, PrepNoteInput { patient_id: patient_id.clone(), origin_session_id: None, content: "Retomar exposición".into() }).unwrap();
        assert_eq!(n.patient_id, patient_id);
        assert_eq!(n.status, "pendiente");
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let conn = test_conn("nonexistent-patient");
        let err = create_prep_note(&conn, PrepNoteInput { patient_id: "no-existe".into(), origin_session_id: None, content: "Nota".into() }).unwrap_err();
        assert!(matches!(err, PrepNoteError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let conn = test_conn("archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = create_prep_note(&conn, PrepNoteInput { patient_id: patient_id.clone(), origin_session_id: None, content: "Nota".into() }).unwrap_err();
        assert!(matches!(err, PrepNoteError::PatientArchived));
    }

    #[test]
    fn rejects_empty_content() {
        let conn = test_conn("empty-content");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let err = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: None, content: "   ".into() }).unwrap_err();
        assert!(matches!(err, PrepNoteError::Validation(PrepNoteValidationError::ContentRequired)));
    }

    #[test]
    fn rejects_an_origin_session_belonging_to_a_different_patient() {
        let conn = test_conn("session-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let session_of_b = create_test_session(&conn, &patient_b);

        let err = create_prep_note(&conn, PrepNoteInput { patient_id: patient_a, origin_session_id: Some(session_of_b), content: "Nota".into() }).unwrap_err();
        assert!(matches!(err, PrepNoteError::PatientMismatch));
    }

    #[test]
    fn rejects_a_nonexistent_origin_session() {
        let conn = test_conn("session-not-found");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let err = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: Some("no-existe".into()), content: "Nota".into() }).unwrap_err();
        assert!(matches!(err, PrepNoteError::SessionNotFound));
    }

    #[test]
    fn lists_pending_and_full_history_separately() {
        let conn = test_conn("list-pending-history");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let n1 = create_prep_note(&conn, PrepNoteInput { patient_id: patient_id.clone(), origin_session_id: None, content: "Uno".into() }).unwrap();
        create_prep_note(&conn, PrepNoteInput { patient_id: patient_id.clone(), origin_session_id: None, content: "Dos".into() }).unwrap();
        set_prep_note_status(&conn, &n1.id, "abordado".into()).unwrap();

        assert_eq!(list_pending_prep_notes(&conn, &patient_id).unwrap().len(), 1);
        assert_eq!(list_prep_notes(&conn, &patient_id).unwrap().len(), 2, "el historial conserva la nota abordada");
    }

    #[test]
    fn update_prep_note_edits_content_while_pending() {
        let conn = test_conn("update-pending");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let n = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: None, content: "Original".into() }).unwrap();
        let updated = update_prep_note(&conn, &n.id, "Editado".into()).unwrap();
        assert_eq!(updated.content, "Editado");
    }

    #[test]
    fn update_prep_note_rejects_editing_a_resolved_note() {
        let conn = test_conn("update-resolved");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let n = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: None, content: "Original".into() }).unwrap();
        set_prep_note_status(&conn, &n.id, "descartado".into()).unwrap();

        let err = update_prep_note(&conn, &n.id, "Ya no debería aplicarse".into()).unwrap_err();
        assert!(matches!(err, PrepNoteError::NotEditable));
        assert_eq!(get_prep_note(&conn, &n.id).unwrap().content, "Original", "el contenido histórico permanece intacto");
    }

    #[test]
    fn update_prep_note_rejects_empty_content() {
        let conn = test_conn("update-empty");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let n = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: None, content: "Original".into() }).unwrap();
        let err = update_prep_note(&conn, &n.id, "   ".into()).unwrap_err();
        assert!(matches!(err, PrepNoteError::Validation(PrepNoteValidationError::ContentRequired)));
    }

    #[test]
    fn set_prep_note_status_marks_as_abordado_and_preserves_history() {
        let conn = test_conn("mark-abordado");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let n = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: None, content: "Nota".into() }).unwrap();
        let resolved = set_prep_note_status(&conn, &n.id, "abordado".into()).unwrap();
        assert_eq!(resolved.status, "abordado");
        // Sigue existiendo y siendo consultable, solo que no aparece en pendientes.
        assert_eq!(get_prep_note(&conn, &n.id).unwrap().status, "abordado");
    }

    #[test]
    fn set_prep_note_status_marks_as_descartado() {
        let conn = test_conn("mark-descartado");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let n = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: None, content: "Nota".into() }).unwrap();
        let resolved = set_prep_note_status(&conn, &n.id, "descartado".into()).unwrap();
        assert_eq!(resolved.status, "descartado");
    }

    #[test]
    fn set_prep_note_status_rejects_invalid_status() {
        let conn = test_conn("invalid-status");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let n = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: None, content: "Nota".into() }).unwrap();
        let err = set_prep_note_status(&conn, &n.id, "inventado".into()).unwrap_err();
        assert!(matches!(err, PrepNoteError::Validation(PrepNoteValidationError::InvalidStatus(_))));
    }

    #[test]
    fn set_prep_note_status_on_unknown_id_reports_not_found() {
        let conn = test_conn("unknown-id");
        let err = set_prep_note_status(&conn, "no-existe", "abordado".into()).unwrap_err();
        assert!(matches!(err, PrepNoteError::NotFound));
    }

    #[test]
    fn a_prep_note_created_from_a_session_records_its_origin() {
        let conn = test_conn("origin-session");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let session_id = create_test_session(&conn, &patient_id);
        let n = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: Some(session_id.clone()), content: "Nota".into() }).unwrap();
        assert_eq!(n.origin_session_id.as_deref(), Some(session_id.as_str()));
    }

    #[test]
    fn a_prep_note_never_requires_a_future_appointment() {
        // Regla 7 de la aprobación: crear una preparación no depende de
        // ninguna cita agendada — este test simplemente confirma que el
        // input no exige nada relacionado con `appointments`.
        let conn = test_conn("no-appointment-needed");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        let n = create_prep_note(&conn, PrepNoteInput { patient_id, origin_session_id: None, content: "Nota sin sesión ni cita".into() }).unwrap();
        assert!(n.origin_session_id.is_none());
    }
}
