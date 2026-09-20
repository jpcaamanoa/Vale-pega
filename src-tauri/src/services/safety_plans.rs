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
    self, NewSafetyPlanContactRow, NewSafetyPlanListItemRow, NewSafetyPlanRow, SafetyPlan, SafetyPlanContact, SafetyPlanContactUpdateRow, SafetyPlanDraftUpdateRow, SafetyPlanListItem,
    SafetyPlanSummary,
};

/// Taxonomía cerrada de tipo de contacto — fijada en el `CHECK` de
/// `SCHEMA_V6`/ampliada en `SCHEMA_V11`. Cambiarla exige otra migración.
///
/// `support_person` se conserva únicamente por compatibilidad con
/// contactos creados antes del rediseño de seis pasos (Fase de
/// continuación post-Fase 19) — la interfaz nueva ya no lo ofrece como
/// opción al crear un contacto, solo lo permite seguir leyendo/editando/
/// eliminando en planes que ya lo tenían. Los seis pasos usan:
/// `distraction_person` (Paso 3, personas), `professional`/`service`
/// (Paso 5, sin cambios), `help_contact` (Paso 4).
pub const VALID_CONTACT_TYPES: &[&str] = &["support_person", "professional", "service", "distraction_person", "help_contact"];

/// Tipos de contacto que la interfaz nueva sigue ofreciendo para crear un
/// contacto — `support_person` queda fuera a propósito (ver
/// `VALID_CONTACT_TYPES`).
pub const CREATABLE_CONTACT_TYPES: &[&str] = &["professional", "service", "distraction_person", "help_contact"];

/// Taxonomía cerrada de `safety_plan_list_items.item_type` — fijada en el
/// `CHECK` de `SCHEMA_V11`. Cada valor corresponde a una lista agregable de
/// los seis pasos: `warning_sign` (Paso 1, señales de alerta),
/// `strategy` (Paso 2, estrategias individuales) y `distraction_place`
/// (Paso 3, lugares de distracción — las *personas* de ese mismo paso son
/// contactos `distraction_person`, no ítems de esta lista).
pub const VALID_ITEM_TYPES: &[&str] = &["warning_sign", "strategy", "distraction_place"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyPlanListItemInput {
    pub item_type: String,
    pub content: String,
}

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
    /// Solo tienen sentido para `contact_type` IN ('professional','service')
    /// (Paso 5) — se aceptan igual para cualquier otro tipo (quedan
    /// simplemente sin usar) en vez de rechazarlos, para no acoplar esta
    /// validación a una regla de UI que puede cambiar.
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub service_phone: Option<String>,
    #[serde(default)]
    pub is_emergency_contact: bool,
    #[serde(default)]
    pub is_crisis_service: bool,
}

#[derive(Debug)]
pub enum SafetyPlanError {
    PatientNotFound,
    PatientArchived,
    AlreadyHasDraft,
    NoCurrentPlan,
    PlanNotFound,
    NotEditable,
    DateFormat,
    ContactNotFound,
    InvalidContactType(String),
    MissingContactName,
    ItemNotFound,
    InvalidItemType(String),
    MissingItemContent,
    Database(rusqlite::Error),
}

impl fmt::Display for SafetyPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SafetyPlanError::PatientNotFound => write!(f, "paciente no encontrado"),
            SafetyPlanError::PatientArchived => write!(f, "no se puede crear un plan de seguridad nuevo para un paciente archivado"),
            SafetyPlanError::AlreadyHasDraft => write!(f, "este paciente ya tiene un borrador de plan de seguridad sin confirmar"),
            SafetyPlanError::NoCurrentPlan => write!(f, "este paciente todavía no tiene un plan de seguridad vigente para actualizar"),
            SafetyPlanError::PlanNotFound => write!(f, "plan de seguridad no encontrado"),
            SafetyPlanError::NotEditable => write!(f, "solo un borrador puede editarse — este plan ya está confirmado"),
            SafetyPlanError::DateFormat => write!(f, "fecha inválida (formato esperado: AAAA-MM-DD)"),
            SafetyPlanError::ContactNotFound => write!(f, "contacto no encontrado"),
            SafetyPlanError::InvalidContactType(t) => write!(f, "tipo de contacto inválido: '{t}'"),
            SafetyPlanError::MissingContactName => write!(f, "el contacto necesita un nombre"),
            SafetyPlanError::ItemNotFound => write!(f, "ítem no encontrado"),
            SafetyPlanError::InvalidItemType(t) => write!(f, "tipo de ítem inválido: '{t}'"),
            SafetyPlanError::MissingItemContent => write!(f, "el ítem necesita contenido"),
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

