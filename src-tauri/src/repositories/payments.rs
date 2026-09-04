//! Acceso a datos de `payments`. SQL puro — sin reglas de negocio (eso vive
//! en `services::payments`) y sin ninguna noción de si el vault está
//! desbloqueado.
//!
//! `is_overdue` es un hecho crudo calculado aquí (`status = 'pendiente' AND
//! due_date < date('now')`) — nunca se escribe de vuelta a `status`, que
//! permanece exactamente como se guardó. La decisión de negocio de qué
//! hacer con ese hecho (mostrar "atrasado" en vez de "pendiente") vive en
//! `services::payments`, no aquí. `date('now')` es una función nativa de
//! SQLite en UTC — no se agrega ninguna dependencia de fechas para esto,
//! ver `docs/payments.md` para la limitación conocida del huso horario.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

/// Ficha completa de un pago.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payment {
    pub id: String,
    pub patient_id: String,
    pub session_id: Option<String>,
    pub amount: f64,
    pub currency: String,
    pub method: Option<String>,
    pub status: String,
    pub due_date: Option<String>,
    pub paid_at: Option<String>,
    pub notes: Option<String>,
    pub is_overdue: bool,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// Fila para el listado dentro de la pestaña "Pagos" de un paciente —
/// deliberadamente sin `patient_id` (el listado ya está scoped a un
/// paciente) ni `notes` (detalle administrativo, no necesario para una
/// lista) — mismo criterio de minimización que `GoalListItem`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentListItem {
    pub id: String,
    pub session_id: Option<String>,
    pub amount: f64,
    pub currency: String,
    pub method: Option<String>,
    pub status: String,
    pub due_date: Option<String>,
    pub paid_at: Option<String>,
    pub is_overdue: bool,
    pub created_at: String,
}

pub struct NewPaymentRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub session_id: Option<&'a str>,
    pub amount: f64,
    pub currency: &'a str,
    pub method: Option<&'a str>,
    pub status: &'a str,
    pub due_date: Option<&'a str>,
    pub paid_at: Option<&'a str>,
    pub notes: Option<&'a str>,
}

/// Campos editables. Deliberadamente sin `patient_id` — reasignar un pago a
/// otro paciente no es una operación de este MVP, mismo criterio que
/// `GoalUpdateRow`/`SessionMetadataUpdateRow`.
pub struct PaymentUpdateRow<'a> {
    pub session_id: Option<&'a str>,
    pub amount: f64,
    pub currency: &'a str,
    pub method: Option<&'a str>,
    pub status: &'a str,
    pub due_date: Option<&'a str>,
    pub paid_at: Option<&'a str>,
    pub notes: Option<&'a str>,
}

/// Agregados administrativos para el Dashboard — nunca un listado de
/// pagos individuales, solo tres números.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentDashboardSummary {
    pub paid_this_month_total: f64,
    pub pending_count: i64,
    pub pending_total: f64,
}

const PAYMENT_COLUMNS: &str = "id, patient_id, session_id, amount, currency, method, status, due_date, paid_at, notes, \
     (status = 'pendiente' AND due_date IS NOT NULL AND due_date < date('now')) AS is_overdue, \
     created_at, updated_at, deleted_at";

fn map_row(row: &Row) -> rusqlite::Result<Payment> {
    Ok(Payment {
        id: row.get("id")?,
        patient_id: row.get("patient_id")?,
        session_id: row.get("session_id")?,
        amount: row.get("amount")?,
        currency: row.get("currency")?,
        method: row.get("method")?,
        status: row.get("status")?,
        due_date: row.get("due_date")?,
        paid_at: row.get("paid_at")?,
        notes: row.get("notes")?,
        is_overdue: row.get("is_overdue")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        deleted_at: row.get("deleted_at")?,
    })
}

fn map_list_row(row: &Row) -> rusqlite::Result<PaymentListItem> {
    Ok(PaymentListItem {
        id: row.get(0)?,
        session_id: row.get(1)?,
        amount: row.get(2)?,
        currency: row.get(3)?,
        method: row.get(4)?,
        status: row.get(5)?,
        due_date: row.get(6)?,
        paid_at: row.get(7)?,
        is_overdue: row.get(8)?,
        created_at: row.get(9)?,
    })
}

