//! Reglas de negocio de pagos / cobros internos (Fase 7). Vertical
//! administrativa, deliberadamente separada de todo lo clínico — nunca
//! importa nada de `patient_clinical_profile`, `session_notes` ni
//! `therapeutic_goals`.
//!
//! Semántica de estados (autoritativa aquí, nunca solo en el formulario):
//!
//! - `pendiente`: `paid_at` debe ser `None`.
//! - `pagado`: `paid_at` es obligatorio; `method` es obligatorio.
//! - `atrasado`: mismo requisito que `pendiente` (`paid_at` debe ser
//!   `None`) — es un valor persistible válido según `SCHEMA_V1`, pero el
//!   flujo normal de esta fase **nunca lo escribe automáticamente**. Un
//!   pago que sigue `pendiente` y cuya `due_date` ya pasó se **deriva**
//!   como atrasado en el momento de leer (`repositories::payments::Payment::is_overdue`,
//!   calculado en SQL vía `date('now')`, UTC) — nunca se reescribe la
//!   columna `status`. Esto evita depender de que la usuaria recuerde
//!   marcar cada pago vencido, y evita que dos fuentes de verdad
//!   (`status` persistido vs. estado derivado) puedan quedar
//!   contradictorias entre sí: solo existe una, `status`, y `is_overdue`
//!   es una etiqueta de presentación calculada aparte, nunca un segundo
//!   estado. Limitación conocida y documentada: `date('now')` es UTC, así
//!   que en el límite exacto de medianoche local puede haber un desfase
//!   de un día — aceptable para una fecha administrativa sin hora.
//! - `condonado`: `paid_at` puede ser `None` o tener valor; `amount` puede
//!   ser `0` o mantener el monto original de la deuda condonada.
//!
//! Reglas de monto: `amount < 0` siempre inválido (además del `CHECK` de
//! SQLite, se repite aquí para un error de dominio claro); `amount == 0`
//! solo es válido si `status == 'condonado'`. Esta fase solo admite
//! `currency == 'CLP'` (el schema no lo restringe, pero no se diseñó ni se
//! pidió soporte multi-moneda) y exige que el monto sea un número entero
//! para CLP — sin decimales, sin cambiar el tipo `REAL` de la columna.
//!
//! `method` es obligatorio únicamente cuando `status == 'pagado'` —
//! opcional en cualquier otro estado, incluido `condonado`.

use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;

use crate::repositories::patients;
use crate::repositories::payments::{self, NewPaymentRow, Payment, PaymentDashboardSummary, PaymentListItem, PaymentUpdateRow};
use crate::repositories::sessions;

pub const VALID_METHODS: &[&str] = &["efectivo", "transferencia", "tarjeta", "otro"];
pub const VALID_STATUSES: &[&str] = &["pendiente", "pagado", "atrasado", "condonado"];
const DEFAULT_STATUS: &str = "pendiente";
/// Única moneda soportada en esta fase. El schema no la restringe (no hay
/// `CHECK` sobre `currency`), pero no se diseñó ni se aprobó soporte
/// multi-moneda — se rechaza cualquier otro valor explícitamente en vez de
/// aceptarlo en silencio.
const SUPPORTED_CURRENCY: &str = "CLP";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentInput {
    pub patient_id: String,
    pub session_id: Option<String>,
    pub amount: f64,
    pub currency: Option<String>,
    pub method: Option<String>,
    pub status: Option<String>,
    pub due_date: Option<String>,
    pub paid_at: Option<String>,
    pub notes: Option<String>,
}

/// Deliberadamente sin `patient_id` — reasignar un pago a otro paciente no
/// es una operación de este MVP, mismo criterio que `GoalUpdateInput`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentUpdateInput {
    pub session_id: Option<String>,
    pub amount: f64,
    pub currency: Option<String>,
    pub method: Option<String>,
    pub status: String,
    pub due_date: Option<String>,
    pub paid_at: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug)]