fn require_not_archived(conn: &Connection, patient_id: &str) -> Result<(), SafetyPlanError> {
    let patient = require_existing_patient(conn, patient_id)?;
    if patient.deleted_at.is_some() {
        return Err(SafetyPlanError::PatientArchived);
    }
    Ok(())
}

/// Un borrador solo es editable (contenido, contactos, confirmación) si
/// además de estar en estado `borrador`, su paciente no está archivado.
/// Micro-hardening post-Fase 12: descartar un borrador queda
/// deliberadamente **fuera** de esta función — decisión de producto
/// explícita (descartar no crea ni modifica historia clínica confirmada,
/// mismo criterio que el resto del código nunca bloquea eliminar/editar
/// contenido no confirmado de un paciente archivado — ver
/// `discard_draft`).
fn require_editable_draft(conn: &Connection, plan_id: &str) -> Result<SafetyPlan, SafetyPlanError> {
    let plan = require_draft(conn, plan_id)?;
    require_not_archived(conn, &plan.patient_id)?;
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
            reviewed_at: f.reviewed_at.as_deref(),
        },
    )?)
}

/// Crea un borrador nuevo precargado con una copia **completa e
/// independiente** del plan vigente actual — contenido narrativo,
/// `reviewed_at` y contactos (con IDs nuevos, nunca compartiendo filas con
/// el vigente). Es el camino real detrás de "Actualizar plan": micro-
/// hardening post-Fase 12, motivado por que el frontend por sí solo
/// precargaba únicamente el texto y dejaba los contactos vacíos en
/// silencio, con riesgo real de que la red de apoyo de la versión anterior
/// se perdiera al confirmar la nueva sin que la profesional lo advirtiera.
///
/// Editar los contactos de la v2 después de esta copia nunca toca los de
/// la v1 (ni viceversa): son filas físicamente distintas desde el momento
/// de la copia, verificado en test.
pub fn create_draft_from_current(conn: &Connection, patient_id: &str) -> Result<SafetyPlan, SafetyPlanError> {
    let patient = require_existing_patient(conn, patient_id)?;
    if patient.deleted_at.is_some() {
        return Err(SafetyPlanError::PatientArchived);
    }
    if safety_plans::find_draft_by_patient(conn, patient_id)?.is_some() {
        return Err(SafetyPlanError::AlreadyHasDraft);
    }
    let current = safety_plans::find_current_by_patient(conn, patient_id)?.ok_or(SafetyPlanError::NoCurrentPlan)?;

    let next_version = safety_plans::max_version_for_patient(conn, patient_id)? + 1;
    let new_id = uuid::Uuid::new_v4().to_string();

    let tx = conn.unchecked_transaction()?;
    safety_plans::insert(
        &tx,
        &NewSafetyPlanRow {
            id: &new_id,
            patient_id,
            version: next_version,
            warning_signs: current.warning_signs.as_deref(),
            internal_strategies: current.internal_strategies.as_deref(),
            social_support_strategies: current.social_support_strategies.as_deref(),
            means_safety: current.means_safety.as_deref(),
            crisis_steps: current.crisis_steps.as_deref(),
            notes: current.notes.as_deref(),
            reviewed_at: current.reviewed_at.as_deref(),
        },
    )?;
    for contact in safety_plans::list_contacts_by_plan(&tx, &current.id)? {
        safety_plans::insert_contact(
            &tx,
            &NewSafetyPlanContactRow {
                id: &uuid::Uuid::new_v4().to_string(),
                safety_plan_id: &new_id,
                contact_type: &contact.contact_type,
                name: &contact.name,
                relationship_or_role: contact.relationship_or_role.as_deref(),
                phone: contact.phone.as_deref(),
                notes: contact.notes.as_deref(),
                address: contact.address.as_deref(),
                service_phone: contact.service_phone.as_deref(),
                is_emergency_contact: contact.is_emergency_contact,
                is_crisis_service: contact.is_crisis_service,
                sort_order: contact.sort_order,
            },
        )?;
    }
    for item in safety_plans::list_items_by_plan(&tx, &current.id)? {
        safety_plans::insert_list_item(
            &tx,
            &NewSafetyPlanListItemRow {
                id: &uuid::Uuid::new_v4().to_string(),
                safety_plan_id: &new_id,
                item_type: &item.item_type,
                content: &item.content,
                sort_order: item.sort_order,
            },
        )?;
    }
    let new_draft = safety_plans::find_by_id(&tx, &new_id)?.expect("se acaba de insertar");
    tx.commit()?;
    Ok(new_draft)
}

