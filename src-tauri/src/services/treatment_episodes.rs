//! Reglas de negocio de los procesos terapéuticos (Fase 9). Ver
//! `docs/treatment-episodes.md` para el diseño completo, resuelto en la
//! auditoría "post Fase 8 — Episodios y Cierre/Alta".
//!
//! Deliberadamente pequeño: solo crear/consultar/pausar/reactivar/archivar
//! un proceso. **`'cerrado'` existe en el `CHECK` de `SCHEMA_V4` pero esta
//! capa nunca lo permite como destino de `set_status`** — el cierre
//! estructurado (motivo, resumen, objetivos alcanzados) es Fase 10,
//! todavía sin construir. Intentar cerrar un proceso desde este servicio
//! se rechaza explícitamente, no silenciosamente.
//!
//! Regla de integridad no negociable, reutilizada por `services::sessions`
//! y `services::goals`: un proceso solo puede asignarse a una sesión u
//! objetivo si pertenece al mismo paciente, no está archivado, y su estado
//! es `activo` o `pausado` (nunca `cerrado`) — `check_episode_assignable`.
//!
//! Esta capa nunca sabe nada de Tauri, del estado de bloqueo del vault, ni
//! toca Google Calendar en ningún punto.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::patients;
use crate::repositories::treatment_episodes::{self, NewTreatmentEpisodeRow, TreatmentEpisode};

/// Estados a los que `set_status` puede transicionar en esta fase — nunca
/// `'cerrado'`, ver doc de módulo. `'cerrado'` sigue siendo un valor legal
/// del `CHECK` de `SCHEMA_V4` (Fase 10 lo necesitará), pero no es un
/// destino alcanzable desde este servicio todavía.
const ASSIGNABLE_TO_UI: &[&str] = &["activo", "pausado"];
const DEFAULT_STATUS: &str = "activo";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreatmentEpisodeInput {
    pub patient_id: String,
    /// Opcional — si no se envía, se usa la fecha de hoy (UTC, formato
    /// AAAA-MM-DD, mismo criterio que el resto del proyecto para "hoy" en
    /// SQL nativo).
    pub started_at: Option<String>,
}

#[derive(Debug)]
pub enum TreatmentEpisodeValidationError {
    DateFormat,
    Status(String),
}

impl fmt::Display for TreatmentEpisodeValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreatmentEpisodeValidationError::DateFormat => write!(f, "fecha inválida (formato esperado: AAAA-MM-DD)"),
            TreatmentEpisodeValidationError::Status(s) => {
                write!(f, "estado inválido: '{s}' (debe ser uno de: {})", ASSIGNABLE_TO_UI.join(", "))
            }
        }
    }
}
impl std::error::Error for TreatmentEpisodeValidationError {}

#[derive(Debug)]
pub enum TreatmentEpisodeError {
    Validation(TreatmentEpisodeValidationError),
    NotFound,
    PatientNotFound,
    PatientArchived,
    AnotherEpisodeActive,
    ClosureNotImplemented,
    EpisodeArchived,
    EpisodeNotAssignable,
    EpisodePatientMismatch,
    Database(rusqlite::Error),
}

impl fmt::Display for TreatmentEpisodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreatmentEpisodeError::Validation(e) => write!(f, "{e}"),
            TreatmentEpisodeError::NotFound => write!(f, "proceso terapéutico no encontrado"),
            TreatmentEpisodeError::PatientNotFound => write!(f, "paciente no encontrado"),
            TreatmentEpisodeError::PatientArchived => write!(f, "no se pueden iniciar procesos nuevos para un paciente archivado"),
            TreatmentEpisodeError::AnotherEpisodeActive => {
                write!(f, "este paciente ya tiene un proceso activo — solo puede haber uno a la vez")
            }
            TreatmentEpisodeError::ClosureNotImplemented => {
                write!(f, "el cierre estructurado de un proceso todavía no está implementado")
            }
            TreatmentEpisodeError::EpisodeArchived => write!(f, "este proceso está archivado y no puede recibir asignaciones nuevas"),
            TreatmentEpisodeError::EpisodeNotAssignable => {
                write!(f, "este proceso está cerrado y no puede recibir asignaciones nuevas")
            }
            TreatmentEpisodeError::EpisodePatientMismatch => {
                write!(f, "el proceso indicado no pertenece a este paciente")
            }
            TreatmentEpisodeError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for TreatmentEpisodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TreatmentEpisodeError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for TreatmentEpisodeError {
    fn from(e: rusqlite::Error) -> Self {
        TreatmentEpisodeError::Database(e)
    }
}
impl From<TreatmentEpisodeValidationError> for TreatmentEpisodeError {
    fn from(e: TreatmentEpisodeValidationError) -> Self {
        TreatmentEpisodeError::Validation(e)
    }
}