pub enum PaymentValidationError {
    NegativeAmount,
    ZeroAmountRequiresCondoned,
    UnsupportedCurrency(String),
    AmountMustBeIntegerForClp,
    InvalidMethod(String),
    MethodRequiredWhenPaid,
    InvalidStatus(String),
    PaidAtRequiredWhenPaid,
    PaidAtMustBeAbsentUnlessPaidOrCondoned,
    InvalidDate { field: &'static str },
}

impl fmt::Display for PaymentValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaymentValidationError::NegativeAmount => write!(f, "el monto no puede ser negativo"),
            PaymentValidationError::ZeroAmountRequiresCondoned => write!(f, "un monto de 0 solo es válido si el estado es 'condonado'"),
            PaymentValidationError::UnsupportedCurrency(c) => write!(f, "moneda no soportada: '{c}' (solo se admite CLP en esta fase)"),
            PaymentValidationError::AmountMustBeIntegerForClp => write!(f, "el monto en CLP debe ser un número entero, sin decimales"),
            PaymentValidationError::InvalidMethod(m) => {
                write!(f, "método de pago inválido: '{m}' (debe ser uno de: {})", VALID_METHODS.join(", "))
            }
            PaymentValidationError::MethodRequiredWhenPaid => write!(f, "el método de pago es obligatorio cuando el estado es 'pagado'"),
            PaymentValidationError::InvalidStatus(s) => {
                write!(f, "estado inválido: '{s}' (debe ser uno de: {})", VALID_STATUSES.join(", "))
            }
            PaymentValidationError::PaidAtRequiredWhenPaid => write!(f, "la fecha de pago es obligatoria cuando el estado es 'pagado'"),
            PaymentValidationError::PaidAtMustBeAbsentUnlessPaidOrCondoned => {
                write!(f, "la fecha de pago solo puede informarse en estado 'pagado' o 'condonado'")
            }
            PaymentValidationError::InvalidDate { field } => {
                write!(f, "fecha inválida en '{field}' (formato esperado: AAAA-MM-DD)")
            }
        }
    }
}
impl std::error::Error for PaymentValidationError {}

#[derive(Debug)]
pub enum PaymentError {
    Validation(PaymentValidationError),
    NotFound,
    PatientNotFound,
    PatientArchived,
    SessionNotFound,
    PatientMismatch,
    Database(rusqlite::Error),
}