/// Reemplaza el contenido de un borrador existente. Nunca sobre un plan ya
/// confirmado — inmutabilidad reforzada también a nivel de SQL en
/// `repositories::safety_plans::update_draft`. Rechaza también un paciente
/// archivado **después** de creado el borrador (micro-hardening post-Fase
/// 12: `require_draft` por sí sola no lo comprobaba — la autoridad real
/// vive aquí, no solo en que React oculte el botón).
pub fn update_draft(conn: &Connection, plan_id: &str, input: SafetyPlanInput) -> Result<SafetyPlan, SafetyPlanError> {
    require_editable_draft(conn, plan_id)?;
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
/// instante con dos vigentes a la vez. Rechaza también un paciente
/// archivado (micro-hardening post-Fase 12 — ver `update_draft`).
pub fn confirm_draft(conn: &Connection, plan_id: &str) -> Result<SafetyPlan, SafetyPlanError> {
    let draft = require_editable_draft(conn, plan_id)?;

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

fn validate_contact_input(input: &SafetyPlanContactInput, allowed_types: &[&str]) -> Result<(), SafetyPlanError> {
    if !allowed_types.contains(&input.contact_type.as_str()) {
        return Err(SafetyPlanError::InvalidContactType(input.contact_type.clone()));
    }
    if input.name.trim().is_empty() {
        return Err(SafetyPlanError::MissingContactName);
    }
    Ok(())
}

/// Solo puede agregarse un contacto a un plan todavía en borrador de un
/// paciente no archivado — mismo criterio de inmutabilidad/archivado que el
/// contenido narrativo del propio borrador. Un contacto nuevo solo puede
/// crearse con uno de `CREATABLE_CONTACT_TYPES` — `support_person` es
/// exclusivamente de lectura/edición para planes que ya lo tenían.
pub fn add_contact(conn: &Connection, plan_id: &str, input: SafetyPlanContactInput) -> Result<SafetyPlanContact, SafetyPlanError> {
    require_editable_draft(conn, plan_id)?;
    validate_contact_input(&input, CREATABLE_CONTACT_TYPES)?;

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
            address: none_if_blank(input.address).as_deref(),
            service_phone: none_if_blank(input.service_phone).as_deref(),
            is_emergency_contact: input.is_emergency_contact,
            is_crisis_service: input.is_crisis_service,
            sort_order: existing.len() as i64,
        },
    )?)
}

fn require_contact_on_draft(conn: &Connection, contact_id: &str) -> Result<SafetyPlanContact, SafetyPlanError> {
    let contact = safety_plans::find_contact_by_id(conn, contact_id)?.ok_or(SafetyPlanError::ContactNotFound)?;
    require_editable_draft(conn, &contact.safety_plan_id)?;
    Ok(contact)
}

/// A diferencia de `add_contact`, valida contra `VALID_CONTACT_TYPES` (no
/// `CREATABLE_CONTACT_TYPES`): editar un contacto `support_person` creado
/// antes del rediseño de seis pasos (por ejemplo, solo corregirle el
/// teléfono) nunca debe bloquearse por un tipo que ya no se ofrece para
/// contactos nuevos.
pub fn update_contact(conn: &Connection, contact_id: &str, input: SafetyPlanContactInput) -> Result<SafetyPlanContact, SafetyPlanError> {
    let existing = require_contact_on_draft(conn, contact_id)?;
    validate_contact_input(&input, VALID_CONTACT_TYPES)?;
    let relationship_or_role = none_if_blank(input.relationship_or_role);
    let phone = none_if_blank(input.phone);
    let notes = none_if_blank(input.notes);
    let address = none_if_blank(input.address);
    let service_phone = none_if_blank(input.service_phone);
    let row = SafetyPlanContactUpdateRow {
        contact_type: &input.contact_type,
        name: input.name.trim(),
        relationship_or_role: relationship_or_role.as_deref(),
        phone: phone.as_deref(),
        notes: notes.as_deref(),
        address: address.as_deref(),
        service_phone: service_phone.as_deref(),
        is_emergency_contact: input.is_emergency_contact,
        is_crisis_service: input.is_crisis_service,
        sort_order: existing.sort_order,
    };
    safety_plans::update_contact(conn, contact_id, &row)?.ok_or(SafetyPlanError::ContactNotFound)
}

pub fn delete_contact(conn: &Connection, contact_id: &str) -> Result<(), SafetyPlanError> {
    require_contact_on_draft(conn, contact_id)?;
    safety_plans::delete_contact(conn, contact_id)?;
    Ok(())
}

// ---- ítems de lista (Paso 1: señales de alerta, Paso 2: estrategias
// individuales, Paso 3: lugares de distracción) ----

