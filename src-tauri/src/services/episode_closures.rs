//! Reglas de negocio del cierre estructurado de un proceso terapéutico
//! (Fase 11). Ver `docs/episode-closure.md` para el diseño completo,
//! resuelto en la auditoría "Fase 11 — Cierre/Alta estructurado".
//!
//! `close_episode` es la única forma de alcanzar `treatment_episodes.status
//! = 'cerrado'` — reemplaza el rechazo incondicional que
//! `services::treatment_episodes::set_episode_status` mantiene para ese
//! valor (`ClosureNotImplemented`, sin cambios en esta fase: sigue siendo
//! la puerta correcta para rechazar un intento de "cerrar" genérico sin
//! pasar por este flujo).
//!
//! El cierre es **inmutable tras crearse** (decisión explícita de la
//! aprobación de Fase 11): no existe ninguna función de edición de
//! contenido. Corregir un error de fondo es `revert_closure` (anula,
//! preservando el original como historia) seguido de un `close_episode`
//! nuevo — nunca un `UPDATE` sobre un cierre existente.
//!
//! Esta capa nunca sabe nada de Tauri, del estado de bloqueo del vault, ni
//! toca Google Calendar en ningún punto — ninguna de las funciones de este
//! archivo se referencia jamás desde `calendar::*`.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::episode_closures::{self, EpisodeClosure, NewEpisodeClosureRow};
use crate::repositories::sessions::{self, SessionMetadataUpdateRow};
use crate::repositories::treatment_episodes::{self, TreatmentEpisode};