impl fmt::Display for PaymentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaymentError::Validation(e) => write!(f, "{e}"),
            PaymentError::NotFound => write!(f, "pago no encontrado"),
            PaymentError::PatientNotFound => write!(f, "paciente no encontrado"),
            PaymentError::PatientArchived => write!(f, "no se pueden registrar pagos nuevos para un paciente archivado"),
            PaymentError::SessionNotFound => write!(f, "sesión no encontrada"),
            PaymentError::PatientMismatch => write!(f, "la sesión indicada pertenece a otro paciente"),
            // Nunca se interpola el error de rusqlite (podría incluir valores).
            PaymentError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
        }
    }
}
impl std::error::Error for PaymentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PaymentError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for PaymentError {
    fn from(e: rusqlite::Error) -> Self {
        PaymentError::Database(e)
    }
}
impl From<PaymentValidationError> for PaymentError {
    fn from(e: PaymentValidationError) -> Self {
        PaymentError::Validation(e)
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Mismo formato y misma validación estructural (no calendárica) que
/// `services::goals::validate_date_format` / `services::patients` —
/// AAAA-MM-DD.
fn validate_date_format(value: &str, field: &'static str) -> Result<(), PaymentValidationError> {
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
        Err(PaymentValidationError::InvalidDate { field })
    }
}

struct ValidatedPaymentFields {
    session_id: Option<String>,
    amount: f64,
    currency: String,
    method: Option<String>,
    status: String,
    due_date: Option<String>,
    paid_at: Option<String>,
    notes: Option<String>,
}

/// Validación autoritativa, común a creación y edición. El frontend valida
/// para UX; esta función es la única que realmente decide.
#[allow(clippy::too_many_arguments)]
fn validate(
    session_id: Option<String>,
    amount: f64,
    currency: Option<String>,
    method: Option<String>,
    status: Option<String>,
    due_date: Option<String>,
    paid_at: Option<String>,
    notes: Option<String>,
) -> Result<ValidatedPaymentFields, PaymentValidationError> {
    let status = status.unwrap_or_else(|| DEFAULT_STATUS.to_string());
    if !VALID_STATUSES.contains(&status.as_str()) {
        return Err(PaymentValidationError::InvalidStatus(status));
    }

    if amount < 0.0 {
        return Err(PaymentValidationError::NegativeAmount);
    }
    if amount == 0.0 && status != "condonado" {
        return Err(PaymentValidationError::ZeroAmountRequiresCondoned);
    }

    let currency = none_if_blank(currency).unwrap_or_else(|| SUPPORTED_CURRENCY.to_string());
    if currency != SUPPORTED_CURRENCY {
        return Err(PaymentValidationError::UnsupportedCurrency(currency));
    }
    if amount.fract() != 0.0 {
        return Err(PaymentValidationError::AmountMustBeIntegerForClp);
    }

    let method = none_if_blank(method);
    if let Some(ref m) = method {
        if !VALID_METHODS.contains(&m.as_str()) {
            return Err(PaymentValidationError::InvalidMethod(m.clone()));
        }
    }
    if status == "pagado" && method.is_none() {
        return Err(PaymentValidationError::MethodRequiredWhenPaid);
    }

    let paid_at = none_if_blank(paid_at);
    if let Some(ref d) = paid_at {
        validate_date_format(d, "paidAt")?;
    }
    match status.as_str() {
        "pagado" if paid_at.is_none() => return Err(PaymentValidationError::PaidAtRequiredWhenPaid),
        "pendiente" | "atrasado" if paid_at.is_some() => {
            return Err(PaymentValidationError::PaidAtMustBeAbsentUnlessPaidOrCondoned)
        }
        _ => {}
    }

    let due_date = none_if_blank(due_date);
    if let Some(ref d) = due_date {
        validate_date_format(d, "dueDate")?;
    }

    Ok(ValidatedPaymentFields { session_id: none_if_blank(session_id), amount, currency, method, status, due_date, paid_at, notes: none_if_blank(notes) })
}

/// Si `session_id` viene informado, comprueba que la sesión exista y que
/// pertenezca al mismo paciente del pago — nunca se confía solo en el
/// `patientId` enviado por React. Mismo patrón que
/// `services::goals::link_session_goal`.
fn check_session_belongs_to_patient(conn: &Connection, session_id: &Option<String>, patient_id: &str) -> Result<(), PaymentError> {
    if let Some(session_id) = session_id {
        let session = sessions::find_by_id(conn, session_id)?.ok_or(PaymentError::SessionNotFound)?;
        if session.patient_id != patient_id {
            return Err(PaymentError::PatientMismatch);
        }
    }
    Ok(())
}

/// Rechaza la creación para un paciente inexistente o archivado — mismo
/// criterio que `services::goals::create_goal` /
/// `services::patient_clinical_profile::create_clinical_profile`. Editar un
/// pago histórico de un paciente archivado sigue permitido
/// (`update_payment` no repite esta comprobación).
pub fn create_payment(conn: &Connection, input: PaymentInput) -> Result<Payment, PaymentError> {
    let patient = patients::find_by_id(conn, &input.patient_id)?.ok_or(PaymentError::PatientNotFound)?;
    if patient.deleted_at.is_some() {
        return Err(PaymentError::PatientArchived);
    }

    let f = validate(input.session_id, input.amount, input.currency, input.method, input.status, input.due_date, input.paid_at, input.notes)?;
    check_session_belongs_to_patient(conn, &f.session_id, &input.patient_id)?;

    let id = uuid::Uuid::new_v4().to_string();
    Ok(payments::insert(
        conn,
        &NewPaymentRow {
            id: &id,
            patient_id: &input.patient_id,
            session_id: f.session_id.as_deref(),
            amount: f.amount,
            currency: &f.currency,
            method: f.method.as_deref(),
            status: &f.status,
            due_date: f.due_date.as_deref(),
            paid_at: f.paid_at.as_deref(),
            notes: f.notes.as_deref(),
        },
    )?)
}

pub fn get_payment(conn: &Connection, id: &str) -> Result<Payment, PaymentError> {
    payments::find_by_id(conn, id)?.ok_or(PaymentError::NotFound)
}

pub fn list_payments(conn: &Connection, patient_id: &str) -> Result<Vec<PaymentListItem>, PaymentError> {
    Ok(payments::list_active_by_patient(conn, patient_id)?)
}

pub fn list_archived_payments(conn: &Connection, patient_id: &str) -> Result<Vec<PaymentListItem>, PaymentError> {
    Ok(payments::list_archived_by_patient(conn, patient_id)?)
}

/// Edita un pago existente. Deliberadamente **no** vuelve a comprobar si el
/// paciente está archivado — corregir datos administrativos de un pago
/// histórico está permitido incluso con el paciente archivado (regla 9 de
/// la aprobación de Fase 7). Si `session_id` cambia, sí se vuelve a
/// verificar `session.patient_id == payment.patient_id`.
pub fn update_payment(conn: &Connection, id: &str, input: PaymentUpdateInput) -> Result<Payment, PaymentError> {
    let existing = payments::find_by_id(conn, id)?.ok_or(PaymentError::NotFound)?;
    let f = validate(input.session_id, input.amount, input.currency, input.method, Some(input.status), input.due_date, input.paid_at, input.notes)?;
    check_session_belongs_to_patient(conn, &f.session_id, &existing.patient_id)?;

    let row = PaymentUpdateRow {
        session_id: f.session_id.as_deref(),
        amount: f.amount,
        currency: &f.currency,
        method: f.method.as_deref(),
        status: &f.status,
        due_date: f.due_date.as_deref(),
        paid_at: f.paid_at.as_deref(),
        notes: f.notes.as_deref(),
    };
    payments::update(conn, id, &row)?.ok_or(PaymentError::NotFound)
}

/// Soft delete únicamente. No existe, en ningún punto de este servicio ni
/// del repositorio, una operación de borrado físico alcanzable desde un
/// comando normal.
pub fn archive_payment(conn: &Connection, id: &str) -> Result<(), PaymentError> {
    if payments::soft_delete(conn, id)? {
        Ok(())
    } else {
        Err(PaymentError::NotFound)
    }
}

pub fn restore_payment(conn: &Connection, id: &str) -> Result<Payment, PaymentError> {
    if payments::restore(conn, id)? {
        get_payment(conn, id)
    } else {
        Err(PaymentError::NotFound)
    }
}

/// Agregados para el Dashboard — ver
/// `repositories::payments::dashboard_summary` para el alcance exacto.
pub fn payment_dashboard_summary(conn: &Connection) -> Result<PaymentDashboardSummary, PaymentError> {
    Ok(payments::dashboard_summary(conn)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::services::patients::{self, PatientInput};
    use crate::services::sessions::{self as session_service, SessionInput};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-payments-service-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x52u8; VAULT_KEY_LEN]);
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