/// Todos los ítems de un plan, de cualquier tipo. El frontend los separa por
/// `item_type` para las tres secciones correspondientes — nunca se agregan
/// como un solo textarea (decisión explícita del rediseño de seis pasos).
pub fn list_items(conn: &Connection, plan_id: &str) -> Result<Vec<SafetyPlanListItem>, SafetyPlanError> {
    safety_plans::find_by_id(conn, plan_id)?.ok_or(SafetyPlanError::PlanNotFound)?;
    Ok(safety_plans::list_items_by_plan(conn, plan_id)?)
}

fn validate_item_input(input: &SafetyPlanListItemInput) -> Result<(), SafetyPlanError> {
    if !VALID_ITEM_TYPES.contains(&input.item_type.as_str()) {
        return Err(SafetyPlanError::InvalidItemType(input.item_type.clone()));
    }
    if input.content.trim().is_empty() {
        return Err(SafetyPlanError::MissingItemContent);
    }
    Ok(())
}

/// Solo puede agregarse un ítem a un plan todavía en borrador de un
/// paciente no archivado — mismo criterio que `add_contact`. El orden se
/// calcula dentro del propio `item_type` (cada lista se reordena de forma
/// independiente de las otras dos).
pub fn add_item(conn: &Connection, plan_id: &str, input: SafetyPlanListItemInput) -> Result<SafetyPlanListItem, SafetyPlanError> {
    require_editable_draft(conn, plan_id)?;
    validate_item_input(&input)?;

    let existing_of_type = safety_plans::list_items_by_plan(conn, plan_id)?.into_iter().filter(|i| i.item_type == input.item_type).count();
    let id = uuid::Uuid::new_v4().to_string();
    Ok(safety_plans::insert_list_item(
        conn,
        &NewSafetyPlanListItemRow {
            id: &id,
            safety_plan_id: plan_id,
            item_type: &input.item_type,
            content: input.content.trim(),
            sort_order: existing_of_type as i64,
        },
    )?)
}

fn require_item_on_draft(conn: &Connection, item_id: &str) -> Result<SafetyPlanListItem, SafetyPlanError> {
    let item = safety_plans::find_list_item_by_id(conn, item_id)?.ok_or(SafetyPlanError::ItemNotFound)?;
    require_editable_draft(conn, &item.safety_plan_id)?;
    Ok(item)
}

/// El tipo de un ítem nunca cambia después de creado (moverlo entre listas
/// no tiene sentido de producto) — solo su contenido es editable.
pub fn update_item(conn: &Connection, item_id: &str, content: &str) -> Result<SafetyPlanListItem, SafetyPlanError> {
    let existing = require_item_on_draft(conn, item_id)?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(SafetyPlanError::MissingItemContent);
    }
    safety_plans::update_list_item(conn, item_id, trimmed, existing.sort_order)?.ok_or(SafetyPlanError::ItemNotFound)
}

