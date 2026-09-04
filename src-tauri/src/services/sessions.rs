//! Reglas de negocio de sesiones clínicas y sus notas versionadas. Esta es
//! la única capa que conoce el ciclo de vida completo
//! Borrador → Cerrada → nueva versión (nunca sobrescritura) — ver
//! `docs/sessions.md` para el diseño completo, ya aprobado en
//! `docs/ARCHITECTURE.md` sección 12.5.
//!
//! Principio central, no negociable: **una nota cerrada es inmutable**.
//! Ninguna función de este archivo ejecuta jamás un `UPDATE` de contenido
//! sobre una fila con `is_locked = 1` — y
//! `repositories::session_notes::update_draft_content` lo hace además
//! estructuralmente imposible con su propio `WHERE is_locked = 0`.
//!
//! Esta capa nunca sabe nada de Tauri, del estado de bloqueo del vault, ni
//! envía nada fuera del proceso. Tampoco toca Google Calendar en ningún
//! punto: las sesiones clínicas no se sincronizan, nunca.

use std::fmt;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::repositories::{appointments, patients};
use crate::repositories::session_notes::{self, NewSessionNoteRow, SessionNote};
use crate::repositories::sessions::{self, NewSessionRow, Session, SessionListItem, SessionMetadataUpdateRow};
use crate::services::treatment_episodes::{self, TreatmentEpisodeError};

pub const VALID_MODALITIES: &[&str] = &["presencial", "online", "telefonico"];
pub const VALID_STATUSES: &[&str] = &["programada", "realizada", "cancelada", "no_asistio"];
const DEFAULT_STATUS: &str = "programada";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInput {
    pub patient_id: String,
    pub appointment_id: Option<String>,
    /// Opcional (Fase 9) — el proceso terapéutico al que se vincula esta
    /// sesión. `None` es válido: una sesión puede existir sin proceso
    /// formal (ver `services::treatment_episodes`).
    pub episode_id: Option<String>,
    pub session_date: String,
    pub start_time: Option<String>,
    pub duration_minutes: Option<i64>,
    pub modality: Option<String>,
}

/// Campos de "metadata administrativa" — deliberadamente sin `patientId`
/// ni `appointmentId` (ver `repositories::sessions::SessionMetadataUpdateRow`)
/// y sin ningún campo de `session_notes`: cambiar esto nunca crea una
/// versión nueva de la nota (regla 25 de la aprobación de Fase 4).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadataInput {
    pub session_date: String,
    pub start_time: Option<String>,
    pub duration_minutes: Option<i64>,
    pub modality: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWithNote {
    #[serde(flatten)]
    pub session: Session,
    pub note: SessionNote,
}

#[derive(Debug)]
pub enum SessionValidationError {
    DateFormat,
    TimeFormat,
    Duration,
    Modality(String),
    Status(String),
}

impl fmt::Display for SessionValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionValidationError::DateFormat => write!(f, "fecha inválida (formato esperado: AAAA-MM-DD)"),
            SessionValidationError::TimeFormat => write!(f, "hora inválida (formato esperado: HH:MM)"),
            SessionValidationError::Duration => write!(f, "la duración debe ser mayor que cero"),
            SessionValidationError::Modality(m) => {
                write!(f, "modalidad inválida: '{m}' (debe ser una de: {})", VALID_MODALITIES.join(", "))
            }
            SessionValidationError::Status(s) => {
                write!(f, "estado inválido: '{s}' (debe ser uno de: {})", VALID_STATUSES.join(", "))
            }
        }
    }
}
impl std::error::Error for SessionValidationError {}