/// Mismo criterio estructural (no calendárico) que
/// `services::sessions::validate_date_format`.
fn validate_date_format(value: &str) -> Result<(), TreatmentEpisodeValidationError> {
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
        Err(TreatmentEpisodeValidationError::DateFormat)
    }
}

fn today_utc_date() -> String {
    // Mismo criterio UTC ya aceptado en el resto del proyecto para "hoy" en
    // ausencia de una librería de fechas (decisión de Fase 1.5, reutilizada
    // en Fase 7/8) — usar el `strftime` nativo de SQLite en vez de
    // calcularlo aquí evitaría una dependencia nueva, pero como este valor
    // se necesita en Rust antes de tocar la base (para poder validarlo con
    // el mismo validador que una fecha explícita), se deriva de
    // `SystemTime`, no de una librería de fechas.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = secs / 86_400;
    // Algoritmo de calendario civil (Howard Hinnant, dominio público) — sin
    // dependencias nuevas, sin lógica de husos horarios (UTC puro).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn require_existing_patient(conn: &Connection, patient_id: &str) -> Result<crate::repositories::patients::Patient, TreatmentEpisodeError> {
    patients::find_by_id(conn, patient_id)?.ok_or(TreatmentEpisodeError::PatientNotFound)
}

/// Crea un proceso terapéutico nuevo. Rechaza: paciente inexistente,
/// paciente archivado, o si el paciente ya tiene un proceso activo (regla
/// de producto no negociable — un solo proceso activo a la vez). Siempre
/// parte en `'activo'`.
pub fn create_episode(conn: &Connection, input: TreatmentEpisodeInput) -> Result<TreatmentEpisode, TreatmentEpisodeError> {
    let patient = require_existing_patient(conn, &input.patient_id)?;
    if patient.deleted_at.is_some() {
        return Err(TreatmentEpisodeError::PatientArchived);
    }
    if treatment_episodes::find_active_by_patient(conn, &input.patient_id)?.is_some() {
        return Err(TreatmentEpisodeError::AnotherEpisodeActive);
    }

    let started_at = match input.started_at {
        Some(d) if !d.trim().is_empty() => {
            validate_date_format(&d)?;
            d
        }
        _ => today_utc_date(),
    };

    let id = uuid::Uuid::new_v4().to_string();
    Ok(treatment_episodes::insert(conn, &NewTreatmentEpisodeRow { id: &id, patient_id: &input.patient_id, started_at: &started_at, status: DEFAULT_STATUS })?)
}

pub fn get_episode(conn: &Connection, id: &str) -> Result<TreatmentEpisode, TreatmentEpisodeError> {
    treatment_episodes::find_by_id(conn, id)?.ok_or(TreatmentEpisodeError::NotFound)
}

pub fn list_episodes(conn: &Connection, patient_id: &str) -> Result<Vec<TreatmentEpisode>, TreatmentEpisodeError> {
    Ok(treatment_episodes::list_active_by_patient(conn, patient_id)?)
}

pub fn list_archived_episodes(conn: &Connection, patient_id: &str) -> Result<Vec<TreatmentEpisode>, TreatmentEpisodeError> {
    Ok(treatment_episodes::list_archived_by_patient(conn, patient_id)?)
}

/// Transiciones permitidas en esta fase: `activo` ↔ `pausado`, únicamente.
/// `'cerrado'` existe en el esquema (preparación para Fase 10) pero se
/// rechaza explícitamente aquí — no hay flujo de cierre estructurado
/// todavía, y permitirlo dejaría un proceso "cerrado" sin ningún registro
/// de motivo/resumen. Reactivar (`pausado` → `activo`) vuelve a exigir que
/// no exista otro proceso activo del mismo paciente.
pub fn set_episode_status(conn: &Connection, id: &str, new_status: &str) -> Result<TreatmentEpisode, TreatmentEpisodeError> {
    if !ASSIGNABLE_TO_UI.contains(&new_status) {
        if new_status == "cerrado" {
            return Err(TreatmentEpisodeError::ClosureNotImplemented);
        }
        return Err(TreatmentEpisodeValidationError::Status(new_status.to_string()).into());
    }

    let episode = treatment_episodes::find_by_id(conn, id)?.ok_or(TreatmentEpisodeError::NotFound)?;
    if episode.deleted_at.is_some() {
        return Err(TreatmentEpisodeError::NotFound);
    }
    if episode.status == "cerrado" {
        return Err(TreatmentEpisodeError::ClosureNotImplemented);
    }

    if new_status == "activo" {
        if let Some(other) = treatment_episodes::find_active_by_patient(conn, &episode.patient_id)? {
            if other.id != episode.id {
                return Err(TreatmentEpisodeError::AnotherEpisodeActive);
            }
        }
    }

    treatment_episodes::set_status(conn, id, new_status)?.ok_or(TreatmentEpisodeError::NotFound)
}