    fn create_test_session(conn: &Connection, patient_id: &str) -> String {
        let input = SessionInput {
            patient_id: patient_id.to_string(),
            appointment_id: None,
            session_date: "2026-09-01".to_string(),
            start_time: Some("15:00".to_string()),
            duration_minutes: Some(50),
            modality: Some("presencial".to_string()),
        };
        session_service::create_session(conn, input).unwrap().session.id
    }

    fn minimal_input(patient_id: &str) -> PaymentInput {
        PaymentInput {
            patient_id: patient_id.to_string(),
            session_id: None,
            amount: 40000.0,
            currency: None,
            method: None,
            status: None,
            due_date: None,
            paid_at: None,
            notes: None,
        }
    }

    // ---- creación ----

    #[test]
    fn creates_a_pending_payment_with_defaults() {
        let conn = test_conn("create-defaults");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let p = create_payment(&conn, minimal_input(&patient_id)).unwrap();
        assert_eq!(p.status, "pendiente");
        assert_eq!(p.currency, "CLP");
        assert_eq!(p.amount, 40000.0);
        assert!(!p.is_overdue);
    }

    #[test]
    fn rejects_creation_for_a_nonexistent_patient() {
        let conn = test_conn("create-nonexistent-patient");
        let err = create_payment(&conn, minimal_input("no-existe")).unwrap_err();
        assert!(matches!(err, PaymentError::PatientNotFound));
    }