pub fn delete_item(conn: &Connection, item_id: &str) -> Result<(), SafetyPlanError> {
    require_item_on_draft(conn, item_id)?;
    safety_plans::delete_list_item(conn, item_id)?;
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

    /// Micro-hardening post-Fase 12: `update_draft` debe rechazar un
    /// paciente archivado **después** de creado el borrador — no solo en el
    /// momento de crearlo.
    #[test]
    fn rejects_updating_a_draft_after_the_patient_is_archived() {
        let conn = test_conn("update-archived-after-creation");
        let patient_id = create_test_patient(&conn, "Paciente Archivado Update");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = update_draft(&conn, &plan.id, full_input()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PatientArchived));
    }

    #[test]
    fn allows_updating_a_draft_again_after_the_patient_is_restored() {
        let conn = test_conn("update-restored");
        let patient_id = create_test_patient(&conn, "Paciente Restaurado Update");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        patients_service::restore_patient(&conn, &patient_id).unwrap();
        let updated = update_draft(&conn, &plan.id, full_input()).unwrap();
        assert_eq!(updated.crisis_steps.as_deref(), Some("1. Llamar a la línea de ayuda"));
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

    /// Decisión de producto explícita del micro-hardening post-Fase 12:
    /// descartar un borrador **sí** se permite aunque el paciente esté
    /// archivado — no crea ni modifica historia clínica confirmada, mismo
    /// criterio que el resto del código nunca bloquea eliminar/editar
    /// contenido no confirmado de un paciente archivado.
    #[test]
    fn allows_discarding_a_draft_even_if_the_patient_is_archived() {
        let conn = test_conn("discard-archived-allowed");
        let patient_id = create_test_patient(&conn, "Paciente Descarte Archivado");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        discard_draft(&conn, &plan.id).unwrap();
        assert!(get_draft(&conn, &patient_id).unwrap().is_none());
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

    /// Micro-hardening post-Fase 12: `confirm_draft` debe rechazar un
    /// paciente archivado **después** de creado el borrador.
    #[test]
    fn rejects_confirming_a_draft_after_the_patient_is_archived() {
        let conn = test_conn("confirm-archived-after-creation");
        let patient_id = create_test_patient(&conn, "Paciente Archivado Confirm");
        let plan = create_draft(&conn, &patient_id, full_input()).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = confirm_draft(&conn, &plan.id).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PatientArchived));
        assert!(get_current_plan(&conn, &patient_id).unwrap().is_none(), "no debe haber quedado confirmado a medias");
    }

    #[test]
    fn allows_confirming_a_draft_again_after_the_patient_is_restored() {
        let conn = test_conn("confirm-restored");
        let patient_id = create_test_patient(&conn, "Paciente Restaurado Confirm");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        patients_service::restore_patient(&conn, &patient_id).unwrap();
        let confirmed = confirm_draft(&conn, &plan.id).unwrap();
        assert_eq!(confirmed.status, "vigente");
    }

    // ---- contactos ----

    /// Un contacto de un tipo `CREATABLE_CONTACT_TYPES` cualquiera, para los
    /// tests genéricos de CRUD que no ejercitan específicamente la
    /// compatibilidad con `support_person` (ver `legacy_support_contact`
    /// más abajo para esos casos).
    fn support_contact() -> SafetyPlanContactInput {
        SafetyPlanContactInput { contact_type: "distraction_person".to_string(), name: "Amiga cercana".to_string(), relationship_or_role: Some("Amistad".to_string()), phone: Some("+56900000001".to_string()), notes: None, address: None, service_phone: None, is_emergency_contact: false, is_crisis_service: false }
    }

    #[test]
    fn adds_and_lists_contacts_in_insertion_order() {
        let conn = test_conn("contacts-add-list");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        add_contact(&conn, &plan.id, support_contact()).unwrap();
        add_contact(&conn, &plan.id, SafetyPlanContactInput { contact_type: "professional".to_string(), name: "Psiquiatra tratante".to_string(), relationship_or_role: None, phone: None, notes: None, address: None, service_phone: None, is_emergency_contact: false, is_crisis_service: false }).unwrap();

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

    /// Micro-hardening post-Fase 12: "modificar borrador" incluye sus
    /// contactos — un paciente archivado no puede agregar contactos nuevos
    /// a un borrador ya existente, mismo criterio que el contenido
    /// narrativo.
    #[test]
    fn rejects_adding_a_contact_after_the_patient_is_archived() {
        let conn = test_conn("contacts-archived-add");
        let patient_id = create_test_patient(&conn, "Paciente Contactos Archivado");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = add_contact(&conn, &plan.id, support_contact()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PatientArchived));
    }

    #[test]
    fn rejects_editing_and_deleting_a_contact_after_the_patient_is_archived() {
        let conn = test_conn("contacts-archived-edit-delete");
        let patient_id = create_test_patient(&conn, "Paciente Contactos Archivado Dos");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let contact = add_contact(&conn, &plan.id, support_contact()).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();

        let err = update_contact(&conn, &contact.id, support_contact()).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PatientArchived));
        let err = delete_contact(&conn, &contact.id).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PatientArchived));
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

    // ---- "Actualizar plan": create_draft_from_current (micro-hardening post-Fase 12) ----

    #[test]
    fn create_draft_from_current_copies_narrative_content_and_reviewed_at() {
        let conn = test_conn("from-current-copies-narrative");
        let patient_id = create_test_patient(&conn, "Paciente Actualizar Uno");
        let plan1 = create_draft(&conn, &patient_id, full_input()).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();

        let plan2 = create_draft_from_current(&conn, &patient_id).unwrap();
        assert_eq!(plan2.status, "borrador");
        assert_eq!(plan2.version, 2);
        assert_eq!(plan2.warning_signs, plan1.warning_signs);
        assert_eq!(plan2.internal_strategies, plan1.internal_strategies);
        assert_eq!(plan2.social_support_strategies, plan1.social_support_strategies);
        assert_eq!(plan2.means_safety, plan1.means_safety);
        assert_eq!(plan2.crisis_steps, plan1.crisis_steps);
        assert_eq!(plan2.notes, plan1.notes);
        assert_eq!(plan2.reviewed_at, plan1.reviewed_at);
    }

    #[test]
    fn create_draft_from_current_copies_all_contacts_with_new_ids_and_order() {
        let conn = test_conn("from-current-copies-contacts");
        let patient_id = create_test_patient(&conn, "Paciente Actualizar Dos");
        let plan1 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let c1 = add_contact(&conn, &plan1.id, support_contact()).unwrap();
        let c2 = add_contact(
            &conn,
            &plan1.id,
            SafetyPlanContactInput { contact_type: "professional".to_string(), name: "Psiquiatra tratante".to_string(), relationship_or_role: Some("Psiquiatra".to_string()), phone: Some("+56900000002".to_string()), notes: Some("Nota".to_string()), address: None, service_phone: None, is_emergency_contact: false, is_crisis_service: false },
        )
        .unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();

        let plan2 = create_draft_from_current(&conn, &patient_id).unwrap();
        let copied = list_contacts(&conn, &plan2.id).unwrap();
        assert_eq!(copied.len(), 2, "los dos contactos de la vigente deben copiarse");

        assert_eq!(copied[0].name, "Amiga cercana");
        assert_eq!(copied[0].contact_type, "distraction_person");
        assert_ne!(copied[0].id, c1.id, "el contacto copiado nunca comparte fila con el original");

        assert_eq!(copied[1].name, "Psiquiatra tratante");
        assert_eq!(copied[1].contact_type, "professional");
        assert_eq!(copied[1].relationship_or_role.as_deref(), Some("Psiquiatra"));
        assert_eq!(copied[1].phone.as_deref(), Some("+56900000002"));
        assert_eq!(copied[1].notes.as_deref(), Some("Nota"));
        assert_ne!(copied[1].id, c2.id);
    }

    #[test]
    fn create_draft_from_current_with_no_contacts_yields_an_empty_list() {
        let conn = test_conn("from-current-no-contacts");
        let patient_id = create_test_patient(&conn, "Paciente Actualizar Tres");
        let plan1 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();

        let plan2 = create_draft_from_current(&conn, &patient_id).unwrap();
        assert!(list_contacts(&conn, &plan2.id).unwrap().is_empty());
    }

    #[test]
    fn editing_contacts_on_the_new_draft_never_touches_the_current_plans_contacts() {
        let conn = test_conn("from-current-isolation");
        let patient_id = create_test_patient(&conn, "Paciente Actualizar Cuatro");
        let plan1 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let original_contact = add_contact(&conn, &plan1.id, support_contact()).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();

        let plan2 = create_draft_from_current(&conn, &patient_id).unwrap();
        let copied_contacts = list_contacts(&conn, &plan2.id).unwrap();
        let copied_contact = &copied_contacts[0];

        // Editar el contacto copiado en v2 nunca debe alterar el original de v1.
        update_contact(&conn, &copied_contact.id, SafetyPlanContactInput { contact_type: "professional".to_string(), name: "Nombre cambiado en v2".to_string(), relationship_or_role: None, phone: None, notes: None, address: None, service_phone: None, is_emergency_contact: false, is_crisis_service: false }).unwrap();
        let original_after_edit = list_contacts(&conn, &plan1.id).unwrap();
        assert_eq!(original_after_edit[0].id, original_contact.id);
        assert_eq!(original_after_edit[0].name, "Amiga cercana", "editar v2 no debe alterar el contacto de v1");

        // Eliminar el contacto copiado en v2 nunca debe eliminar el de v1.
        delete_contact(&conn, &copied_contact.id).unwrap();
        assert_eq!(list_contacts(&conn, &plan1.id).unwrap().len(), 1, "borrar en v2 no debe borrar el contacto de v1");
    }

    #[test]
    fn confirming_the_new_draft_never_alters_the_superseded_plans_contacts() {
        let conn = test_conn("from-current-confirm-isolation");
        let patient_id = create_test_patient(&conn, "Paciente Actualizar Cinco");
        let plan1 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        add_contact(&conn, &plan1.id, support_contact()).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();

        let plan2 = create_draft_from_current(&conn, &patient_id).unwrap();
        confirm_draft(&conn, &plan2.id).unwrap();

        // El plan1, ahora reemplazado, conserva su propio contacto intacto.
        let plan1_contacts = list_contacts(&conn, &plan1.id).unwrap();
        assert_eq!(plan1_contacts.len(), 1);
        assert_eq!(plan1_contacts[0].name, "Amiga cercana");

        let plan2_contacts = list_contacts(&conn, &plan2.id).unwrap();
        assert_eq!(plan2_contacts.len(), 1);
    }

    #[test]
    fn rejects_create_draft_from_current_when_there_is_no_current_plan() {
        let conn = test_conn("from-current-no-current-plan");
        let patient_id = create_test_patient(&conn, "Paciente Sin Vigente");
        let err = create_draft_from_current(&conn, &patient_id).unwrap_err();
        assert!(matches!(err, SafetyPlanError::NoCurrentPlan));
    }

    #[test]
    fn rejects_create_draft_from_current_for_an_archived_patient() {
        let conn = test_conn("from-current-archived");
        let patient_id = create_test_patient(&conn, "Paciente Actualizar Archivado");
        let plan1 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();

        let err = create_draft_from_current(&conn, &patient_id).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PatientArchived));
    }

    #[test]
    fn rejects_create_draft_from_current_when_a_draft_already_exists() {
        let conn = test_conn("from-current-already-has-draft");
        let patient_id = create_test_patient(&conn, "Paciente Actualizar Con Borrador");
        let plan1 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();
        create_draft(&conn, &patient_id, empty_input()).unwrap();

        let err = create_draft_from_current(&conn, &patient_id).unwrap_err();
        assert!(matches!(err, SafetyPlanError::AlreadyHasDraft));
    }

    #[test]
    fn create_draft_from_current_copies_list_items_with_new_ids_and_per_type_order() {
        let conn = test_conn("from-current-copies-items");
        let patient_id = create_test_patient(&conn, "Paciente Actualizar Ítems");
        let plan1 = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let w1 = add_item(&conn, &plan1.id, SafetyPlanListItemInput { item_type: "warning_sign".to_string(), content: "Aislamiento".to_string() }).unwrap();
        add_item(&conn, &plan1.id, SafetyPlanListItemInput { item_type: "warning_sign".to_string(), content: "Insomnio".to_string() }).unwrap();
        add_item(&conn, &plan1.id, SafetyPlanListItemInput { item_type: "strategy".to_string(), content: "Respirar".to_string() }).unwrap();
        confirm_draft(&conn, &plan1.id).unwrap();

        let plan2 = create_draft_from_current(&conn, &patient_id).unwrap();
        let copied = list_items(&conn, &plan2.id).unwrap();
        assert_eq!(copied.len(), 3);
        assert!(copied.iter().all(|i| i.id != w1.id));

        let warnings: Vec<_> = copied.iter().filter(|i| i.item_type == "warning_sign").collect();
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].content, "Aislamiento");
        assert_eq!(warnings[1].content, "Insomnio");
    }

    /// Compatibilidad post-Fase 19: `support_person` ya no es creable desde
    /// `add_contact` (rechazado con `InvalidContactType`), pero un contacto
    /// de ese tipo insertado antes del rediseño (aquí, directamente vía el
    /// repositorio, simulando una fila preexistente) sigue siendo legible y
    /// editable — incluso cambiándole solo el teléfono sin tocar su tipo.
    #[test]
    fn rejects_creating_a_support_person_contact_but_still_allows_editing_a_pre_existing_one() {
        let conn = test_conn("legacy-support-person");
        let patient_id = create_test_patient(&conn, "Paciente Legado Support Person");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();

        let mut new_type = support_contact();
        new_type.contact_type = "support_person".to_string();
        let err = add_contact(&conn, &plan.id, new_type).unwrap_err();
        assert!(matches!(err, SafetyPlanError::InvalidContactType(t) if t == "support_person"));

        let legacy_id = "legacy-c1";
        crate::repositories::safety_plans::insert_contact(
            &conn,
            &crate::repositories::safety_plans::NewSafetyPlanContactRow {
                id: legacy_id,
                safety_plan_id: &plan.id,
                contact_type: "support_person",
                name: "Amiga de antes del rediseño",
                relationship_or_role: None,
                phone: Some("+56900000000"),
                notes: None,
                address: None,
                service_phone: None,
                is_emergency_contact: false,
                is_crisis_service: false,
                sort_order: 0,
            },
        )
        .unwrap();

        let mut edit = support_contact();
        edit.contact_type = "support_person".to_string();
        edit.phone = Some("+56911111111".to_string());
        let updated = update_contact(&conn, legacy_id, edit).unwrap();
        assert_eq!(updated.contact_type, "support_person");
        assert_eq!(updated.phone.as_deref(), Some("+56911111111"));
    }

    // ---- ítems de lista (Paso 1/2/3-lugares) ----

    #[test]
    fn adds_and_lists_items_ordered_per_type() {
        let conn = test_conn("items-add-list");
        let patient_id = create_test_patient(&conn, "Paciente Ítems Uno");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "warning_sign".to_string(), content: "Aislamiento".to_string() }).unwrap();
        add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "strategy".to_string(), content: "Salir a caminar".to_string() }).unwrap();
        add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "warning_sign".to_string(), content: "Insomnio".to_string() }).unwrap();

        let items = list_items(&conn, &plan.id).unwrap();
        assert_eq!(items.len(), 3);
        let warnings: Vec<_> = items.iter().filter(|i| i.item_type == "warning_sign").collect();
        assert_eq!(warnings[0].content, "Aislamiento");
        assert_eq!(warnings[1].content, "Insomnio");
    }

    #[test]
    fn rejects_an_invalid_item_type() {
        let conn = test_conn("items-invalid-type");
        let patient_id = create_test_patient(&conn, "Paciente Ítems Dos");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let err = add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "inventado".to_string(), content: "Algo".to_string() }).unwrap_err();
        assert!(matches!(err, SafetyPlanError::InvalidItemType(t) if t == "inventado"));
    }

    #[test]
    fn rejects_a_blank_item_content() {
        let conn = test_conn("items-blank-content");
        let patient_id = create_test_patient(&conn, "Paciente Ítems Tres");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let err = add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "warning_sign".to_string(), content: "   ".to_string() }).unwrap_err();
        assert!(matches!(err, SafetyPlanError::MissingItemContent));
    }

    #[test]
    fn rejects_adding_an_item_to_a_confirmed_plan() {
        let conn = test_conn("items-confirmed-plan");
        let patient_id = create_test_patient(&conn, "Paciente Ítems Cuatro");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        confirm_draft(&conn, &plan.id).unwrap();
        let err = add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "warning_sign".to_string(), content: "Algo".to_string() }).unwrap_err();
        assert!(matches!(err, SafetyPlanError::NotEditable));
    }

    #[test]
    fn rejects_adding_an_item_after_the_patient_is_archived() {
        let conn = test_conn("items-archived-add");
        let patient_id = create_test_patient(&conn, "Paciente Ítems Archivado");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        patients_service::archive_patient(&conn, &patient_id).unwrap();
        let err = add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "warning_sign".to_string(), content: "Algo".to_string() }).unwrap_err();
        assert!(matches!(err, SafetyPlanError::PatientArchived));
    }

    #[test]
    fn updates_and_deletes_an_item_on_a_draft() {
        let conn = test_conn("items-update-delete");
        let patient_id = create_test_patient(&conn, "Paciente Ítems Cinco");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let item = add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "strategy".to_string(), content: "Original".to_string() }).unwrap();

        let updated = update_item(&conn, &item.id, "Editado").unwrap();
        assert_eq!(updated.content, "Editado");
        assert_eq!(updated.item_type, "strategy", "editar el contenido nunca cambia el tipo del ítem");

        delete_item(&conn, &item.id).unwrap();
        assert!(list_items(&conn, &plan.id).unwrap().is_empty());
    }

    #[test]
    fn rejects_updating_an_item_with_blank_content() {
        let conn = test_conn("items-update-blank");
        let patient_id = create_test_patient(&conn, "Paciente Ítems Seis");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let item = add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "strategy".to_string(), content: "Original".to_string() }).unwrap();
        let err = update_item(&conn, &item.id, "   ").unwrap_err();
        assert!(matches!(err, SafetyPlanError::MissingItemContent));
    }

    #[test]
    fn rejects_editing_and_deleting_an_item_of_an_already_confirmed_plan() {
        let conn = test_conn("items-edit-confirmed");
        let patient_id = create_test_patient(&conn, "Paciente Ítems Siete");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        let item = add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "strategy".to_string(), content: "Original".to_string() }).unwrap();
        confirm_draft(&conn, &plan.id).unwrap();

        let err = update_item(&conn, &item.id, "Editado").unwrap_err();
        assert!(matches!(err, SafetyPlanError::NotEditable));
        let err = delete_item(&conn, &item.id).unwrap_err();
        assert!(matches!(err, SafetyPlanError::NotEditable));
    }

    #[test]
    fn a_confirmed_plans_items_are_still_readable() {
        let conn = test_conn("items-readable-confirmed");
        let patient_id = create_test_patient(&conn, "Paciente Ítems Ocho");
        let plan = create_draft(&conn, &patient_id, empty_input()).unwrap();
        add_item(&conn, &plan.id, SafetyPlanListItemInput { item_type: "distraction_place".to_string(), content: "Parque cercano".to_string() }).unwrap();
        confirm_draft(&conn, &plan.id).unwrap();

        let items = list_items(&conn, &plan.id).unwrap();
        assert_eq!(items.len(), 1, "confirmar el plan no debe ocultar ni borrar sus ítems");
    }
}