pub fn archive_episode(conn: &Connection, id: &str) -> Result<(), TreatmentEpisodeError> {
    if treatment_episodes::soft_delete(conn, id)? {
        Ok(())
    } else {
        Err(TreatmentEpisodeError::NotFound)
    }
}

pub fn restore_episode(conn: &Connection, id: &str) -> Result<TreatmentEpisode, TreatmentEpisodeError> {
    if treatment_episodes::restore(conn, id)? {
        get_episode(conn, id)
    } else {
        Err(TreatmentEpisodeError::NotFound)
    }
}

/// Regla de integridad reutilizada por `services::sessions::create_session`
/// y `services::goals::create_goal` al asignar un `episode_id` opcional:
/// el proceso debe existir, pertenecer al mismo paciente, no estar
/// archivado, y no estar `cerrado` — política explícita (§8 de la
/// aprobación de Fase 9): un proceso cerrado o archivado no recibe
/// asignaciones nuevas. `Ok(())` si `episode_id` es `None` (una sesión u
/// objetivo sin proceso sigue siendo válida, sin cambios).
pub fn check_episode_assignable(conn: &Connection, episode_id: &Option<String>, patient_id: &str) -> Result<(), TreatmentEpisodeError> {
    let Some(episode_id) = episode_id else { return Ok(()) };
    let episode = treatment_episodes::find_by_id(conn, episode_id)?.ok_or(TreatmentEpisodeError::NotFound)?;
    if episode.patient_id != patient_id {
        return Err(TreatmentEpisodeError::EpisodePatientMismatch);
    }
    if episode.deleted_at.is_some() {
        return Err(TreatmentEpisodeError::EpisodeArchived);
    }
    if episode.status == "cerrado" {
        return Err(TreatmentEpisodeError::EpisodeNotAssignable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-episodes-svc-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x43u8; VAULT_KEY_LEN]);
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

    fn archive_test_patient(conn: &Connection, patient_id: &str) {
        crate::services::patients::archive_patient(conn, patient_id).unwrap();
    }

    #[test]
    fn creates_an_episode_with_explicit_started_at() {
        let conn = test_conn("create-explicit-date");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: Some("2026-03-01".into()) }).unwrap();
        assert_eq!(episode.patient_id, patient_id);
        assert_eq!(episode.started_at, "2026-03-01");
        assert_eq!(episode.status, "activo");
    }

    #[test]
    fn creates_an_episode_defaulting_started_at_to_today() {
        let conn = test_conn("create-default-date");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: None }).unwrap();
        assert_eq!(episode.started_at.len(), 10);
        assert_eq!(episode.started_at.as_bytes()[4], b'-');
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let conn = test_conn("create-no-patient");
        let err = create_episode(&conn, TreatmentEpisodeInput { patient_id: "no-existe".into(), started_at: None }).unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let conn = test_conn("create-archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        archive_test_patient(&conn, &patient_id);
        let err = create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: None }).unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::PatientArchived));
    }

    #[test]
    fn rejects_invalid_started_at_format() {
        let conn = test_conn("create-bad-date");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let err = create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: Some("01-03-2026".into()) }).unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::Validation(TreatmentEpisodeValidationError::DateFormat)));
    }

    #[test]
    fn rejects_a_second_active_episode_for_the_same_patient() {
        let conn = test_conn("create-second-active");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        create_episode(&conn, TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        let err = create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: None }).unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::AnotherEpisodeActive));
    }

    #[test]
    fn allows_a_new_active_episode_once_the_previous_one_is_paused() {
        let conn = test_conn("create-after-pause");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let first = create_episode(&conn, TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        set_episode_status(&conn, &first.id, "pausado").unwrap();
        let second = create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: None }).unwrap();
        assert_eq!(second.status, "activo");
    }

    #[test]
    fn pauses_and_reactivates_an_episode() {
        let conn = test_conn("pause-reactivate");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: None }).unwrap();
        let paused = set_episode_status(&conn, &episode.id, "pausado").unwrap();
        assert_eq!(paused.status, "pausado");
        let reactivated = set_episode_status(&conn, &episode.id, "activo").unwrap();
        assert_eq!(reactivated.status, "activo");
    }

    #[test]
    fn reactivating_is_rejected_if_another_episode_became_active_meanwhile() {
        let conn = test_conn("reactivate-conflict");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let first = create_episode(&conn, TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        set_episode_status(&conn, &first.id, "pausado").unwrap();
        create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: None }).unwrap();
        let err = set_episode_status(&conn, &first.id, "activo").unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::AnotherEpisodeActive));
    }

    #[test]
    fn setting_status_to_cerrado_is_rejected_in_this_phase() {
        let conn = test_conn("close-rejected");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: None }).unwrap();
        let err = set_episode_status(&conn, &episode.id, "cerrado").unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::ClosureNotImplemented));
    }

    #[test]
    fn setting_status_to_an_invalid_value_is_rejected() {
        let conn = test_conn("invalid-status");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: None }).unwrap();
        let err = set_episode_status(&conn, &episode.id, "inventado").unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::Validation(_)));
    }

    #[test]
    fn set_status_on_nonexistent_episode_is_rejected() {
        let conn = test_conn("set-status-not-found");
        let err = set_episode_status(&conn, "no-existe", "pausado").unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::NotFound));
    }

    #[test]
    fn archive_and_restore_roundtrip() {
        let conn = test_conn("archive-restore");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id, started_at: None }).unwrap();
        archive_episode(&conn, &episode.id).unwrap();
        assert!(treatment_episodes::find_by_id(&conn, &episode.id).unwrap().unwrap().deleted_at.is_some());
        let restored = restore_episode(&conn, &episode.id).unwrap();
        assert!(restored.deleted_at.is_none());
    }

    #[test]
    fn check_episode_assignable_accepts_none() {
        let conn = test_conn("assignable-none");
        assert!(check_episode_assignable(&conn, &None, "any-patient").is_ok());
    }

    #[test]
    fn check_episode_assignable_accepts_a_matching_active_episode() {
        let conn = test_conn("assignable-match-active");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        assert!(check_episode_assignable(&conn, &Some(episode.id), &patient_id).is_ok());
    }

    #[test]
    fn check_episode_assignable_accepts_a_matching_paused_episode() {
        let conn = test_conn("assignable-match-paused");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        set_episode_status(&conn, &episode.id, "pausado").unwrap();
        assert!(check_episode_assignable(&conn, &Some(episode.id), &patient_id).is_ok());
    }

    #[test]
    fn check_episode_assignable_rejects_a_nonexistent_episode() {
        let conn = test_conn("assignable-not-found");
        let err = check_episode_assignable(&conn, &Some("no-existe".into()), "any-patient").unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::NotFound));
    }

    #[test]
    fn check_episode_assignable_rejects_an_episode_of_a_different_patient() {
        let conn = test_conn("assignable-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id: patient_a, started_at: None }).unwrap();
        let err = check_episode_assignable(&conn, &Some(episode.id), &patient_b).unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::EpisodePatientMismatch));
    }

    #[test]
    fn check_episode_assignable_rejects_an_archived_episode() {
        let conn = test_conn("assignable-archived");
        let patient_id = create_test_patient(&conn, "Paciente Catorce");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        archive_episode(&conn, &episode.id).unwrap();
        let err = check_episode_assignable(&conn, &Some(episode.id), &patient_id).unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::EpisodeArchived));
    }

    #[test]
    fn check_episode_assignable_rejects_a_closed_episode() {
        let conn = test_conn("assignable-closed");
        let patient_id = create_test_patient(&conn, "Paciente Quince");
        let episode = create_episode(&conn, TreatmentEpisodeInput { patient_id: patient_id.clone(), started_at: None }).unwrap();
        // No hay flujo de cierre en esta fase — se simula directamente en el
        // repositorio, igual que la migración legacy lo hace para pacientes
        // con `status = 'alta'`.
        treatment_episodes::set_status(&conn, &episode.id, "cerrado").unwrap();
        let err = check_episode_assignable(&conn, &Some(episode.id), &patient_id).unwrap_err();
        assert!(matches!(err, TreatmentEpisodeError::EpisodeNotAssignable));
    }

    #[test]
    fn legacy_episodes_are_visible_through_normal_listing() {
        // No crea nada nuevo — solo confirma que, tras una migración con
        // datos legacy, list_episodes los expone con normalidad a través
        // de la capa de servicio (no solo del repositorio).
        let conn = test_conn("legacy-visible");
        let patient_id = create_test_patient(&conn, "Paciente Legacy");
        conn.execute("INSERT INTO sessions (id, patient_id, session_date) VALUES ('s1', ?1, '2025-01-01')", [&patient_id]).unwrap();
        conn.execute(
            "INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES ('legacy-x', ?1, '2025-01-01', 'activo')",
            [&patient_id],
        )
        .unwrap();
        let episodes = list_episodes(&conn, &patient_id).unwrap();
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].id, "legacy-x");
    }
}