#[derive(Debug)]
pub enum SessionError {
    Validation(SessionValidationError),
    NotFound,
    PatientNotFound,
    PatientArchived,
    AppointmentNotFound,
    AppointmentHasNoPatient,
    PatientMismatch,
    AppointmentAlreadyHasSession,
    NoteNotFound,
    NoteIsLocked,
    EmptyNoteContent,
    EpisodeNotFound,
    EpisodeArchived,
    EpisodeNotAssignable,
    EpisodePatientMismatch,
    Database(rusqlite::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Validation(e) => write!(f, "{e}"),
            SessionError::NotFound => write!(f, "sesión no encontrada"),
            SessionError::PatientNotFound => write!(f, "paciente no encontrado"),
            SessionError::PatientArchived => write!(f, "no se pueden crear sesiones nuevas para un paciente archivado"),
            SessionError::AppointmentNotFound => write!(f, "cita no encontrada"),
            SessionError::AppointmentHasNoPatient => {
                write!(f, "esta cita es un bloqueo personal sin paciente — no puede iniciar una sesión")
            }
            SessionError::PatientMismatch => write!(f, "el paciente de la sesión no coincide con el paciente de la cita"),
            SessionError::AppointmentAlreadyHasSession => write!(f, "esta cita ya tiene una sesión asociada"),
            SessionError::NoteNotFound => write!(f, "nota no encontrada"),
            SessionError::NoteIsLocked => write!(f, "esta nota está cerrada y no puede editarse directamente"),
            SessionError::EmptyNoteContent => write!(f, "la nota no puede cerrarse sin contenido"),
            SessionError::EpisodeNotFound => write!(f, "proceso terapéutico no encontrado"),
            SessionError::EpisodeArchived => write!(f, "este proceso está archivado y no puede recibir sesiones nuevas"),
            SessionError::EpisodeNotAssignable => write!(f, "este proceso está cerrado y no puede recibir sesiones nuevas"),
            SessionError::EpisodePatientMismatch => write!(f, "el proceso indicado no pertenece a este paciente"),
            SessionError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SessionError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for SessionError {
    fn from(e: rusqlite::Error) -> Self {
        SessionError::Database(e)
    }
}
impl From<SessionValidationError> for SessionError {
    fn from(e: SessionValidationError) -> Self {
        SessionError::Validation(e)
    }
}
/// Traduce los errores de `treatment_episodes::check_episode_assignable` a
/// sus equivalentes de dominio de sesiones — nunca deja pasar un
/// `TreatmentEpisodeError` crudo hacia arriba.
impl From<TreatmentEpisodeError> for SessionError {
    fn from(e: TreatmentEpisodeError) -> Self {
        match e {
            TreatmentEpisodeError::NotFound => SessionError::EpisodeNotFound,
            TreatmentEpisodeError::EpisodeArchived => SessionError::EpisodeArchived,
            TreatmentEpisodeError::EpisodeNotAssignable => SessionError::EpisodeNotAssignable,
            TreatmentEpisodeError::EpisodePatientMismatch => SessionError::EpisodePatientMismatch,
            TreatmentEpisodeError::Database(err) => SessionError::Database(err),
            // El resto de las variantes (Validation/PatientNotFound/PatientArchived/
            // AnotherEpisodeActive/ClosureNotImplemented) no las produce
            // `check_episode_assignable` — nunca deberían alcanzarse aquí.
            _ => SessionError::EpisodeNotFound,
        }
    }
}

/// Mismo formato y misma forma de validación (estructural, no calendárica)
/// que `services::patients::validate_date_format` — AAAA-MM-DD.
fn validate_date_format(value: &str) -> Result<(), SessionValidationError> {
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
        Err(SessionValidationError::DateFormat)
    }
}

fn validate_time_format(value: &str) -> Result<(), SessionValidationError> {
    let bytes = value.as_bytes();
    let shape_ok = bytes.len() == 5 && bytes[2] == b':';
    let parse = |s: &str| s.parse::<u32>().ok();
    let ok = shape_ok
        && match (parse(&value[0..2]), parse(&value[3..5])) {
            (Some(h), Some(m)) => h <= 23 && m <= 59,
            _ => false,
        };
    if ok {
        Ok(())
    } else {
        Err(SessionValidationError::TimeFormat)
    }
}

fn validate_modality(modality: &Option<String>) -> Result<(), SessionValidationError> {
    match modality {
        Some(m) if !VALID_MODALITIES.contains(&m.as_str()) => Err(SessionValidationError::Modality(m.clone())),
        _ => Ok(()),
    }
}

fn validate_status(status: &str) -> Result<(), SessionValidationError> {
    if VALID_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(SessionValidationError::Status(status.to_string()))
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

struct ValidatedSessionFields {
    session_date: String,
    start_time: Option<String>,
    duration_minutes: Option<i64>,
    modality: Option<String>,
}

fn validate_common(
    session_date: String,
    start_time: Option<String>,
    duration_minutes: Option<i64>,
    modality: Option<String>,
) -> Result<ValidatedSessionFields, SessionValidationError> {
    validate_date_format(&session_date)?;
    let start_time = none_if_blank(start_time);
    if let Some(ref t) = start_time {
        validate_time_format(t)?;
    }
    if let Some(minutes) = duration_minutes {
        if minutes <= 0 {
            return Err(SessionValidationError::Duration);
        }
    }
    validate_modality(&modality)?;
    let modality = none_if_blank(modality);
    Ok(ValidatedSessionFields { session_date, start_time, duration_minutes, modality })
}

/// Crea una sesión y, en la misma transacción, su primera versión de nota
/// (`version = 1`, borrador vacío) — nunca queda una sesión sin nota ni una
/// nota huérfana. Aplica todas las reglas de coherencia con Agenda antes de
/// escribir nada: paciente existente y activo; si hay `appointment_id`, la
/// cita debe existir, tener paciente, coincidir con `patient_id`, y no
/// tener ya una sesión (activa o archivada).
pub fn create_session(conn: &Connection, input: SessionInput) -> Result<SessionWithNote, SessionError> {
    let patient = patients::find_by_id(conn, &input.patient_id)?.ok_or(SessionError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(SessionError::PatientArchived);
    }

    if let Some(appointment_id) = &input.appointment_id {
        let appointment = appointments::find_by_id(conn, appointment_id)?.ok_or(SessionError::AppointmentNotFound)?;
        let appointment_patient_id = appointment.patient_id.ok_or(SessionError::AppointmentHasNoPatient)?;
        if appointment_patient_id != input.patient_id {
            return Err(SessionError::PatientMismatch);
        }
        if sessions::find_by_appointment_id(conn, appointment_id)?.is_some() {
            return Err(SessionError::AppointmentAlreadyHasSession);
        }
    }

    treatment_episodes::check_episode_assignable(conn, &input.episode_id, &input.patient_id)?;

    let f = validate_common(input.session_date, input.start_time, input.duration_minutes, input.modality)?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let note_id = uuid::Uuid::new_v4().to_string();

    let tx = conn.unchecked_transaction()?;
    let session = sessions::insert(
        &tx,
        &NewSessionRow {
            id: &session_id,
            patient_id: &input.patient_id,
            appointment_id: input.appointment_id.as_deref(),
            episode_id: input.episode_id.as_deref(),
            session_date: &f.session_date,
            start_time: f.start_time.as_deref(),
            duration_minutes: f.duration_minutes,
            modality: f.modality.as_deref(),
            status: DEFAULT_STATUS,
        },
    )?;
    let note = session_notes::insert(
        &tx,
        &NewSessionNoteRow {
            id: &note_id,
            session_id: &session_id,
            content: None,
            interventions: None,
            homework_tasks: None,
            next_focus: None,
            version: 1,
        },
    )?;
    tx.commit()?;

    Ok(SessionWithNote { session, note })
}

pub fn get_session(conn: &Connection, id: &str) -> Result<Session, SessionError> {
    sessions::find_by_id(conn, id)?.ok_or(SessionError::NotFound)
}

pub fn get_session_for_appointment(conn: &Connection, appointment_id: &str) -> Result<Option<Session>, SessionError> {
    Ok(sessions::find_by_appointment_id(conn, appointment_id)?)
}

pub fn list_sessions(conn: &Connection, patient_id: &str) -> Result<Vec<SessionListItem>, SessionError> {
    Ok(sessions::list_active_by_patient(conn, patient_id)?)
}

pub fn list_archived_sessions(conn: &Connection, patient_id: &str) -> Result<Vec<SessionListItem>, SessionError> {
    Ok(sessions::list_deleted_by_patient(conn, patient_id)?)
}

/// Cambia únicamente metadata administrativa (fecha, hora, duración,
/// modalidad, estado). Nunca toca `session_notes` — no crea versión nueva.
pub fn update_session_metadata(conn: &Connection, id: &str, input: SessionMetadataInput) -> Result<Session, SessionError> {
    validate_status(&input.status)?;
    let f = validate_common(input.session_date, input.start_time, input.duration_minutes, input.modality)?;
    let row = SessionMetadataUpdateRow {
        session_date: &f.session_date,
        start_time: f.start_time.as_deref(),
        duration_minutes: f.duration_minutes,
        modality: f.modality.as_deref(),
        status: &input.status,
    };
    sessions::update_metadata(conn, id, &row)?.ok_or(SessionError::NotFound)
}

/// Soft delete únicamente. No existe, en ningún punto de este servicio ni
/// del repositorio, una operación de borrado físico. No toca
/// `session_notes` — el historial clínico permanece intacto y consultable.
pub fn archive_session(conn: &Connection, id: &str) -> Result<(), SessionError> {
    if sessions::soft_delete(conn, id)? {
        Ok(())
    } else {
        Err(SessionError::NotFound)
    }
}

pub fn restore_session(conn: &Connection, id: &str) -> Result<Session, SessionError> {
    if sessions::restore(conn, id)? {
        get_session(conn, id)
    } else {
        Err(SessionError::NotFound)
    }
}

/// Conteo global de sesiones del mes actual, para el bloque "Resumen" del
/// Dashboard (Fase 8).
pub fn sessions_this_month_count(conn: &Connection) -> Result<i64, SessionError> {
    Ok(sessions::count_this_month(conn)?)
}

pub fn get_current_note(conn: &Connection, session_id: &str) -> Result<SessionNote, SessionError> {
    session_notes::find_current(conn, session_id)?.ok_or(SessionError::NoteNotFound)
}

pub fn list_note_history(conn: &Connection, session_id: &str) -> Result<Vec<SessionNote>, SessionError> {
    Ok(session_notes::list_history(conn, session_id)?)
}

/// Autoguardado del borrador vigente de una sesión. Nunca toca una nota
/// cerrada: si la vigente ya está cerrada (no debería ocurrir desde el
/// flujo normal — cerrar siempre deja de ser "vigente-editable" solo tras
/// pasar por `create_new_note_version`), se rechaza explícitamente en vez
/// de fallar en silencio o, peor, intentar el `UPDATE`.
pub fn autosave_note_draft(
    conn: &Connection,
    session_id: &str,
    content: Option<String>,
    interventions: Option<String>,
    homework_tasks: Option<String>,
    next_focus: Option<String>,
) -> Result<(), SessionError> {
    let current = session_notes::find_current(conn, session_id)?.ok_or(SessionError::NoteNotFound)?;
    if current.is_locked {
        return Err(SessionError::NoteIsLocked);
    }
    session_notes::update_draft_content(
        conn,
        &current.id,
        none_if_blank(content).as_deref(),
        none_if_blank(interventions).as_deref(),
        none_if_blank(homework_tasks).as_deref(),
        none_if_blank(next_focus).as_deref(),
    )?;
    Ok(())
}

fn has_content(note: &SessionNote) -> bool {
    [&note.content, &note.interventions, &note.homework_tasks, &note.next_focus]
        .into_iter()
        .any(|field| field.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false))
}

/// Cierra la nota vigente de la sesión. Rechaza el cierre si ninguno de los
/// cuatro campos tiene contenido no vacío después de recortar espacios —
/// en ese caso la nota permanece exactamente como estaba (sigue en
/// borrador, `is_locked`/`closed_at` sin tocar). Si la nota vigente ya
/// estaba cerrada, la operación es idempotente: no crea una versión nueva,
/// no vuelve a tocar `closed_at`, y simplemente devuelve la misma nota
/// cerrada.
pub fn close_current_note(conn: &Connection, session_id: &str) -> Result<SessionNote, SessionError> {
    let current = session_notes::find_current(conn, session_id)?.ok_or(SessionError::NoteNotFound)?;
    if current.is_locked {
        return Ok(current);
    }
    if !has_content(&current) {
        return Err(SessionError::EmptyNoteContent);
    }
    session_notes::close(conn, &current.id)?;
    session_notes::find_by_id(conn, &current.id)?.ok_or(SessionError::NoteNotFound)
}

/// Crea la siguiente versión de la nota a partir de la vigente, que debe
/// estar cerrada: copia su contenido como punto de partida del nuevo
/// borrador, marca la anterior como reemplazada, y la nueva versión pasa a
/// ser la única vigente — todo dentro de una única transacción. La versión
/// anterior no se modifica en absoluto salvo `is_current`/`superseded_at`;
/// su contenido permanece intacto para siempre. Si la nota vigente todavía
/// está en borrador (no cerrada), no hay nada que versionar — se devuelve
/// tal cual, sin crear una fila nueva.
pub fn create_new_note_version(conn: &Connection, session_id: &str) -> Result<SessionNote, SessionError> {
    let current = session_notes::find_current(conn, session_id)?.ok_or(SessionError::NoteNotFound)?;
    if !current.is_locked {
        return Ok(current);
    }

    let new_id = uuid::Uuid::new_v4().to_string();
    let tx = conn.unchecked_transaction()?;
    session_notes::mark_superseded(&tx, &current.id)?;
    let new_note = session_notes::insert(
        &tx,
        &NewSessionNoteRow {
            id: &new_id,
            session_id,
            content: current.content.as_deref(),
            interventions: current.interventions.as_deref(),
            homework_tasks: current.homework_tasks.as_deref(),
            next_focus: current.next_focus.as_deref(),
            version: current.version + 1,
        },
    )?;
    tx.commit()?;
    Ok(new_note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::services::appointments::{self, AppointmentInput};
    use crate::services::patients::{self, PatientInput};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-sessions-service-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x33u8; VAULT_KEY_LEN]);
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

