//! Reglas de negocio del Plan de Seguridad Clínico Versionado (Fase 12).
//! Ver `docs/safety-plan.md` para el diseño completo.
//!
//! Esta es una **herramienta de documentación clínica**, nunca un algoritmo
//! de predicción de riesgo, un sistema de emergencia ni una herramienta de
//! vigilancia: esta capa nunca infiere, puntúa, colorea, prioriza ni
//! notifica nada automáticamente a partir de su contenido. Deliberadamente
//! sin ninguna relación con `services::patient_clinical_profile` ni con
//! `risk_flags` — ninguna función de este archivo lee ni escribe esa tabla.
//!
//! Un plan de seguridad es **por paciente**, nunca por proceso terapéutico:
//! sigue teniendo sentido clínico con el proceso cerrado o en un reingreso
//! posterior.
//!
//! Ciclo de vida (borrador → vigente → reemplazado), mismo patrón de
//! `services::sessions::create_new_note_version` (Fase 4) adaptado a un
//! campo `status` de tres valores en vez de dos flags booleanos: al
//! confirmar un borrador, el plan vigente anterior (si existe) se marca
//! `reemplazado` **antes** de confirmar el nuevo — nunca hay un instante con
//! dos vigentes para el mismo paciente (el índice único parcial de
//! `SCHEMA_V6` lo garantiza también a nivel de base de datos).
//!
//! Esta capa nunca sabe nada de Tauri, del estado de bloqueo del vault, ni
//! toca Google Calendar en ningún punto — ninguna función de este archivo
//! se referencia jamás desde `calendar::*`.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::patients;
use crate::repositories::safety_plans::{
    self, NewSafetyPlanContactRow, NewSafetyPlanRow, SafetyPlan, SafetyPlanContact, SafetyPlanContactUpdateRow, SafetyPlanDraftUpdateRow, SafetyPlanSummary,
};

