//! Comandos Tauri de pagos / cobros internos. Capa fina: sin SQL, sin
//! reglas de negocio — cada uno obtiene la conexión vía
//! `VaultSession::with_connection` y delega en `services::payments`.
//!
//! Ningún pago se sincroniza jamás con Google Calendar — este archivo no
//! importa nada de `calendar::*`, y así se queda.

use std::sync::Arc;

use tauri::State;

use crate::repositories::payments::{Payment, PaymentDashboardSummary, PaymentListItem};
use crate::security::VaultSession;
use crate::services::payments::{self, PaymentInput, PaymentUpdateInput};

type SharedVaultSession = Arc<VaultSession>;

const LOCKED_MESSAGE: &str = "el vault está bloqueado";

#[tauri::command]
pub fn create_payment(input: PaymentInput, state: State<'_, SharedVaultSession>) -> Result<Payment, String> {
    state.with_connection(|conn| payments::create_payment(conn, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_payment(id: String, state: State<'_, SharedVaultSession>) -> Result<Payment, String> {
    state.with_connection(|conn| payments::get_payment(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_payments(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<PaymentListItem>, String> {
    state.with_connection(|conn| payments::list_payments(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Papelera: pagos con soft delete aplicado. Separado de `list_payments` a
/// propósito, mismo criterio que en Pacientes, Agenda, Sesiones y
/// Objetivos.
#[tauri::command]
pub fn list_archived_payments(patient_id: String, state: State<'_, SharedVaultSession>) -> Result<Vec<PaymentListItem>, String> {
    state.with_connection(|conn| payments::list_archived_payments(conn, &patient_id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_payment(id: String, input: PaymentUpdateInput, state: State<'_, SharedVaultSession>) -> Result<Payment, String> {
    state.with_connection(|conn| payments::update_payment(conn, &id, input)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Soft delete ("archivar"). No existe un comando de borrado físico.
#[tauri::command]
pub fn archive_payment(id: String, state: State<'_, SharedVaultSession>) -> Result<(), String> {
    state.with_connection(|conn| payments::archive_payment(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_payment(id: String, state: State<'_, SharedVaultSession>) -> Result<Payment, String> {
    state.with_connection(|conn| payments::restore_payment(conn, &id)).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}

/// Agregados administrativos para el Dashboard (ingresos del mes, pagos
/// pendientes) — nunca un listado de pagos individuales.
#[tauri::command]
pub fn get_payment_dashboard_summary(state: State<'_, SharedVaultSession>) -> Result<PaymentDashboardSummary, String> {
    state.with_connection(payments::payment_dashboard_summary).map_err(|_| LOCKED_MESSAGE.to_string())?.map_err(|e| e.to_string())
}