    fn minimal_input(patient_id: &str) -> SessionInput {
        SessionInput {
            patient_id: patient_id.to_string(),
            appointment_id: None,
            episode_id: None,
            session_date: "2026-09-01".to_string(),
            start_time: Some("15:00".to_string()),
            duration_minutes: Some(50),
            modality: Some("presencial".to_string()),
        }
    }

    fn create_test_episode(conn: &Connection, patient_id: &str) -> String {
        use crate::services::treatment_episodes::{self, TreatmentEpisodeInput};
        treatment_episodes::create_episode(conn, TreatmentEpisodeInput { patient_id: patient_id.to_string(), started_at: None }).unwrap().id
    }

    // ---- Fase 9: episode_id opcional ----

    #[test]
    fn a_session_can_be_created_without_an_episode() {
        let conn = test_conn("episode-none");
        let patient_id = create_test_patient(&conn, "Paciente Sin Proceso");
        let result = create_session(&conn, minimal_input(&patient_id)).unwrap();
        assert!(result.session.episode_id.is_none());
    }

    #[test]
    fn a_session_can_be_created_with_a_valid_episode_of_the_same_patient() {
        let conn = test_conn("episode-valid");
        let patient_id = create_test_patient(&conn, "Paciente Con Proceso");
        let episode_id = create_test_episode(&conn, &patient_id);
        let mut input = minimal_input(&patient_id);
        input.episode_id = Some(episode_id.clone());
        let result = create_session(&conn, input).unwrap();
        assert_eq!(result.session.episode_id.as_deref(), Some(episode_id.as_str()));
    }