    #[test]
    fn rejects_creation_for_an_archived_patient() {
        let conn = test_conn("create-archived-patient");
        let patient_id = create_test_patient(&conn, "Paciente Archivado");
        patients::archive_patient(&conn, &patient_id).unwrap();
        let err = create_payment(&conn, minimal_input(&patient_id)).unwrap_err();
        assert!(matches!(err, PaymentError::PatientArchived));
    }

    #[test]
    fn editing_a_historical_payment_of_an_archived_patient_is_allowed() {
        let conn = test_conn("edit-archived-patient-payment");
        let patient_id = create_test_patient(&conn, "Paciente Historico");
        let created = create_payment(&conn, minimal_input(&patient_id)).unwrap();
        patients::archive_patient(&conn, &patient_id).unwrap();

        let updated = update_payment(
            &conn,
            &created.id,
            PaymentUpdateInput { session_id: None, amount: 40000.0, currency: None, method: None, status: "pendiente".to_string(), due_date: None, paid_at: None, notes: Some("Corrección administrativa".to_string()) },
        )
        .unwrap();
        assert_eq!(updated.notes.as_deref(), Some("Corrección administrativa"));
    }

    #[test]
    fn rejects_a_session_belonging_to_another_patient() {
        let conn = test_conn("session-other-patient");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        let session_a = create_test_session(&conn, &patient_a);

        let mut input = minimal_input(&patient_b);
        input.session_id = Some(session_a);
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::PatientMismatch));
    }

    #[test]
    fn rejects_a_nonexistent_session() {
        let conn = test_conn("session-nonexistent");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let mut input = minimal_input(&patient_id);
        input.session_id = Some("no-existe".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::SessionNotFound));
    }

    #[test]
    fn accepts_a_payment_linked_to_the_correct_patients_session() {
        let conn = test_conn("session-correct-patient");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let session_id = create_test_session(&conn, &patient_id);
        let mut input = minimal_input(&patient_id);
        input.session_id = Some(session_id.clone());
        let p = create_payment(&conn, input).unwrap();
        assert_eq!(p.session_id.as_deref(), Some(session_id.as_str()));
    }

    // ---- monto ----

    #[test]
    fn rejects_a_negative_amount() {
        let conn = test_conn("negative-amount");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        let mut input = minimal_input(&patient_id);
        input.amount = -100.0;
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::NegativeAmount)));
    }

    #[test]
    fn rejects_zero_amount_when_not_condoned() {
        let conn = test_conn("zero-amount-not-condoned");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let mut input = minimal_input(&patient_id);
        input.amount = 0.0;
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::ZeroAmountRequiresCondoned)));
    }

    #[test]
    fn accepts_zero_amount_when_condoned() {
        let conn = test_conn("zero-amount-condoned");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        let mut input = minimal_input(&patient_id);
        input.amount = 0.0;
        input.status = Some("condonado".to_string());
        let p = create_payment(&conn, input).unwrap();
        assert_eq!(p.amount, 0.0);
        assert_eq!(p.status, "condonado");
    }

    #[test]
    fn accepts_condoned_with_the_original_positive_amount() {
        let conn = test_conn("condoned-positive-amount");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        let mut input = minimal_input(&patient_id);
        input.amount = 40000.0;
        input.status = Some("condonado".to_string());
        let p = create_payment(&conn, input).unwrap();
        assert_eq!(p.amount, 40000.0);
        assert_eq!(p.status, "condonado");
    }

    #[test]
    fn rejects_a_decimal_amount_for_clp() {
        let conn = test_conn("clp-decimal-rejected");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let mut input = minimal_input(&patient_id);
        input.amount = 40000.5;
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::AmountMustBeIntegerForClp)));
    }

    #[test]
    fn accepts_an_integer_amount_for_clp() {
        let conn = test_conn("clp-integer-accepted");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let mut input = minimal_input(&patient_id);
        input.amount = 40500.0;
        let p = create_payment(&conn, input).unwrap();
        assert_eq!(p.amount, 40500.0);
    }

    #[test]
    fn rejects_an_unsupported_currency() {
        let conn = test_conn("unsupported-currency");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let mut input = minimal_input(&patient_id);
        input.currency = Some("USD".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::UnsupportedCurrency(_))));
    }

    // ---- método ----

    #[test]
    fn rejects_an_invalid_method() {
        let conn = test_conn("invalid-method");
        let patient_id = create_test_patient(&conn, "Paciente Once");
        let mut input = minimal_input(&patient_id);
        input.method = Some("bitcoin".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::InvalidMethod(_))));
    }

    #[test]
    fn rejects_marking_paid_without_a_method() {
        let conn = test_conn("paid-without-method");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        let mut input = minimal_input(&patient_id);
        input.status = Some("pagado".to_string());
        input.paid_at = Some("2026-09-01".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::MethodRequiredWhenPaid)));
    }

    #[test]
    fn accepts_a_pending_payment_without_a_method() {
        let conn = test_conn("pending-without-method");
        let patient_id = create_test_patient(&conn, "Paciente Trece");
        let p = create_payment(&conn, minimal_input(&patient_id)).unwrap();
        assert!(p.method.is_none());
    }

    #[test]
    fn accepts_a_condoned_payment_without_a_method() {
        let conn = test_conn("condoned-without-method");
        let patient_id = create_test_patient(&conn, "Paciente Catorce");
        let mut input = minimal_input(&patient_id);
        input.status = Some("condonado".to_string());
        let p = create_payment(&conn, input).unwrap();
        assert!(p.method.is_none());
    }

    #[test]
    fn accepts_a_paid_payment_with_method_and_paid_at() {
        let conn = test_conn("paid-with-method");
        let patient_id = create_test_patient(&conn, "Paciente Quince");
        let mut input = minimal_input(&patient_id);
        input.status = Some("pagado".to_string());
        input.method = Some("efectivo".to_string());
        input.paid_at = Some("2026-09-01".to_string());
        let p = create_payment(&conn, input).unwrap();
        assert_eq!(p.status, "pagado");
        assert_eq!(p.method.as_deref(), Some("efectivo"));
        assert_eq!(p.paid_at.as_deref(), Some("2026-09-01"));
    }

    // ---- paid_at / status ----

    #[test]
    fn rejects_paid_status_without_paid_at() {
        let conn = test_conn("paid-without-paid-at");
        let patient_id = create_test_patient(&conn, "Paciente Dieciséis");
        let mut input = minimal_input(&patient_id);
        input.status = Some("pagado".to_string());
        input.method = Some("efectivo".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::PaidAtRequiredWhenPaid)));
    }

    #[test]
    fn rejects_paid_at_on_a_pending_payment() {
        let conn = test_conn("paid-at-on-pending");
        let patient_id = create_test_patient(&conn, "Paciente Diecisiete");
        let mut input = minimal_input(&patient_id);
        input.paid_at = Some("2026-09-01".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::PaidAtMustBeAbsentUnlessPaidOrCondoned)));
    }

    #[test]
    fn rejects_paid_at_on_an_explicitly_atrasado_payment() {
        let conn = test_conn("paid-at-on-atrasado");
        let patient_id = create_test_patient(&conn, "Paciente Dieciocho");
        let mut input = minimal_input(&patient_id);
        input.status = Some("atrasado".to_string());
        input.paid_at = Some("2026-09-01".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::PaidAtMustBeAbsentUnlessPaidOrCondoned)));
    }

    #[test]
    fn accepts_condoned_with_a_paid_at() {
        let conn = test_conn("condoned-with-paid-at");
        let patient_id = create_test_patient(&conn, "Paciente Diecinueve");
        let mut input = minimal_input(&patient_id);
        input.status = Some("condonado".to_string());
        input.paid_at = Some("2026-09-01".to_string());
        let p = create_payment(&conn, input).unwrap();
        assert_eq!(p.paid_at.as_deref(), Some("2026-09-01"));
    }

    #[test]
    fn accepts_condoned_without_a_paid_at() {
        let conn = test_conn("condoned-without-paid-at");
        let patient_id = create_test_patient(&conn, "Paciente Veinte");
        let mut input = minimal_input(&patient_id);
        input.status = Some("condonado".to_string());
        let p = create_payment(&conn, input).unwrap();
        assert!(p.paid_at.is_none());
    }

    #[test]
    fn rejects_an_invalid_status() {
        let conn = test_conn("invalid-status");
        let patient_id = create_test_patient(&conn, "Paciente Veintiuno");
        let mut input = minimal_input(&patient_id);
        input.status = Some("cancelado".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::InvalidStatus(_))));
    }

    // ---- fechas ----

    #[test]
    fn rejects_an_invalid_due_date() {
        let conn = test_conn("invalid-due-date");
        let patient_id = create_test_patient(&conn, "Paciente Veintidós");
        let mut input = minimal_input(&patient_id);
        input.due_date = Some("no-es-fecha".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::InvalidDate { field: "dueDate" })));
    }

    #[test]
    fn rejects_an_invalid_paid_at() {
        let conn = test_conn("invalid-paid-at");
        let patient_id = create_test_patient(&conn, "Paciente Veintitrés");
        let mut input = minimal_input(&patient_id);
        input.status = Some("pagado".to_string());
        input.method = Some("efectivo".to_string());
        input.paid_at = Some("31-02-2026".to_string());
        let err = create_payment(&conn, input).unwrap_err();
        assert!(matches!(err, PaymentError::Validation(PaymentValidationError::InvalidDate { field: "paidAt" })));
    }

    // ---- atrasado derivado ----

    #[test]
    fn a_pending_payment_past_due_date_is_shown_as_overdue_but_status_stays_pendiente() {
        let conn = test_conn("overdue-derived");
        let patient_id = create_test_patient(&conn, "Paciente Veinticuatro");
        let mut input = minimal_input(&patient_id);
        input.due_date = Some("2000-01-01".to_string());
        let p = create_payment(&conn, input).unwrap();
        assert_eq!(p.status, "pendiente");
        assert!(p.is_overdue);

        let listed = list_payments(&conn, &patient_id).unwrap();
        assert_eq!(listed[0].status, "pendiente");
        assert!(listed[0].is_overdue);
    }

    #[test]
    fn a_pending_payment_with_a_future_due_date_is_not_overdue() {
        let conn = test_conn("not-overdue-future-service");
        let patient_id = create_test_patient(&conn, "Paciente Veinticinco");
        let mut input = minimal_input(&patient_id);
        input.due_date = Some("2999-01-01".to_string());
        let p = create_payment(&conn, input).unwrap();
        assert!(!p.is_overdue);
    }

    #[test]
    fn a_manually_set_atrasado_payment_is_accepted_and_never_auto_reverted() {
        let conn = test_conn("manual-atrasado");
        let patient_id = create_test_patient(&conn, "Paciente Veintiséis");
        let mut input = minimal_input(&patient_id);
        input.status = Some("atrasado".to_string());
        let p = create_payment(&conn, input).unwrap();
        assert_eq!(p.status, "atrasado");
        // Sigue existiendo como "atrasado" (no se auto-revierte a "pendiente").
        assert_eq!(get_payment(&conn, &p.id).unwrap().status, "atrasado");
    }

    // ---- edición ----

    #[test]
    fn updating_a_nonexistent_payment_reports_not_found() {
        let conn = test_conn("update-nonexistent");
        let err = update_payment(
            &conn,
            "no-existe",
            PaymentUpdateInput { session_id: None, amount: 1000.0, currency: None, method: None, status: "pendiente".to_string(), due_date: None, paid_at: None, notes: None },
        )
        .unwrap_err();
        assert!(matches!(err, PaymentError::NotFound));
    }

    #[test]
    fn update_rejects_changing_the_session_to_one_of_another_patient() {
        let conn = test_conn("update-session-mismatch");
        let patient_a = create_test_patient(&conn, "Paciente Veintisiete");
        let patient_b = create_test_patient(&conn, "Paciente Veintiocho");
        let session_b = create_test_session(&conn, &patient_b);
        let created = create_payment(&conn, minimal_input(&patient_a)).unwrap();

        let err = update_payment(
            &conn,
            &created.id,
            PaymentUpdateInput { session_id: Some(session_b), amount: 40000.0, currency: None, method: None, status: "pendiente".to_string(), due_date: None, paid_at: None, notes: None },
        )
        .unwrap_err();
        assert!(matches!(err, PaymentError::PatientMismatch));
    }

    #[test]
    fn update_can_mark_a_pending_payment_as_paid() {
        let conn = test_conn("update-mark-paid");
        let patient_id = create_test_patient(&conn, "Paciente Veintinueve");
        let created = create_payment(&conn, minimal_input(&patient_id)).unwrap();

        let updated = update_payment(
            &conn,
            &created.id,
            PaymentUpdateInput { session_id: None, amount: 40000.0, currency: None, method: Some("transferencia".to_string()), status: "pagado".to_string(), due_date: None, paid_at: Some("2026-09-02".to_string()), notes: None },
        )
        .unwrap();
        assert_eq!(updated.status, "pagado");
        assert_eq!(updated.paid_at.as_deref(), Some("2026-09-02"));
    }

    // ---- archivar / restaurar ----

    #[test]
    fn archiving_hides_from_active_listing_and_restoring_brings_it_back() {
        let conn = test_conn("archive-restore");
        let patient_id = create_test_patient(&conn, "Paciente Treinta");
        let created = create_payment(&conn, minimal_input(&patient_id)).unwrap();

        archive_payment(&conn, &created.id).unwrap();
        assert!(list_payments(&conn, &patient_id).unwrap().is_empty());
        assert_eq!(list_archived_payments(&conn, &patient_id).unwrap().len(), 1);

        let restored = restore_payment(&conn, &created.id).unwrap();
        assert!(restored.deleted_at.is_none());
        assert_eq!(list_payments(&conn, &patient_id).unwrap().len(), 1);
    }

    #[test]
    fn archiving_an_unknown_payment_reports_not_found() {
        let conn = test_conn("archive-unknown");
        assert!(matches!(archive_payment(&conn, "no-existe").unwrap_err(), PaymentError::NotFound));
    }

    #[test]
    fn restoring_an_active_payment_reports_not_found() {
        let conn = test_conn("restore-active");
        let patient_id = create_test_patient(&conn, "Paciente Treinta y Uno");
        let created = create_payment(&conn, minimal_input(&patient_id)).unwrap();
        assert!(matches!(restore_payment(&conn, &created.id).unwrap_err(), PaymentError::NotFound));
    }

    #[test]
    fn archiving_an_already_archived_payment_reports_not_found() {
        let conn = test_conn("archive-twice");
        let patient_id = create_test_patient(&conn, "Paciente Treinta y Dos");
        let created = create_payment(&conn, minimal_input(&patient_id)).unwrap();
        archive_payment(&conn, &created.id).unwrap();
        assert!(matches!(archive_payment(&conn, &created.id).unwrap_err(), PaymentError::NotFound));
    }

    // ---- privacidad estructural ----

    #[test]
    fn list_item_never_carries_clinical_content() {
        // Verificación estructural: PaymentListItem no tiene ningún campo de
        // contenido clínico — ni siquiera `notes` (administrativa, se deja
        // fuera del listado igual que `GoalListItem` deja fuera `description`).
        let item = PaymentListItem {
            id: "x".into(),
            session_id: None,
            amount: 1000.0,
            currency: "CLP".into(),
            method: None,
            status: "pendiente".into(),
            due_date: None,
            paid_at: None,
            is_overdue: false,
            created_at: "2026-09-01T00:00:00.000Z".into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("notes"));
        assert!(!json.contains("diagnos"));
        assert!(!json.contains("risk"));
    }

    // ---- Dashboard ----

    #[test]
    fn dashboard_summary_reflects_created_payments() {
        let conn = test_conn("dashboard-through-service");
        let patient_id = create_test_patient(&conn, "Paciente Treinta y Tres");
        create_payment(&conn, minimal_input(&patient_id)).unwrap();

        let summary = payment_dashboard_summary(&conn).unwrap();
        assert_eq!(summary.pending_count, 1);
        assert_eq!(summary.pending_total, 40000.0);
    }
}