/// Taxonomía cerrada de tipo de contacto — fijada en el `CHECK` de
/// `SCHEMA_V6`. Cambiarla exige otra migración.
pub const VALID_CONTACT_TYPES: &[&str] = &["support_person", "professional", "service"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyPlanInput {
    pub warning_signs: Option<String>,
    pub internal_strategies: Option<String>,
    pub social_support_strategies: Option<String>,
    pub means_safety: Option<String>,
    pub crisis_steps: Option<String>,
    pub notes: Option<String>,
    /// Fecha (AAAA-MM-DD) de la última revisión del plan con el paciente.
    /// Puramente informativo — no dispara ninguna acción ni recordatorio.
    pub reviewed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyPlanContactInput {
    pub contact_type: String,
    pub name: String,
    pub relationship_or_role: Option<String>,
    pub phone: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug)]
pub enum SafetyPlanError {
    PatientNotFound,
    PatientArchived,
    AlreadyHasDraft,
    PlanNotFound,
    NotEditable,
    DateFormat,
    ContactNotFound,
    InvalidContactType(String),
    MissingContactName,
    Database(rusqlite::Error),
}

impl fmt::Display for SafetyPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SafetyPlanError::PatientNotFound => write!(f, "paciente no encontrado"),
            SafetyPlanError::PatientArchived => write!(f, "no se puede crear un plan de seguridad nuevo para un paciente archivado"),
            SafetyPlanError::AlreadyHasDraft => write!(f, "este paciente ya tiene un borrador de plan de seguridad sin confirmar"),
            SafetyPlanError::PlanNotFound => write!(f, "plan de seguridad no encontrado"),
            SafetyPlanError::NotEditable => write!(f, "solo un borrador puede editarse — este plan ya está confirmado"),
            SafetyPlanError::DateFormat => write!(f, "fecha inválida (formato esperado: AAAA-MM-DD)"),
            SafetyPlanError::ContactNotFound => write!(f, "contacto no encontrado"),
            SafetyPlanError::InvalidContactType(t) => write!(f, "tipo de contacto inválido: '{t}'"),
            SafetyPlanError::MissingContactName => write!(f, "el contacto necesita un nombre"),
            SafetyPlanError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for SafetyPlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SafetyPlanError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for SafetyPlanError {
    fn from(e: rusqlite::Error) -> Self {
        SafetyPlanError::Database(e)
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// Mismo criterio estructural (no calendárico) que
/// `services::episode_closures::validate_date_format`. Duplicado
/// deliberadamente en vez de exportado desde ahí — es una función privada
/// de ese módulo.
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

struct ValidatedFields {
    warning_signs: Option<String>,
    internal_strategies: Option<String>,
    social_support_strategies: Option<String>,
    means_safety: Option<String>,
    crisis_steps: Option<String>,
    notes: Option<String>,
    reviewed_at: Option<String>,
}

fn validate_input(input: SafetyPlanInput) -> Result<ValidatedFields, SafetyPlanError> {
    let reviewed_at = none_if_blank(input.reviewed_at);
    if let Some(ref d) = reviewed_at {
        if !validate_date_format(d) {
            return Err(SafetyPlanError::DateFormat);
        }
    }
    Ok(ValidatedFields {
        warning_signs: none_if_blank(input.warning_signs),
        internal_strategies: none_if_blank(input.internal_strategies),
        social_support_strategies: none_if_blank(input.social_support_strategies),
        means_safety: none_if_blank(input.means_safety),
        crisis_steps: none_if_blank(input.crisis_steps),
        notes: none_if_blank(input.notes),
        reviewed_at,
    })
}

fn require_existing_patient(conn: &Connection, patient_id: &str) -> Result<patients::Patient, SafetyPlanError> {
    patients::find_by_id(conn, patient_id)?.ok_or(SafetyPlanError::PatientNotFound)
}

fn require_draft(conn: &Connection, plan_id: &str) -> Result<SafetyPlan, SafetyPlanError> {
    let plan = safety_plans::find_by_id(conn, plan_id)?.ok_or(SafetyPlanError::PlanNotFound)?;
    if plan.status != "borrador" {
        return Err(SafetyPlanError::NotEditable);
    }
    Ok(plan)
}

/// El plan vigente de un paciente, si existe. `None` es el estado inicial
/// normal — la ausencia de un plan de seguridad nunca bloquea ningún otro
/// flujo de la aplicación.
pub fn get_current_plan(conn: &Connection, patient_id: &str) -> Result<Option<SafetyPlan>, SafetyPlanError> {
    require_existing_patient(conn, patient_id)?;
    Ok(safety_plans::find_current_by_patient(conn, patient_id)?)
}

/// El borrador sin confirmar de un paciente, si existe.
pub fn get_draft(conn: &Connection, patient_id: &str) -> Result<Option<SafetyPlan>, SafetyPlanError> {
    require_existing_patient(conn, patient_id)?;
    Ok(safety_plans::find_draft_by_patient(conn, patient_id)?)
}

/// Todas las versiones (vigente, reemplazadas y el borrador si existe) —
/// más reciente primero. Uso interno/tests: contenido completo. La capa de
/// comandos nunca expone esta función directamente por IPC — usa
/// `list_history_summaries` (§33/§49 de la aprobación de Fase 12).
#[allow(dead_code)] // se usa desde los tests de este módulo
pub fn list_history(conn: &Connection, patient_id: &str) -> Result<Vec<SafetyPlan>, SafetyPlanError> {
    require_existing_patient(conn, patient_id)?;
    Ok(safety_plans::list_history_by_patient(conn, patient_id)?)
}

/// Versión minimizada del historial para IPC — nunca lleva contenido
/// narrativo ni contactos. Abrir una versión concreta del historial (para
/// leer su contenido completo) pasa por `get_plan_by_id`.
pub fn list_history_summaries(conn: &Connection, patient_id: &str) -> Result<Vec<SafetyPlanSummary>, SafetyPlanError> {
    require_existing_patient(conn, patient_id)?;
    Ok(safety_plans::list_history_summaries_by_patient(conn, patient_id)?)
}

/// El contenido completo de una versión concreta (vigente, reemplazada o el
/// borrador) — solo se llama cuando la usuaria la abre explícitamente desde
/// el historial.
pub fn get_plan_by_id(conn: &Connection, plan_id: &str) -> Result<SafetyPlan, SafetyPlanError> {
    safety_plans::find_by_id(conn, plan_id)?.ok_or(SafetyPlanError::PlanNotFound)
}

/// Crea un borrador nuevo. Rechaza un paciente inexistente o archivado
/// (mismo criterio que `services::goals::create_goal` y
/// `services::patient_clinical_profile::create_clinical_profile`: no se
/// crean registros clínicos nuevos para un paciente archivado), y rechaza
/// crear un segundo borrador mientras el primero sigue sin confirmar o
/// descartar.
pub fn create_draft(conn: &Connection, patient_id: &str, input: SafetyPlanInput) -> Result<SafetyPlan, SafetyPlanError> {
    let patient = require_existing_patient(conn, patient_id)?;
    if patient.deleted_at.is_some() {
        return Err(SafetyPlanError::PatientArchived);
    }
    if safety_plans::find_draft_by_patient(conn, patient_id)?.is_some() {
        return Err(SafetyPlanError::AlreadyHasDraft);
    }

    let f = validate_input(input)?;
    let next_version = safety_plans::max_version_for_patient(conn, patient_id)? + 1;
    let id = uuid::Uuid::new_v4().to_string();
    Ok(safety_plans::insert(
        conn,
        &NewSafetyPlanRow {
            id: &id,
            patient_id,
            version: next_version,
            warning_signs: f.warning_signs.as_deref(),
            internal_strategies: f.internal_strategies.as_deref(),
            social_support_strategies: f.social_support_strategies.as_deref(),
            means_safety: f.means_safety.as_deref(),
            crisis_steps: f.crisis_steps.as_deref(),
            notes: f.notes.as_deref(),
        },
    )?)
}

/// Reemplaza el contenido de un borrador existente. Nunca sobre un plan ya
/// confirmado — inmutabilidad reforzada también a nivel de SQL en
/// `repositories::safety_plans::update_draft`.
pub fn update_draft(conn: &Connection, plan_id: &str, input: SafetyPlanInput) -> Result<SafetyPlan, SafetyPlanError> {
    require_draft(conn, plan_id)?;
    let f = validate_input(input)?;
    let row = SafetyPlanDraftUpdateRow {
        warning_signs: f.warning_signs.as_deref(),
        internal_strategies: f.internal_strategies.as_deref(),
        social_support_strategies: f.social_support_strategies.as_deref(),
        means_safety: f.means_safety.as_deref(),
        crisis_steps: f.crisis_steps.as_deref(),
        notes: f.notes.as_deref(),
        reviewed_at: f.reviewed_at.as_deref(),
    };
    safety_plans::update_draft(conn, plan_id, &row)?;
    safety_plans::find_by_id(conn, plan_id)?.ok_or(SafetyPlanError::PlanNotFound)
}

/// Descarta un borrador nunca confirmado — eliminación en duro, la única
/// permitida por este dominio (§24 de la aprobación de Fase 12: un plan
/// `vigente` o `reemplazado` nunca se elimina). Sus contactos se eliminan
/// en cascada (`ON DELETE CASCADE` de `SCHEMA_V6`).
pub fn discard_draft(conn: &Connection, plan_id: &str) -> Result<(), SafetyPlanError> {
    require_draft(conn, plan_id)?;
    safety_plans::delete_draft(conn, plan_id)?;
    Ok(())
}

/// Confirma un borrador: si el paciente ya tenía un plan vigente, lo marca
/// `reemplazado` primero — dentro de la misma transacción, así nunca hay un
/// instante con dos vigentes a la vez.
pub fn confirm_draft(conn: &Connection, plan_id: &str) -> Result<SafetyPlan, SafetyPlanError> {
    let draft = require_draft(conn, plan_id)?;

    let tx = conn.unchecked_transaction()?;
    if let Some(current) = safety_plans::find_current_by_patient(&tx, &draft.patient_id)? {
        safety_plans::mark_superseded(&tx, &current.id)?;
    }
    safety_plans::confirm(&tx, plan_id)?;
    let confirmed = safety_plans::find_by_id(&tx, plan_id)?.ok_or(SafetyPlanError::PlanNotFound)?;
    tx.commit()?;
    Ok(confirmed)
}

pub fn list_contacts(conn: &Connection, plan_id: &str) -> Result<Vec<SafetyPlanContact>, SafetyPlanError> {
    safety_plans::find_by_id(conn, plan_id)?.ok_or(SafetyPlanError::PlanNotFound)?;
    Ok(safety_plans::list_contacts_by_plan(conn, plan_id)?)
}

fn validate_contact_input(input: &SafetyPlanContactInput) -> Result<(), SafetyPlanError> {
    if !VALID_CONTACT_TYPES.contains(&input.contact_type.as_str()) {
        return Err(SafetyPlanError::InvalidContactType(input.contact_type.clone()));
    }
    if input.name.trim().is_empty() {
        return Err(SafetyPlanError::MissingContactName);
    }
    Ok(())
}

/// Solo puede agregarse un contacto a un plan todavía en borrador — mismo
/// criterio de inmutabilidad que el contenido del propio plan.
pub fn add_contact(conn: &Connection, plan_id: &str, input: SafetyPlanContactInput) -> Result<SafetyPlanContact, SafetyPlanError> {
    require_draft(conn, plan_id)?;
    validate_contact_input(&input)?;

    let existing = safety_plans::list_contacts_by_plan(conn, plan_id)?;
    let id = uuid::Uuid::new_v4().to_string();
    Ok(safety_plans::insert_contact(
        conn,
        &NewSafetyPlanContactRow {
            id: &id,
            safety_plan_id: plan_id,
            contact_type: &input.contact_type,
            name: input.name.trim(),
            relationship_or_role: none_if_blank(input.relationship_or_role).as_deref(),
            phone: none_if_blank(input.phone).as_deref(),
            notes: none_if_blank(input.notes).as_deref(),
            sort_order: existing.len() as i64,
        },
    )?)
}

fn require_contact_on_draft(conn: &Connection, contact_id: &str) -> Result<SafetyPlanContact, SafetyPlanError> {
    let contact = safety_plans::find_contact_by_id(conn, contact_id)?.ok_or(SafetyPlanError::ContactNotFound)?;
    require_draft(conn, &contact.safety_plan_id)?;
    Ok(contact)
}

pub fn update_contact(conn: &Connection, contact_id: &str, input: SafetyPlanContactInput) -> Result<SafetyPlanContact, SafetyPlanError> {
    let existing = require_contact_on_draft(conn, contact_id)?;
    validate_contact_input(&input)?;
    let relationship_or_role = none_if_blank(input.relationship_or_role);
    let phone = none_if_blank(input.phone);
    let notes = none_if_blank(input.notes);
    let row = SafetyPlanContactUpdateRow {
        contact_type: &input.contact_type,
        name: input.name.trim(),
        relationship_or_role: relationship_or_role.as_deref(),
        phone: phone.as_deref(),
        notes: notes.as_deref(),
        sort_order: existing.sort_order,
    };
    safety_plans::update_contact(conn, contact_id, &row)?.ok_or(SafetyPlanError::ContactNotFound)
}

pub fn delete_contact(conn: &Connection, contact_id: &str) -> Result<(), SafetyPlanError> {
    require_contact_on_draft(conn, contact_id)?;
    safety_plans::delete_contact(conn, contact_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::services::patients::{self as patients_service, PatientInput};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-safety-plans-svc-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x54u8; VAULT_KEY_LEN]);
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

    fn empty_input() -> SafetyPlanInput {
        SafetyPlanInput { warning_signs: None, internal_strategies: None, social_support_strategies: None, means_safety: None, crisis_steps: None, notes: None, reviewed_at: None }
    }

    fn full_input() -> SafetyPlanInput {
        SafetyPlanInput {
            warning_signs: Some("Aislamiento, insomnio".to_string()),
            internal_strategies: Some("Respiración, salir a caminar".to_string()),
            social_support_strategies: Some("Llamar a un amigo".to_string()),
            means_safety: Some("Guardar medicamentos con un familiar".to_string()),
            crisis_steps: Some("1. Llamar a la línea de ayuda".to_string()),
            notes: Some("Notas adicionales".to_string()),
            reviewed_at: Some("2026-09-01".to_string()),
        }
    }

    // ---- crear borrador ----

    #[test]
    fn creates_a_draft_with_version_one() {
        let conn = test_conn("create-draft-v1");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let plan = create_draft(&conn, &patient_id, full_input()).unwrap();
        assert_eq!(plan.version, 1);
        assert_eq!(plan.status, "borrador");
        assert_eq!(plan.warning_signs.as_deref(), Some("Aislamiento, insomnio"));
    }

    #[test]
    fn rejects_creating_a_draft_for_a_nonexistent_patient() {
        let conn = test_conn("create-nonexistent-patient");
        let err = create_draft(&conn, "no-existe", empty_input()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PatientNotFound));
    }

    #[test]
    fn rejects_creating_a_draft_for_an_archived_patient() {
        let conn = test_conn("create-archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Archivado");
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = create_draft(&conn, &patient_id, empty_input()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PatientArchived));
    }

    #[test]
    fn rejects_a_second_draft_while_the_first_is_unconfirmed() {
        let conn = test_conn("create-duplicate-draft");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        create_draft(&conn, &patient_id, empty_input()).unwrap();
        let err = create_draft(&conn, &patient_id, empty_input()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::AlreadyHasDraft));
    }

    #[test]
    fn rejects_an_invalid_reviewed_at_format() {
        let conn = test_conn("create-bad-date");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let mut input = empty_input();
        input.reviewed_at = Some("01-09-2026".to_string());
        let err = create_draft(&conn, &patient_id, input).unwrap_err();
        assert!(matches!(err, SafetyPlanError::DateFormat));
    }

    // ---- editar borrador ----

    #[test]
    fn updates_a_draft() {
        let conn = test_conn("update-draft");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let updated = update_draft(&conn, &plan.id, full_input()).unwrap();
        assert_eq!(updated.crisis_steps.as_deref(), Some("1. Llamar a la línea de ayuda"));
    }

    #[test]
    fn rejects_updating_a_confirmed_plan() {
        let conn = test_conn("update-confirmed");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan.id).unwrap();
        let err = update_draft(&conn, &plan.id, full_input()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::NotEditable));
    }

    #[test]
    fn rejects_updating_a_nonexistent_plan() {
        let conn = test_conn("update-nonexistent");
        let err = update_draft(&conn, "no-existe", empty_input()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PlanNotFound));
    }

    // ---- descartar borrador ----

    #[test]
    fn discards_a_draft() {
        let conn = test_conn("discard-draft");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        discard_draft(&conn, &plan.id).unwrap();
        assert!(get_draft(&conn, &patient_id).unwrap().is_none());
    }

    #[test]
    fn discarding_frees_up_creating_a_new_draft() {
        // Un borrador descartado se elimina en duro (nunca existió como
        // contenido clínico confirmado), así que su número de versión queda
        // libre para el siguiente borrador — a diferencia de un plan
        // reemplazado, cuya versión sí queda fija para siempre en el
        // historial.
        let conn = test_conn("discard-then-create");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        discard_draft(&conn, &plan.id).unwrap();
        let plan2 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        assert_eq!(plan2.version, 1);
    }

    #[test]
    fn rejects_discarding_a_confirmed_plan() {
        let conn = test_conn("discard-confirmed");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan.id).unwrap();
        let err = discard_draft(&conn, &plan.id).unwrap_err();
        assert!(matches!(err, SafetyPlanError::NotEditable));
    }

    // ---- confirmar ----

    #[test]
    fn confirms_the_first_draft_as_current() {
        let conn = test_conn("confirm-first");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let confirmed = confirm_draft(&conn, &plan.id).unwrap();
        assert_eq!(confirmed.status, "vigente");
        assert_eq!(get_current_plan(&conn, &patient_id).unwrap().unwrap().id, confirmed.id);
    }

    #[test]
    fn confirming_a_new_draft_supersedes_the_previous_current_plan() {
        let conn = test_conn("confirm-supersedes");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let plan1 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();

        let plan2 = create_draft(&conn, &patient_id, full_input()).unwrap();
        let confirmed2 = confirm_draft(&conn, &plan2.id).unwrap();

        assert_eq!(get_current_plan(&conn, &patient_id).unwrap().unwrap().id, confirmed2.id);
        let history = list_history(&conn, &patient_id).unwrap();
        let old = history.iter().find(|p| p.id == plan1.id).unwrap();
        assert_eq!(old.status, "reemplazado");
        assert!(old.superseded_at.is_some());
        // El contenido original del plan reemplazado nunca se pierde.
        assert!(old.warning_signs.is_none());
    }

    #[test]
    fn rejects_confirming_an_already_confirmed_plan() {
        let conn = test_conn("confirm-twice");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan.id).unwrap();
        let err = confirm_draft(&conn, &plan.id).unwrap_err();
        assert!(matches!(err, SafetyPlanError::NotEditable));
    }

    // ---- contactos ----

    fn support_contact() -> SafetyPlanContactInput {
        SafetyPlanContactInput { contact_type: "support_person".to_string(), name: "Amiga cercana".to_string(), relationship_or_role: Some("Amistad".to_string()), phone: Some("+56900000001".to_string()), notes: None }
    }

    #[test]
    fn adds_and_lists_contacts_in_insertion_order() {
        let conn = test_conn("contacts-add-list");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        add_contact(&conn, &plan.id, support_contact()).unwrap();
        add_contact(&conn, &plan.id, SafetyPlanContactInput { contact_type: "professional".to_string(), name: "Psiquiatra tratante".to_string(), relationship_or_role: None, phone: None, notes: None }).unwrap();

        let contacts = list_contacts(&conn, &plan.id).unwrap();
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].name, "Amiga cercana");
        assert_eq!(contacts[1].name, "Psiquiatra tratante");
    }

    #[test]
    fn rejects_an_invalid_contact_type() {
        let conn = test_conn("contacts-invalid-type");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let mut input = support_contact();
        input.contact_type = "inventado".to_string();
        let err = add_contact(&conn, &plan.id, input).unwrap_err();
        assert!(matches!(err, SafetyPlanError::InvalidContactType(t) if t == "inventado"));
    }

    #[test]
    fn rejects_a_blank_contact_name() {
        let conn = test_conn("contacts-blank-name");
        let patient_id = create_test_patient(&conn, "Paciente Catorce");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let mut input = support_contact();
        input.name = "   ".to_string();
        let err = add_contact(&conn, &plan.id, input).unwrap_err();
        assert!(matches!(err, SafetyPlanError::MissingContactName));
    }

    #[test]
    fn rejects_adding_a_contact_to_a_confirmed_plan() {
        let conn = test_conn("contacts-confirmed-plan");
        let patient_id = create_test_patient(&conn, "Paciente Quince");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan.id).unwrap();
        let err = add_contact(&conn, &plan.id, support_contact()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::NotEditable));
    }

    #[test]
    fn updates_and_deletes_a_contact_on_a_draft() {
        let conn = test_conn("contacts-update-delete");
        let patient_id = create_test_patient(&conn, "Paciente Dieciseis");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let contact = add_contact(&conn, &plan.id, support_contact()).unwrap();

        let mut edit = support_contact();
        edit.name = "Nombre Editado".to_string();
        let updated = update_contact(&conn, &contact.id, edit).unwrap();
        assert_eq!(updated.name, "Nombre Editado");

        delete_contact(&conn, &contact.id).unwrap();
        assert!(list_contacts(&conn, &plan.id).unwrap().is_empty());
    }

    #[test]
    fn rejects_editing_a_contact_of_an_already_confirmed_plan() {
        let conn = test_conn("contacts-edit-confirmed");
        let patient_id = create_test_patient(&conn, "Paciente Diecisiete");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let contact = add_contact(&conn, &plan.id, support_contact()).unwrap();
        confirm_draft(&conn, &plan.id).unwrap();

        let err = update_contact(&conn, &contact.id, support_contact()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::NotEditable));
        let err = delete_contact(&conn, &contact.id).unwrap_err();
        assert!(matches!(err, SafetyPlanError::NotEditable));
    }

    #[test]
    fn a_confirmed_plans_contacts_are_still_readable() {
        let conn = test_conn("contacts-readable-confirmed");
        let patient_id = create_test_patient(&conn, "Paciente Dieciocho");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        add_contact(&conn, &plan.id, support_contact()).unwrap();
        confirm_draft(&conn, &plan.id).unwrap();

        let contacts = list_contacts(&conn, &plan.id).unwrap();
        assert_eq!(contacts.len(), 1, "confirmar el plan no debe ocultar ni borrar sus contactos");
    }

    // ---- aislamiento entre pacientes ----

    #[test]
    fn a_patients_plan_is_never_visible_through_another_patients_id() {
        let conn = test_conn("isolation");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        create_draft(&conn, &patient_a, full_input()).unwrap();

        assert!(get_draft(&conn, &patient_b).unwrap().is_none());
        assert!(get_current_plan(&conn, &patient_b).unwrap().is_none());
        assert!(list_history(&conn, &patient_b).unwrap().is_empty());
    }

    // ---- minimización IPC del historial ----

    #[test]
    fn list_history_summaries_reports_the_same_versions_as_the_full_history() {
        let conn = test_conn("summaries-match-full");
        let patient_id = create_test_patient(&conn, "Paciente Veinte");
        let plan1 = create_draft(&conn, &patient_id, full_input()).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();
        let plan2 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan2.id).unwrap();

        let summaries = list_history_summaries(&conn, &patient_id).unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, plan2.id);
        assert_eq!(summaries[1].id, plan1.id);
        assert_eq!(summaries[1].status, "reemplazado");
    }

    #[test]
    fn get_plan_by_id_returns_the_full_content_of_a_historical_version() {
        let conn = test_conn("get-by-id-historical");
        let patient_id = create_test_patient(&conn, "Paciente Veintiuno");
        let plan1 = create_draft(&conn, &patient_id, full_input()).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();
        let plan2 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan2.id).unwrap();

        let fetched = get_plan_by_id(&conn, &plan1.id).unwrap();
        assert_eq!(fetched.status, "reemplazado");
        assert_eq!(fetched.warning_signs.as_deref(), Some("Aislamiento, insomnio"));
    }

    #[test]
    fn get_plan_by_id_reports_not_found_for_an_unknown_id() {
        let conn = test_conn("get-by-id-not-found");
        let err = get_plan_by_id(&conn, "no-existe").unwrap_err();
        assert!(matches!(err, SafetyPlanError::PlanNotFound));
    }
}