    #[test]
    fn creation_rejects_an_episode_belonging_to_a_different_patient() {
        let conn = test_conn("episode-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let episode_of_a = create_test_episode(&conn, &patient_a);
        let mut input = minimal_input(&patient_b);
        input.episode_id = Some(episode_of_a);
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::EpisodePatientMismatch));
    }

    #[test]
    fn creation_rejects_a_nonexistent_episode() {
        let conn = test_conn("episode-not-found");
        let patient_id = create_test_patient(&conn, "Paciente C");
        let mut input = minimal_input(&patient_id);
        input.episode_id = Some("no-existe".to_string());
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::EpisodeNotFound));
    }

    #[test]
    fn creation_rejects_an_archived_episode() {
        let conn = test_conn("episode-archived");
        let patient_id = create_test_patient(&conn, "Paciente D");
        let episode_id = create_test_episode(&conn, &patient_id);
        crate::services::treatment_episodes::archive_episode(&conn, &episode_id).unwrap();
        let mut input = minimal_input(&patient_id);
        input.episode_id = Some(episode_id);
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::EpisodeArchived));
    }

    #[test]
    fn creation_rejects_a_closed_episode() {
        let conn = test_conn("episode-closed");
        let patient_id = create_test_patient(&conn, "Paciente E");
        let episode_id = create_test_episode(&conn, &patient_id);
        crate::repositories::treatment_episodes::set_status(&conn, &episode_id, "cerrado").unwrap();
        let mut input = minimal_input(&patient_id);
        input.episode_id = Some(episode_id);
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::EpisodeNotAssignable));
    }

    #[test]
    fn creation_accepts_a_paused_episode() {
        let conn = test_conn("episode-paused");
        let patient_id = create_test_patient(&conn, "Paciente F");
        let episode_id = create_test_episode(&conn, &patient_id);
        crate::services::treatment_episodes::set_episode_status(&conn, &episode_id, "pausado").unwrap();
        let mut input = minimal_input(&patient_id);
        input.episode_id = Some(episode_id.clone());
        let result = create_session(&conn, input).unwrap();
        assert_eq!(result.session.episode_id.as_deref(), Some(episode_id.as_str()));
    }

    // ---- creación ----

    #[test]
    fn creates_a_session_with_its_first_note_version_atomically() {
        let conn = test_conn("create-with-note");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let result = create_session(&conn, minimal_input(&patient_id)).unwrap();

        assert_eq!(result.session.patient_id, patient_id);
        assert!(result.session.appointment_id.is_none());
        assert_eq!(result.note.version, 1);
        assert!(!result.note.is_locked);
        assert!(result.note.is_current);
        assert!(result.note.content.is_none());

        // La nota realmente quedó persistida, no solo en el valor devuelto.
        let fetched = get_current_note(&conn, &result.session.id).unwrap();
        assert_eq!(fetched.id, result.note.id);
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let conn = test_conn("create-nonexistent-patient");
        let err = create_session(&conn, minimal_input("no-existe")).unwrap_err();
        assert!(matches!(err, SessionError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let conn = test_conn("create-archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Archivado");
        patients::archive_patient(&conn, &patient_id).unwrap();
        let err = create_session(&conn, minimal_input(&patient_id)).unwrap_err();
        assert!(matches!(err, SessionError::PatientArchived));
    }

    #[test]
    fn rejects_invalid_session_date() {
        let conn = test_conn("create-invalid-date");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let mut input = minimal_input(&patient_id);
        input.session_date = "no-es-fecha".to_string();
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::Validation(SessionValidationError::DateFormat)));
    }

    #[test]
    fn rejects_invalid_start_time() {
        let conn = test_conn("create-invalid-time");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let mut input = minimal_input(&patient_id);
        input.start_time = Some("25:99".to_string());
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::Validation(SessionValidationError::TimeFormat)));
    }

    #[test]
    fn rejects_non_positive_duration() {
        let conn = test_conn("create-invalid-duration");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let mut input = minimal_input(&patient_id);
        input.duration_minutes = Some(0);
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::Validation(SessionValidationError::Duration)));
    }

    #[test]
    fn rejects_invalid_modality() {
        let conn = test_conn("create-invalid-modality");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let mut input = minimal_input(&patient_id);
        input.modality = Some("teletransporte".to_string());
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::Validation(SessionValidationError::Modality(_))));
    }

    // ---- relación con Agenda ----

    fn create_test_appointment(conn: &Connection, patient_id: Option<&str>) -> String {
        let input = AppointmentInput {
            patient_id: patient_id.map(|s| s.to_string()),
            starts_at: "2026-09-01T15:00:00Z".to_string(),
            ends_at: "2026-09-01T16:00:00Z".to_string(),
            modality: None,
        };
        appointments::create_appointment(conn, input).unwrap().id
    }

    #[test]
    fn creates_a_session_from_an_appointment_inheriting_its_patient() {
        let conn = test_conn("create-from-appointment");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let appointment_id = create_test_appointment(&conn, Some(&patient_id));

        let mut input = minimal_input(&patient_id);
        input.appointment_id = Some(appointment_id.clone());
        let result = create_session(&conn, input).unwrap();

        assert_eq!(result.session.appointment_id.as_deref(), Some(appointment_id.as_str()));
        assert_eq!(result.session.patient_id, patient_id);
    }

    #[test]
    fn rejects_a_session_for_an_appointment_without_a_patient() {
        let conn = test_conn("create-appointment-no-patient");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let appointment_id = create_test_appointment(&conn, None);

        let mut input = minimal_input(&patient_id);
        input.appointment_id = Some(appointment_id);
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::AppointmentHasNoPatient));
    }

    #[test]
    fn rejects_a_session_whose_patient_does_not_match_the_appointments_patient() {
        let conn = test_conn("create-patient-mismatch");
        let appointment_patient = create_test_patient(&conn, "Paciente de la Cita");
        let other_patient = create_test_patient(&conn, "Paciente Distinto");
        let appointment_id = create_test_appointment(&conn, Some(&appointment_patient));

        let mut input = minimal_input(&other_patient);
        input.appointment_id = Some(appointment_id);
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::PatientMismatch));
    }

    #[test]
    fn rejects_a_second_session_for_the_same_appointment() {
        let conn = test_conn("create-second-session-same-appointment");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let appointment_id = create_test_appointment(&conn, Some(&patient_id));

        let mut input = minimal_input(&patient_id);
        input.appointment_id = Some(appointment_id.clone());
        create_session(&conn, input).unwrap();

        let mut second_input = minimal_input(&patient_id);
        second_input.appointment_id = Some(appointment_id);
        let err = create_session(&conn, second_input).unwrap_err();
        assert!(matches!(err, SessionError::AppointmentAlreadyHasSession));
    }

    #[test]
    fn rejects_a_second_session_even_if_the_first_was_archived() {
        let conn = test_conn("create-second-session-first-archived");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let appointment_id = create_test_appointment(&conn, Some(&patient_id));

        let mut input = minimal_input(&patient_id);
        input.appointment_id = Some(appointment_id.clone());
        let first = create_session(&conn, input).unwrap();
        archive_session(&conn, &first.session.id).unwrap();

        let mut second_input = minimal_input(&patient_id);
        second_input.appointment_id = Some(appointment_id);
        let err = create_session(&conn, second_input).unwrap_err();
        assert!(matches!(err, SessionError::AppointmentAlreadyHasSession));
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_appointment() {
        let conn = test_conn("create-nonexistent-appointment");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let mut input = minimal_input(&patient_id);
        input.appointment_id = Some("no-existe".to_string());
        let err = create_session(&conn, input).unwrap_err();
        assert!(matches!(err, SessionError::AppointmentNotFound));
    }

    // ---- listado / archivado ----

    #[test]
    fn archiving_hides_from_active_listing_but_keeps_notes_intact() {
        let conn = test_conn("archive-keeps-notes");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();

        archive_session(&conn, &created.session.id).unwrap();

        assert!(list_sessions(&conn, &patient_id).unwrap().is_empty());
        assert_eq!(list_archived_sessions(&conn, &patient_id).unwrap().len(), 1);
        // La nota sigue ahí, consultable, sin cambios.
        let note = get_current_note(&conn, &created.session.id).unwrap();
        assert_eq!(note.id, created.note.id);
    }

    #[test]
    fn restoring_brings_it_back_to_the_active_listing() {
        let conn = test_conn("restore-session");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();
        archive_session(&conn, &created.session.id).unwrap();

        let restored = restore_session(&conn, &created.session.id).unwrap();
        assert!(restored.deleted_at.is_none());
        assert_eq!(list_sessions(&conn, &patient_id).unwrap().len(), 1);
    }

    #[test]
    fn archiving_a_nonexistent_session_reports_not_found() {
        let conn = test_conn("archive-not-found");
        let err = archive_session(&conn, "no-existe").unwrap_err();
        assert!(matches!(err, SessionError::NotFound));
    }

    #[test]
    fn list_item_reports_current_note_presence_and_lock_state() {
        let conn = test_conn("list-item-note-flags");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();

        let items = list_sessions(&conn, &patient_id).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].has_current_note);
        assert!(!items[0].current_note_is_locked);

        autosave_note_draft(&conn, &created.session.id, Some("contenido".to_string()), None, None, None).unwrap();
        close_current_note(&conn, &created.session.id).unwrap();

        let items = list_sessions(&conn, &patient_id).unwrap();
        assert!(items[0].current_note_is_locked);
    }

    // ---- metadata administrativa ----

    #[test]
    fn updating_metadata_never_touches_the_note() {
        let conn = test_conn("update-metadata-no-note-touch");
        let patient_id = create_test_patient(&conn, "Paciente Catorce");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();

        update_session_metadata(
            &conn,
            &created.session.id,
            SessionMetadataInput {
                session_date: "2026-09-05".to_string(),
                start_time: Some("10:00".to_string()),
                duration_minutes: Some(45),
                modality: Some("online".to_string()),
                status: "realizada".to_string(),
            },
        )
        .unwrap();

        let note = get_current_note(&conn, &created.session.id).unwrap();
        assert_eq!(note.id, created.note.id, "actualizar metadata no debe crear ni tocar ninguna versión de la nota");
        assert_eq!(note.version, 1);
    }

    #[test]
    fn rejects_invalid_status_on_metadata_update() {
        let conn = test_conn("update-metadata-invalid-status");
        let patient_id = create_test_patient(&conn, "Paciente Quince");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();

        let err = update_session_metadata(
            &conn,
            &created.session.id,
            SessionMetadataInput {
                session_date: "2026-09-05".to_string(),
                start_time: None,
                duration_minutes: None,
                modality: None,
                status: "inventado".to_string(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, SessionError::Validation(SessionValidationError::Status(_))));
    }

    // ---- versionado append-only: el corazón de la fase ----

    #[test]
    fn autosave_writes_to_the_draft_and_never_locks_it() {
        let conn = test_conn("autosave-draft");
        let patient_id = create_test_patient(&conn, "Paciente Dieciséis");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();

        autosave_note_draft(&conn, &created.session.id, Some("primer borrador".to_string()), None, None, None).unwrap();
        let note = get_current_note(&conn, &created.session.id).unwrap();
        assert_eq!(note.content.as_deref(), Some("primer borrador"));
        assert!(!note.is_locked);
    }

    #[test]
    fn closing_an_empty_note_is_rejected_and_changes_nothing() {
        let conn = test_conn("close-empty-rejected");
        let patient_id = create_test_patient(&conn, "Paciente Diecisiete");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();

        let err = close_current_note(&conn, &created.session.id).unwrap_err();
        assert!(matches!(err, SessionError::EmptyNoteContent));

        let note = get_current_note(&conn, &created.session.id).unwrap();
        assert!(!note.is_locked);
        assert!(note.closed_at.is_none());
    }

    #[test]
    fn closing_a_note_with_only_whitespace_is_rejected() {
        let conn = test_conn("close-whitespace-rejected");
        let patient_id = create_test_patient(&conn, "Paciente Dieciocho");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();
        autosave_note_draft(&conn, &created.session.id, Some("   \n  ".to_string()), None, None, None).unwrap();

        let err = close_current_note(&conn, &created.session.id).unwrap_err();
        assert!(matches!(err, SessionError::EmptyNoteContent));
    }

    #[test]
    fn closing_a_note_with_content_in_any_single_field_succeeds() {
        let conn = test_conn("close-any-field-counts");
        let patient_id = create_test_patient(&conn, "Paciente Diecinueve");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();
        // Solo `homework_tasks` tiene contenido — igual debe alcanzar.
        autosave_note_draft(&conn, &created.session.id, None, None, Some("tarea para la casa".to_string()), None).unwrap();

        let closed = close_current_note(&conn, &created.session.id).unwrap();
        assert!(closed.is_locked);
        assert!(closed.closed_at.is_some());
    }

    #[test]
    fn closing_an_already_closed_note_is_idempotent() {
        let conn = test_conn("close-idempotent");
        let patient_id = create_test_patient(&conn, "Paciente Veinte");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();
        autosave_note_draft(&conn, &created.session.id, Some("contenido".to_string()), None, None, None).unwrap();
        let first_close = close_current_note(&conn, &created.session.id).unwrap();

        let second_close = close_current_note(&conn, &created.session.id).unwrap();
        assert_eq!(second_close.id, first_close.id);
        assert_eq!(second_close.closed_at, first_close.closed_at, "cerrar una nota ya cerrada no debe volver a tocar closed_at");
        assert_eq!(second_close.version, 1, "no debe crear una versión nueva");
    }

    #[test]
    fn autosaving_a_locked_note_is_rejected() {
        let conn = test_conn("autosave-locked-rejected");
        let patient_id = create_test_patient(&conn, "Paciente Veintiuno");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();
        autosave_note_draft(&conn, &created.session.id, Some("contenido".to_string()), None, None, None).unwrap();
        close_current_note(&conn, &created.session.id).unwrap();

        let err = autosave_note_draft(&conn, &created.session.id, Some("intento".to_string()), None, None, None).unwrap_err();
        assert!(matches!(err, SessionError::NoteIsLocked));

        // Y el contenido de la nota cerrada sigue intacto.
        let note = get_current_note(&conn, &created.session.id).unwrap();
        assert_eq!(note.content.as_deref(), Some("contenido"));
    }

    #[test]
    fn editing_a_closed_note_creates_a_new_version_and_leaves_the_previous_one_intact() {
        let conn = test_conn("edit-creates-v2");
        let patient_id = create_test_patient(&conn, "Paciente Veintidós");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();
        autosave_note_draft(&conn, &created.session.id, Some("versión uno".to_string()), None, None, None).unwrap();
        close_current_note(&conn, &created.session.id).unwrap();

        let v2 = create_new_note_version(&conn, &created.session.id).unwrap();
        assert_eq!(v2.version, 2);
        assert!(!v2.is_locked);
        assert!(v2.is_current);
        assert_eq!(v2.content.as_deref(), Some("versión uno"), "la versión nueva parte precargada con el contenido de la anterior");

        // La versión 1 permanece intacta, ya no vigente.
        let history = list_note_history(&conn, &created.session.id).unwrap();
        assert_eq!(history.len(), 2);
        let v1 = history.iter().find(|n| n.version == 1).unwrap();
        assert!(v1.is_locked);
        assert!(!v1.is_current);
        assert!(v1.superseded_at.is_some());
        assert_eq!(v1.content.as_deref(), Some("versión uno"));
    }

    #[test]
    fn requesting_a_new_version_while_the_current_one_is_still_a_draft_changes_nothing() {
        let conn = test_conn("new-version-noop-on-draft");
        let patient_id = create_test_patient(&conn, "Paciente Veintitrés");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();

        let result = create_new_note_version(&conn, &created.session.id).unwrap();
        assert_eq!(result.id, created.note.id);
        assert_eq!(list_note_history(&conn, &created.session.id).unwrap().len(), 1);
    }

    /// El escenario completo que pide la aprobación de Fase 4: al menos
    /// tres versiones consecutivas, cada una cerrada antes de que exista la
    /// siguiente, con el historial completo intacto y correctamente
    /// ordenado al final.
    #[test]
    fn three_consecutive_versions_are_all_preserved_correctly() {
        let conn = test_conn("three-versions");
        let patient_id = create_test_patient(&conn, "Paciente Veinticuatro");
        let created = create_session(&conn, minimal_input(&patient_id)).unwrap();

        autosave_note_draft(&conn, &created.session.id, Some("contenido v1".to_string()), None, None, None).unwrap();
        close_current_note(&conn, &created.session.id).unwrap();

        create_new_note_version(&conn, &created.session.id).unwrap();
        autosave_note_draft(&conn, &created.session.id, Some("contenido v2".to_string()), None, None, None).unwrap();
        close_current_note(&conn, &created.session.id).unwrap();

        create_new_note_version(&conn, &created.session.id).unwrap();
        autosave_note_draft(&conn, &created.session.id, Some("contenido v3".to_string()), None, None, None).unwrap();
        let v3 = close_current_note(&conn, &created.session.id).unwrap();

        assert_eq!(v3.version, 3);
        assert!(v3.is_locked);
        assert!(v3.is_current);

        let history = list_note_history(&conn, &created.session.id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].version, 3);
        assert_eq!(history[1].version, 2);
        assert_eq!(history[2].version, 1);
        // Exactamente una vigente.
        assert_eq!(history.iter().filter(|n| n.is_current).count(), 1);
        assert_eq!(history[2].content.as_deref(), Some("contenido v1"));
        assert_eq!(history[1].content.as_deref(), Some("contenido v2"));
        assert_eq!(history[0].content.as_deref(), Some("contenido v3"));
        // Las versiones 1 y 2 quedaron cerradas y ya no vigentes.
        assert!(history[1].is_locked && !history[1].is_current);
        assert!(history[2].is_locked && !history[2].is_current);
    }
}