pub fn insert(conn: &Connection, row: &NewPaymentRow) -> rusqlite::Result<Payment> {
    conn.execute(
        "INSERT INTO payments (
            id, patient_id, session_id, amount, currency, method, status, due_date, paid_at, notes
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            row.id,
            row.patient_id,
            row.session_id,
            row.amount,
            row.currency,
            row.method,
            row.status,
            row.due_date,
            row.paid_at,
            row.notes,
        ],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

/// Devuelve el pago exista o no `deleted_at` — archivado no es lo mismo que
/// inexistente, la capa de servicio decide qué hacer con cada caso (mismo
/// criterio que `repositories::goals::find_by_id`).
pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<Payment>> {
    conn.query_row(&format!("SELECT {PAYMENT_COLUMNS} FROM payments WHERE id = ?1"), params![id], map_row).optional()
}

fn list(conn: &Connection, patient_id: &str, deleted: bool) -> rusqlite::Result<Vec<PaymentListItem>> {
    let deleted_clause = if deleted { "deleted_at IS NOT NULL" } else { "deleted_at IS NULL" };
    let sql = format!(
        "SELECT id, session_id, amount, currency, method, status, due_date, paid_at, \
         (status = 'pendiente' AND due_date IS NOT NULL AND due_date < date('now')) AS is_overdue, \
         created_at \
         FROM payments \
         WHERE patient_id = ?1 AND {deleted_clause} \
         ORDER BY created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], map_list_row)?;
    rows.collect()
}

pub fn list_active_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<PaymentListItem>> {
    list(conn, patient_id, false)
}

pub fn list_archived_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<PaymentListItem>> {
    list(conn, patient_id, true)
}

pub fn update(conn: &Connection, id: &str, row: &PaymentUpdateRow) -> rusqlite::Result<Option<Payment>> {
    let affected = conn.execute(
        "UPDATE payments SET
            session_id = ?1, amount = ?2, currency = ?3, method = ?4, status = ?5,
            due_date = ?6, paid_at = ?7, notes = ?8
         WHERE id = ?9 AND deleted_at IS NULL",
        params![row.session_id, row.amount, row.currency, row.method, row.status, row.due_date, row.paid_at, row.notes, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

/// Soft delete únicamente. No existe, en ningún punto de este módulo, una
/// operación de borrado físico alcanzable desde un comando normal.
pub fn soft_delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE payments SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("UPDATE payments SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL", params![id])?;
    Ok(affected > 0)
}

/// Agregados para el Dashboard. Alcance deliberado: sobre **todos** los
/// pagos no archivados de la consulta, sin filtrar por si el paciente
/// dueño está archivado — un pago pendiente sigue siendo dinero pendiente
/// independientemente de si la ficha del paciente está archivada o no (ver
/// `docs/payments.md`, no fue una regla explícita de la aprobación, es la
/// lectura más simple y defendible de "agregado administrativo útil").
/// "Ingresos del mes" = suma de `amount` de pagos `pagado` cuyo `paid_at`
/// cae en el mes calendario actual (`strftime('%Y-%m', paid_at) =
/// strftime('%Y-%m','now')`, UTC). "Pagos pendientes" = cuenta y suma de
/// filas con `status = 'pendiente'` (incluye lo que se mostrará como
/// "atrasado" — es el mismo conjunto de filas, atrasado es un subconjunto
/// derivado, no un estado guardado aparte).
pub fn dashboard_summary(conn: &Connection) -> rusqlite::Result<PaymentDashboardSummary> {
    let paid_this_month_total: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount), 0.0) FROM payments \
         WHERE deleted_at IS NULL AND status = 'pagado' AND paid_at IS NOT NULL \
           AND strftime('%Y-%m', paid_at) = strftime('%Y-%m', date('now'))",
        [],
        |row| row.get(0),
    )?;

    let (pending_count, pending_total): (i64, f64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(amount), 0.0) FROM payments \
         WHERE deleted_at IS NULL AND status = 'pendiente'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    Ok(PaymentDashboardSummary { paid_this_month_total, pending_count, pending_total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::sessions::{self, NewSessionRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-payments-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x51u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_patient(conn: &Connection, name: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        patients::insert(
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
        sessions::insert(
            conn,
            &NewSessionRow {
                id: &id,
                patient_id,
                appointment_id: None,
                episode_id: None,
                session_date: "2026-09-01",
                start_time: Some("15:00"),
                duration_minutes: Some(50),
                modality: Some("presencial"),
                status: "programada",
            },
        )
        .unwrap();
        id
    }

    fn base_row<'a>(id: &'a str, patient_id: &'a str) -> NewPaymentRow<'a> {
        NewPaymentRow {
            id,
            patient_id,
            session_id: None,
            amount: 40000.0,
            currency: "CLP",
            method: None,
            status: "pendiente",
            due_date: None,
            paid_at: None,
            notes: None,
        }
    }

    #[test]
    fn inserts_and_finds_a_payment() {
        let conn = test_conn("insert-find");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let p = insert(&conn, &base_row("pay1", &patient_id)).unwrap();
        assert_eq!(p.patient_id, patient_id);
        assert_eq!(p.amount, 40000.0);
        assert_eq!(p.status, "pendiente");
        assert!(p.session_id.is_none());
        assert!(!p.is_overdue);
    }

    #[test]
    fn inserts_a_payment_with_a_session() {
        let conn = test_conn("insert-with-session");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let session_id = create_test_session(&conn, &patient_id);
        let mut row = base_row("pay1", &patient_id);
        row.session_id = Some(&session_id);
        let p = insert(&conn, &row).unwrap();
        assert_eq!(p.session_id.as_deref(), Some(session_id.as_str()));
    }

    #[test]
    fn find_by_id_returns_none_for_unknown_id() {
        let conn = test_conn("find-unknown");
        assert!(find_by_id(&conn, "no-existe").unwrap().is_none());
    }

    #[test]
    fn list_active_and_archived_are_mutually_exclusive() {
        let conn = test_conn("list-active-archived");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        insert(&conn, &base_row("pay1", &patient_id)).unwrap();

        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 1);
        assert_eq!(list_archived_by_patient(&conn, &patient_id).unwrap().len(), 0);

        assert!(soft_delete(&conn, "pay1").unwrap());

        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 0);
        assert_eq!(list_archived_by_patient(&conn, &patient_id).unwrap().len(), 1);

        assert!(restore(&conn, "pay1").unwrap());
        assert_eq!(list_active_by_patient(&conn, &patient_id).unwrap().len(), 1);
    }

    #[test]
    fn update_changes_fields_but_not_patient() {
        let conn = test_conn("update");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        insert(&conn, &base_row("pay1", &patient_id)).unwrap();

        let updated = update(
            &conn,
            "pay1",
            &PaymentUpdateRow {
                session_id: None,
                amount: 50000.0,
                currency: "CLP",
                method: Some("efectivo"),
                status: "pagado",
                due_date: None,
                paid_at: Some("2026-09-05"),
                notes: Some("Pagado en efectivo"),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.amount, 50000.0);
        assert_eq!(updated.status, "pagado");
        assert_eq!(updated.method.as_deref(), Some("efectivo"));
        assert_eq!(updated.patient_id, patient_id);
    }

    #[test]
    fn update_can_change_the_linked_session() {
        let conn = test_conn("update-session");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        let session_id = create_test_session(&conn, &patient_id);
        insert(&conn, &base_row("pay1", &patient_id)).unwrap();

        let updated = update(
            &conn,
            "pay1",
            &PaymentUpdateRow {
                session_id: Some(&session_id),
                amount: 40000.0,
                currency: "CLP",
                method: None,
                status: "pendiente",
                due_date: None,
                paid_at: None,
                notes: None,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.session_id.as_deref(), Some(session_id.as_str()));
    }

    #[test]
    fn update_on_archived_payment_does_nothing() {
        let conn = test_conn("update-archived");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        insert(&conn, &base_row("pay1", &patient_id)).unwrap();
        soft_delete(&conn, "pay1").unwrap();

        let result = update(
            &conn,
            "pay1",
            &PaymentUpdateRow { session_id: None, amount: 1.0, currency: "CLP", method: None, status: "pendiente", due_date: None, paid_at: None, notes: None },
        )
        .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn restoring_a_never_archived_payment_reports_nothing_changed() {
        let conn = test_conn("restore-noop");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        insert(&conn, &base_row("pay1", &patient_id)).unwrap();
        assert!(!restore(&conn, "pay1").unwrap());
    }

    #[test]
    fn a_pending_payment_past_its_due_date_is_flagged_overdue_without_changing_status() {
        let conn = test_conn("is-overdue-flag");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        let mut row = base_row("pay1", &patient_id);
        row.due_date = Some("2000-01-01");
        let p = insert(&conn, &row).unwrap();
        assert_eq!(p.status, "pendiente", "el status crudo nunca cambia solo");
        assert!(p.is_overdue);

        let listed = list_active_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(listed[0].status, "pendiente");
        assert!(listed[0].is_overdue);
    }

    #[test]
    fn a_pending_payment_with_a_future_due_date_is_not_overdue() {
        let conn = test_conn("not-overdue-future");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        let mut row = base_row("pay1", &patient_id);
        row.due_date = Some("2999-01-01");
        let p = insert(&conn, &row).unwrap();
        assert!(!p.is_overdue);
    }

    #[test]
    fn a_paid_payment_past_its_due_date_is_never_flagged_overdue() {
        let conn = test_conn("paid-never-overdue");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        let mut row = base_row("pay1", &patient_id);
        row.due_date = Some("2000-01-01");
        row.status = "pagado";
        row.paid_at = Some("2026-09-01");
        row.method = Some("efectivo");
        let p = insert(&conn, &row).unwrap();
        assert!(!p.is_overdue);
    }

    #[test]
    fn dashboard_summary_counts_paid_this_month_and_pending() {
        let conn = test_conn("dashboard-summary");
        let patient_id = create_test_patient(&conn, "Paciente Once");

        // Inserta directo por SQL: `paid_at` necesita ser "hoy" en tiempo de
        // ejecución del test (`strftime('now')`), no un literal fijo.
        conn.execute(
            "INSERT INTO payments (id, patient_id, amount, currency, method, status, paid_at) \
             VALUES ('pay1', ?1, 40000.0, 'CLP', 'efectivo', 'pagado', strftime('%Y-%m-%d','now'))",
            params![patient_id],
        )
        .unwrap();

        let mut pending1 = base_row("pay2", &patient_id);
        pending1.amount = 10000.0;
        insert(&conn, &pending1).unwrap();
        let mut pending2 = base_row("pay3", &patient_id);
        pending2.amount = 15000.0;
        insert(&conn, &pending2).unwrap();

        let mut condoned = base_row("pay4", &patient_id);
        condoned.status = "condonado";
        condoned.amount = 0.0;
        insert(&conn, &condoned).unwrap();

        let summary = dashboard_summary(&conn).unwrap();
        assert_eq!(summary.paid_this_month_total, 40000.0);
        assert_eq!(summary.pending_count, 2);
        assert_eq!(summary.pending_total, 25000.0);
    }

    #[test]
    fn dashboard_summary_with_no_payments_is_all_zero() {
        let conn = test_conn("dashboard-empty");
        let summary = dashboard_summary(&conn).unwrap();
        assert_eq!(summary.paid_this_month_total, 0.0);
        assert_eq!(summary.pending_count, 0);
        assert_eq!(summary.pending_total, 0.0);
    }

    #[test]
    fn dashboard_summary_excludes_archived_payments() {
        let conn = test_conn("dashboard-excludes-archived");
        let patient_id = create_test_patient(&conn, "Paciente Doce");
        insert(&conn, &base_row("pay1", &patient_id)).unwrap();
        soft_delete(&conn, "pay1").unwrap();

        let summary = dashboard_summary(&conn).unwrap();
        assert_eq!(summary.pending_count, 0);
        assert_eq!(summary.pending_total, 0.0);
    }
}