/// Taxonomía de motivo de cierre — fijada en el `CHECK` de `SCHEMA_V5`,
/// aprobada explícitamente en el Bloque C de Fase 11 (propuesta de 6
/// categorías). Cambiarla exige otra migración.
pub const VALID_REASONS: &[&str] = &["alta", "cierre_acordado", "interrupcion", "derivacion", "decision_profesional", "otro"];
/// Taxonomía de resultado — independiente de `reason` (un cierre por
/// derivación puede coexistir con objetivos parcialmente logrados).
pub const VALID_OUTCOMES: &[&str] = &["objetivos_logrados", "parcialmente_logrados", "no_logrados", "no_evaluable"];
/// Estados válidos a los que puede volver un proceso al anular su cierre —
/// decisión explícita del Bloque C: siempre se pregunta, nunca se asume.
pub const VALID_REOPEN_STATUSES: &[&str] = &["activo", "pausado"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResolutionInput {
    pub session_id: String,
    /// `true` = cancelar esta sesión futura como parte del cierre; `false`
    /// = mantenerla tal cual. Ambos son una resolución explícita — no hay
    /// un tercer valor implícito.
    pub cancel: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseEpisodeInput {
    /// Opcional — si no se envía, se usa la fecha de hoy.
    pub closed_at: Option<String>,
    pub reason: String,
    pub reason_detail: Option<String>,
    pub outcome: String,
    pub summary: Option<String>,
    pub recommendations: Option<String>,
    /// Debe cubrir **exactamente** las sesiones futuras agendadas del
    /// proceso (`sessions::list_upcoming_by_episode`) — ni de más ni de
    /// menos. Resolución manual explícita, exigida por el Bloque C de la
    /// aprobación de Fase 11: nunca se asume silenciosamente "mantener".
    pub session_resolutions: Vec<SessionResolutionInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertClosureInput {
    pub reverted_reason: String,
    /// A qué estado vuelve el proceso — el Bloque C de la aprobación pidió
    /// preguntarlo siempre explícitamente, nunca asumir `'activo'`.
    pub reopen_status: String,
}

#[derive(Debug)]
pub enum EpisodeClosureError {
    EpisodeNotFound,
    EpisodeArchived,
    AlreadyClosed,
    EpisodeNotClosable,
    DateFormat,
    ClosedBeforeStarted,
    InvalidReason(String),
    MissingReasonDetail,
    InvalidOutcome(String),
    /// Falta resolver explícitamente una o más sesiones futuras del
    /// proceso — se listan sus IDs para que la UI sepa cuáles pedir.
    PendingSessionResolutionRequired(Vec<String>),
    /// El llamador incluyó una resolución para una sesión que no es una
    /// sesión futura agendada de este proceso — nunca se ignora en
    /// silencio.
    UnknownSessionInResolution(String),
    ClosureNotFound,
    AlreadyReverted,
    EpisodeNotClosedForRevert,
    InvalidReopenStatus(String),
    AnotherEpisodeActive,
    Database(rusqlite::Error),
}

impl fmt::Display for EpisodeClosureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EpisodeClosureError::EpisodeNotFound => write!(f, "proceso terapéutico no encontrado"),
            EpisodeClosureError::EpisodeArchived => write!(f, "este proceso está archivado y no puede cerrarse"),
            EpisodeClosureError::AlreadyClosed => write!(f, "este proceso ya está cerrado"),
            EpisodeClosureError::EpisodeNotClosable => write!(f, "este proceso no está en un estado que pueda cerrarse"),
            EpisodeClosureError::DateFormat => write!(f, "fecha inválida (formato esperado: AAAA-MM-DD)"),
            EpisodeClosureError::ClosedBeforeStarted => write!(f, "la fecha de término no puede ser anterior a la fecha de inicio del proceso"),
            EpisodeClosureError::InvalidReason(r) => write!(f, "motivo de cierre inválido: '{r}'"),
            EpisodeClosureError::MissingReasonDetail => write!(f, "debes especificar el detalle cuando el motivo es 'otro'"),
            EpisodeClosureError::InvalidOutcome(o) => write!(f, "resultado inválido: '{o}'"),
            EpisodeClosureError::PendingSessionResolutionRequired(ids) => {
                write!(f, "faltan resolver sesiones futuras del proceso: {}", ids.join(", "))
            }
            EpisodeClosureError::UnknownSessionInResolution(id) => {
                write!(f, "la sesión '{id}' no es una sesión futura agendada de este proceso")
            }
            EpisodeClosureError::ClosureNotFound => write!(f, "cierre no encontrado"),
            EpisodeClosureError::AlreadyReverted => write!(f, "este cierre ya fue anulado anteriormente"),
            EpisodeClosureError::EpisodeNotClosedForRevert => write!(f, "este proceso no está cerrado"),
            EpisodeClosureError::InvalidReopenStatus(s) => write!(f, "estado de reapertura inválido: '{s}'"),
            EpisodeClosureError::AnotherEpisodeActive => write!(f, "este paciente ya tiene un proceso activo — solo puede haber uno a la vez"),
            EpisodeClosureError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for EpisodeClosureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EpisodeClosureError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for EpisodeClosureError {
    fn from(e: rusqlite::Error) -> Self {
        EpisodeClosureError::Database(e)
    }
}

/// Mismo criterio estructural (no calendárico) que
/// `services::treatment_episodes::validate_date_format`. Duplicado
/// deliberadamente en vez de exportado desde ahí — es una función privada
/// de ese módulo, y esta validación es lo bastante pequeña como para no
/// justificar cambiar su visibilidad.
fn validate_date_format(value: &str) -> bool {
    let bytes = value.as_bytes();
    let shape_ok = bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-';
    let parse = |s: &str| s.parse::<u32>().ok();
    shape_ok
        && match (parse(&value[0..4]), parse(&value[5..7]), parse(&value[8..10])) {
            (Some(_year), Some(month), Some(day)) => (1..=12).contains(&month) && (1..=31).contains(&day),
            _ => false,
        }
}

fn today_utc_date(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("SELECT strftime('%Y-%m-%d','now')", [], |r| r.get(0))
}

fn require_closable_episode(conn: &Connection, episode_id: &str) -> Result<TreatmentEpisode, EpisodeClosureError> {
    let episode = treatment_episodes::find_by_id(conn, episode_id)?.ok_or(EpisodeClosureError::EpisodeNotFound)?;
    if episode.deleted_at.is_some() {
        return Err(EpisodeClosureError::EpisodeArchived);
    }
    match episode.status.as_str() {
        "cerrado" => Err(EpisodeClosureError::AlreadyClosed),
        "activo" | "pausado" => Ok(episode),
        _ => Err(EpisodeClosureError::EpisodeNotClosable),
    }
}

/// Cierra un proceso terapéutico: valida todo, resuelve explícitamente las
/// sesiones futuras pendientes, y solo entonces escribe — dentro de una
/// única transacción — el cierre y el cambio de estado del proceso.
pub fn close_episode(conn: &Connection, episode_id: &str, input: CloseEpisodeInput) -> Result<(EpisodeClosure, TreatmentEpisode), EpisodeClosureError> {
    let episode = require_closable_episode(conn, episode_id)?;

    let closed_at = match input.closed_at {
        Some(d) if !d.trim().is_empty() => {
            if !validate_date_format(&d) {
                return Err(EpisodeClosureError::DateFormat);
            }
            d
        }
        _ => today_utc_date(conn)?,
    };
    if closed_at.as_str() < episode.started_at.as_str() {
        return Err(EpisodeClosureError::ClosedBeforeStarted);
    }

    let reason = input.reason.trim();
    if !VALID_REASONS.contains(&reason) {
        return Err(EpisodeClosureError::InvalidReason(reason.to_string()));
    }
    let reason_detail = input.reason_detail.filter(|s| !s.trim().is_empty());
    if reason == "otro" && reason_detail.is_none() {
        return Err(EpisodeClosureError::MissingReasonDetail);
    }

    let outcome = input.outcome.trim();
    if !VALID_OUTCOMES.contains(&outcome) {
        return Err(EpisodeClosureError::InvalidOutcome(outcome.to_string()));
    }

    let summary = input.summary.filter(|s| !s.trim().is_empty());
    let recommendations = input.recommendations.filter(|s| !s.trim().is_empty());

    // Resolución manual explícita de sesiones futuras (Bloque C de la
    // aprobación): el conjunto recibido debe coincidir exactamente con las
    // sesiones futuras agendadas del proceso — ni de más ni de menos.
    let upcoming = sessions::list_upcoming_by_episode(conn, episode_id)?;
    let mut expected_ids: std::collections::HashSet<&str> = upcoming.iter().map(|s| s.id.as_str()).collect();
    for resolution in &input.session_resolutions {
        if !expected_ids.remove(resolution.session_id.as_str()) {
            return Err(EpisodeClosureError::UnknownSessionInResolution(resolution.session_id.clone()));
        }
    }
    if !expected_ids.is_empty() {
        let mut missing: Vec<String> = expected_ids.into_iter().map(String::from).collect();
        missing.sort();
        return Err(EpisodeClosureError::PendingSessionResolutionRequired(missing));
    }

    let closure_id = uuid::Uuid::new_v4().to_string();
    let tx = conn.unchecked_transaction()?;

    for resolution in &input.session_resolutions {
        if !resolution.cancel {
            continue;
        }
        let session = sessions::find_by_id(&tx, &resolution.session_id)?.expect("ya validado contra list_upcoming_by_episode");
        sessions::update_metadata(
            &tx,
            &resolution.session_id,
            &SessionMetadataUpdateRow {
                session_date: &session.session_date,
                start_time: session.start_time.as_deref(),
                duration_minutes: session.duration_minutes,
                modality: session.modality.as_deref(),
                status: "cancelada",
            },
        )?;
    }

    let closure = episode_closures::insert(
        &tx,
        &NewEpisodeClosureRow {
            id: &closure_id,
            episode_id,
            closed_at: &closed_at,
            reason,
            reason_detail: reason_detail.as_deref(),
            outcome,
            summary: summary.as_deref(),
            recommendations: recommendations.as_deref(),
        },
    )?;
    let updated_episode = treatment_episodes::set_status(&tx, episode_id, "cerrado")?.expect("el proceso ya se validó como existente arriba");
    tx.commit()?;

    Ok((closure, updated_episode))
}

pub fn get_active_closure(conn: &Connection, episode_id: &str) -> Result<Option<EpisodeClosure>, EpisodeClosureError> {
    treatment_episodes::find_by_id(conn, episode_id)?.ok_or(EpisodeClosureError::EpisodeNotFound)?;
    Ok(episode_closures::find_active_by_episode(conn, episode_id)?)
}

pub fn list_closure_history(conn: &Connection, episode_id: &str) -> Result<Vec<EpisodeClosure>, EpisodeClosureError> {
    treatment_episodes::find_by_id(conn, episode_id)?.ok_or(EpisodeClosureError::EpisodeNotFound)?;
    Ok(episode_closures::list_history_by_episode(conn, episode_id)?)
}

/// Anula un cierre por error: nunca borra ni sobrescribe el registro
/// original (solo marca `reverted_at`/`reverted_reason`), y vuelve a poner
/// el proceso en el estado explícitamente elegido — reutilizando la misma
/// regla `AnotherEpisodeActive` ya existente y probada desde Fase 9 cuando
/// el destino es `'activo'`.
pub fn revert_closure(conn: &Connection, closure_id: &str, input: RevertClosureInput) -> Result<(EpisodeClosure, TreatmentEpisode), EpisodeClosureError> {
    if !VALID_REOPEN_STATUSES.contains(&input.reopen_status.as_str()) {
        return Err(EpisodeClosureError::InvalidReopenStatus(input.reopen_status.clone()));
    }
    let reverted_reason = input.reverted_reason.trim();

    let closure = episode_closures::find_by_id(conn, closure_id)?.ok_or(EpisodeClosureError::ClosureNotFound)?;
    if closure.reverted_at.is_some() {
        return Err(EpisodeClosureError::AlreadyReverted);
    }
    let episode = treatment_episodes::find_by_id(conn, &closure.episode_id)?.ok_or(EpisodeClosureError::EpisodeNotFound)?;
    if episode.status != "cerrado" {
        return Err(EpisodeClosureError::EpisodeNotClosedForRevert);
    }

    if input.reopen_status == "activo" {
        if let Some(other) = treatment_episodes::find_active_by_patient(conn, &episode.patient_id)? {
            if other.id != episode.id {
                return Err(EpisodeClosureError::AnotherEpisodeActive);
            }
        }
    }

    let tx = conn.unchecked_transaction()?;
    let reverted_closure = episode_closures::revert(&tx, closure_id, reverted_reason)?.expect("ya validado como vigente arriba");
    let reopened_episode = treatment_episodes::set_status(&tx, &episode.id, &input.reopen_status)?.expect("el proceso ya se validó como existente arriba");
    tx.commit()?;

    Ok((reverted_closure, reopened_episode))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::sessions::{self as sessions_repo, NewSessionRow};
    use crate::repositories::treatment_episodes::{self as episodes_repo, NewTreatmentEpisodeRow};
    use crate::services::treatment_episodes as episodes_service;

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-closures-svc-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x47u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_patient(conn: &Connection, name: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        patients::insert(
            conn,
            &NewPatientRow {
                id: &id, full_name: name, preferred_name: None, rut: None, birth_date: None, phone: None, email: None,
                address: None, emergency_contact_name: None, emergency_contact_phone: None, emergency_contact_relationship: None,
                status: "activo", referred_by: None, intake_date: None, region: None, commune: None,
            },
        )
        .unwrap();
        id
    }

    fn create_test_episode_with_status(conn: &Connection, patient_id: &str, status: &str) -> String {
        let episode_id = uuid::Uuid::new_v4().to_string();
        episodes_repo::insert(conn, &NewTreatmentEpisodeRow { id: &episode_id, patient_id, started_at: "2026-01-01", status }).unwrap();
        episode_id
    }

    fn minimal_input() -> CloseEpisodeInput {
        CloseEpisodeInput { closed_at: None, reason: "alta".to_string(), reason_detail: None, outcome: "objetivos_logrados".to_string(), summary: None, recommendations: None, session_resolutions: vec![] }
    }

    #[test]
    fn closes_an_active_episode() {
        let conn = test_conn("close-active");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let (closure, episode) = close_episode(&conn, &episode_id, minimal_input()).unwrap();
        assert_eq!(closure.reason, "alta");
        assert_eq!(episode.status, "cerrado");
    }

    #[test]
    fn closes_a_paused_episode() {
        let conn = test_conn("close-paused");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "pausado");
        let (_closure, episode) = close_episode(&conn, &episode_id, minimal_input()).unwrap();
        assert_eq!(episode.status, "cerrado");
    }

    #[test]
    fn rejects_closing_a_nonexistent_episode() {
        let conn = test_conn("close-not-found");
        let err = close_episode(&conn, "no-existe", minimal_input()).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::EpisodeNotFound));
    }

    #[test]
    fn rejects_closing_an_archived_episode() {
        let conn = test_conn("close-archived");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        episodes_service::archive_episode(&conn, &episode_id).unwrap();
        let err = close_episode(&conn, &episode_id, minimal_input()).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::EpisodeArchived));
    }

    #[test]
    fn rejects_closing_an_already_closed_episode() {
        let conn = test_conn("close-already-closed");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        close_episode(&conn, &episode_id, minimal_input()).unwrap();
        let err = close_episode(&conn, &episode_id, minimal_input()).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::AlreadyClosed));
    }

    #[test]
    fn rejects_an_invalid_reason() {
        let conn = test_conn("close-invalid-reason");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let mut input = minimal_input();
        input.reason = "inventado".to_string();
        let err = close_episode(&conn, &episode_id, input).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::InvalidReason(r) if r == "inventado"));
    }

    #[test]
    fn rejects_an_invalid_outcome() {
        let conn = test_conn("close-invalid-outcome");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let mut input = minimal_input();
        input.outcome = "inventado".to_string();
        let err = close_episode(&conn, &episode_id, input).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::InvalidOutcome(o) if o == "inventado"));
    }

    #[test]
    fn reason_otro_requires_a_detail() {
        let conn = test_conn("close-otro-sin-detalle");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let mut input = minimal_input();
        input.reason = "otro".to_string();
        let err = close_episode(&conn, &episode_id, input).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::MissingReasonDetail));
    }

    #[test]
    fn reason_otro_with_a_detail_is_accepted() {
        let conn = test_conn("close-otro-con-detalle");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let mut input = minimal_input();
        input.reason = "otro".to_string();
        input.reason_detail = Some("Circunstancia particular".to_string());
        let (closure, _) = close_episode(&conn, &episode_id, input).unwrap();
        assert_eq!(closure.reason_detail.as_deref(), Some("Circunstancia particular"));
    }

    #[test]
    fn rejects_a_closed_at_before_started_at() {
        let conn = test_conn("close-before-started");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let mut input = minimal_input();
        input.closed_at = Some("2025-01-01".to_string());
        let err = close_episode(&conn, &episode_id, input).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::ClosedBeforeStarted));
    }

    #[test]
    fn rejects_an_invalid_closed_at_format() {
        let conn = test_conn("close-bad-date");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let mut input = minimal_input();
        input.closed_at = Some("01-02-2026".to_string());
        let err = close_episode(&conn, &episode_id, input).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::DateFormat));
    }

    #[test]
    fn rejects_closing_with_an_unresolved_future_session() {
        let conn = test_conn("close-pending-session");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        sessions_repo::insert(&conn, &NewSessionRow { id: "s1", patient_id: &patient_id, appointment_id: None, episode_id: Some(&episode_id), session_date: "2099-01-01", start_time: None, duration_minutes: None, modality: None, status: "programada" }).unwrap();

        let err = close_episode(&conn, &episode_id, minimal_input()).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::PendingSessionResolutionRequired(ids) if ids == vec!["s1".to_string()]));
    }

    #[test]
    fn rejects_an_unknown_session_in_resolution() {
        let conn = test_conn("close-unknown-session");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let mut input = minimal_input();
        input.session_resolutions = vec![SessionResolutionInput { session_id: "no-es-una-sesion-futura".to_string(), cancel: true }];
        let err = close_episode(&conn, &episode_id, input).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::UnknownSessionInResolution(id) if id == "no-es-una-sesion-futura"));
    }

    #[test]
    fn closing_with_cancel_resolution_marks_the_session_cancelled() {
        let conn = test_conn("close-cancel-session");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        sessions_repo::insert(&conn, &NewSessionRow { id: "s1", patient_id: &patient_id, appointment_id: None, episode_id: Some(&episode_id), session_date: "2099-01-01", start_time: None, duration_minutes: None, modality: None, status: "programada" }).unwrap();

        let mut input = minimal_input();
        input.session_resolutions = vec![SessionResolutionInput { session_id: "s1".to_string(), cancel: true }];
        close_episode(&conn, &episode_id, input).unwrap();

        let session = sessions_repo::find_by_id(&conn, "s1").unwrap().unwrap();
        assert_eq!(session.status, "cancelada");
    }

    #[test]
    fn closing_with_keep_resolution_leaves_the_session_untouched() {
        let conn = test_conn("close-keep-session");
        let patient_id = create_test_patient(&conn, "Paciente Catorce");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        sessions_repo::insert(&conn, &NewSessionRow { id: "s1", patient_id: &patient_id, appointment_id: None, episode_id: Some(&episode_id), session_date: "2099-01-01", start_time: None, duration_minutes: None, modality: None, status: "programada" }).unwrap();

        let mut input = minimal_input();
        input.session_resolutions = vec![SessionResolutionInput { session_id: "s1".to_string(), cancel: false }];
        close_episode(&conn, &episode_id, input).unwrap();

        let session = sessions_repo::find_by_id(&conn, "s1").unwrap().unwrap();
        assert_eq!(session.status, "programada", "mantener no debe tocar la sesión");
    }

    #[test]
    fn get_active_closure_returns_none_before_closing() {
        let conn = test_conn("get-active-none");
        let patient_id = create_test_patient(&conn, "Paciente Quince");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        assert!(get_active_closure(&conn, &episode_id).unwrap().is_none());
    }

    #[test]
    fn revert_reopens_to_activo_and_preserves_the_original_closure() {
        let conn = test_conn("revert-to-activo");
        let patient_id = create_test_patient(&conn, "Paciente Dieciseis");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let (closure, _) = close_episode(&conn, &episode_id, minimal_input()).unwrap();

        let (reverted, episode) = revert_closure(&conn, &closure.id, RevertClosureInput { reverted_reason: "Cerrado por error".to_string(), reopen_status: "activo".to_string() }).unwrap();
        assert!(reverted.reverted_at.is_some());
        assert_eq!(reverted.reason, "alta", "el contenido original del cierre nunca se pierde");
        assert_eq!(episode.status, "activo");
        assert!(get_active_closure(&conn, &episode_id).unwrap().is_none());
    }

    #[test]
    fn revert_reopens_to_pausado_when_explicitly_requested() {
        let conn = test_conn("revert-to-pausado");
        let patient_id = create_test_patient(&conn, "Paciente Diecisiete");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let (closure, _) = close_episode(&conn, &episode_id, minimal_input()).unwrap();

        let (_reverted, episode) = revert_closure(&conn, &closure.id, RevertClosureInput { reverted_reason: "Cerrado por error".to_string(), reopen_status: "pausado".to_string() }).unwrap();
        assert_eq!(episode.status, "pausado");
    }

    #[test]
    fn rejects_an_invalid_reopen_status() {
        let conn = test_conn("revert-invalid-status");
        let patient_id = create_test_patient(&conn, "Paciente Dieciocho");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let (closure, _) = close_episode(&conn, &episode_id, minimal_input()).unwrap();
        let err = revert_closure(&conn, &closure.id, RevertClosureInput { reverted_reason: "Motivo".to_string(), reopen_status: "inventado".to_string() }).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::InvalidReopenStatus(s) if s == "inventado"));
    }

    #[test]
    fn rejects_reverting_a_closure_that_does_not_exist() {
        let conn = test_conn("revert-not-found");
        let err = revert_closure(&conn, "no-existe", RevertClosureInput { reverted_reason: "Motivo".to_string(), reopen_status: "activo".to_string() }).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::ClosureNotFound));
    }

    #[test]
    fn rejects_reverting_an_already_reverted_closure() {
        let conn = test_conn("revert-twice");
        let patient_id = create_test_patient(&conn, "Paciente Diecinueve");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let (closure, _) = close_episode(&conn, &episode_id, minimal_input()).unwrap();
        revert_closure(&conn, &closure.id, RevertClosureInput { reverted_reason: "Primero".to_string(), reopen_status: "activo".to_string() }).unwrap();
        let err = revert_closure(&conn, &closure.id, RevertClosureInput { reverted_reason: "Segundo".to_string(), reopen_status: "activo".to_string() }).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::AlreadyReverted));
    }

    /// El caso crítico explícito de la aprobación: Proceso A se cierra,
    /// Proceso B se crea y queda activo, luego se intenta reabrir A. Nunca
    /// puede resultar en dos procesos activos del mismo paciente.
    #[test]
    fn reverting_to_activo_is_rejected_if_another_episode_became_active_meanwhile() {
        let conn = test_conn("revert-conflict");
        let patient_id = create_test_patient(&conn, "Paciente Veinte");
        let episode_a = create_test_episode_with_status(&conn, &patient_id, "activo");
        let (closure_a, _) = close_episode(&conn, &episode_a, minimal_input()).unwrap();

        // Con A cerrado, el reingreso ya documentado (Fase 9) funciona sin
        // ningún cambio de código: crear B queda permitido de inmediato.
        let episode_b = episodes_service::create_episode(&conn, episodes_service::TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        assert_eq!(episode_b.status, "activo");

        let err = revert_closure(&conn, &closure_a.id, RevertClosureInput { reverted_reason: "Cerrado por error".to_string(), reopen_status: "activo".to_string() }).unwrap_err();
        assert!(matches!(err, EpisodeClosureError::AnotherEpisodeActive));

        // Pero reabrir A como 'pausado' sí es válido, incluso con B activo.
        let (_reverted, episode_a_reopened) =
            revert_closure(&conn, &closure_a.id, RevertClosureInput { reverted_reason: "Cerrado por error".to_string(), reopen_status: "pausado".to_string() }).unwrap();
        assert_eq!(episode_a_reopened.status, "pausado");
    }

    #[test]
    fn after_revert_the_episode_can_be_closed_again_with_a_new_closure() {
        let conn = test_conn("revert-then-close-again");
        let patient_id = create_test_patient(&conn, "Paciente Veintiuno");
        let episode_id = create_test_episode_with_status(&conn, &patient_id, "activo");
        let (closure_1, _) = close_episode(&conn, &episode_id, minimal_input()).unwrap();
        revert_closure(&conn, &closure_1.id, RevertClosureInput { reverted_reason: "Cerrado por error".to_string(), reopen_status: "activo".to_string() }).unwrap();

        let mut second_input = minimal_input();
        second_input.reason = "derivacion".to_string();
        let (closure_2, episode) = close_episode(&conn, &episode_id, second_input).unwrap();
        assert_eq!(episode.status, "cerrado");
        assert_ne!(closure_2.id, closure_1.id);

        let history = list_closure_history(&conn, &episode_id).unwrap();
        assert_eq!(history.len(), 2, "el cierre anulado permanece en el historial junto al nuevo");
    }

    #[test]
    fn legacy_episode_can_be_closed() {
        // El proceso legacy (Fase 9) no tiene nada especial que le impida
        // cerrarse — mismo camino de código que cualquier otro proceso.
        let conn = test_conn("close-legacy");
        let patient_id = create_test_patient(&conn, "Paciente Legacy");
        let episode_id = format!("legacy-{patient_id}");
        episodes_repo::insert(&conn, &NewTreatmentEpisodeRow { id: &episode_id, patient_id: &patient_id, started_at: "2020-01-01", status: "activo" }).unwrap();

        let (_closure, episode) = close_episode(&conn, &episode_id, minimal_input()).unwrap();
        assert_eq!(episode.status, "cerrado");
    }
}
