//! Acceso a datos de `appointments`. SQL puro — sin reglas de negocio (eso
//! vive en `services::appointments`) y sin ninguna noción de si el vault
//! está desbloqueado ni de la integración con Google Calendar (eso vive en
//! el módulo `calendar`, que llama a este repositorio, nunca al revés).
//!
//! `title` es un campo local únicamente de conveniencia (nunca se expone
//! por IPC ni se envía a Google): se fija una sola vez al crear la cita a
//! partir de si tiene paciente o no, y las consultas de lectura devuelven
//! en su lugar `patient_name` vía `LEFT JOIN` con `patients` — así el
//! nombre mostrado nunca queda desactualizado si el paciente cambia de
//! nombre después.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Appointment {
    pub id: String,
    pub patient_id: Option<String>,
    pub patient_name: Option<String>,
    pub starts_at: String,
    pub ends_at: String,
    pub status: String,
    pub modality: Option<String>,
    pub google_event_id: Option<String>,
    pub google_calendar_id: Option<String>,
    pub last_synced_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

pub struct NewAppointmentRow<'a> {
    pub id: &'a str,
    pub patient_id: Option<&'a str>,
    pub title: &'a str,
    pub starts_at: &'a str,
    pub ends_at: &'a str,
    pub modality: Option<&'a str>,
}

/// Campos editables desde el formulario general. Deliberadamente sin
/// `status`: las transiciones de estado (cancelar/archivar/restaurar) son
/// operaciones propias, con su propio efecto sobre Google Calendar — ver
/// `services::appointments`.
pub struct AppointmentUpdateRow<'a> {
    pub patient_id: Option<&'a str>,
    pub starts_at: &'a str,
    pub ends_at: &'a str,
    pub modality: Option<&'a str>,
}

const APPOINTMENT_COLUMNS: &str = "a.id, a.patient_id, p.full_name, a.starts_at, a.ends_at, a.status, \
     a.modality, a.google_event_id, a.google_calendar_id, a.last_synced_at, a.created_at, a.updated_at, a.deleted_at";

fn map_row(row: &Row) -> rusqlite::Result<Appointment> {
    Ok(Appointment {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        patient_name: row.get(2)?,
        starts_at: row.get(3)?,
        ends_at: row.get(4)?,
        status: row.get(5)?,
        modality: row.get(6)?,
        google_event_id: row.get(7)?,
        google_calendar_id: row.get(8)?,
        last_synced_at: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        deleted_at: row.get(12)?,
    })
}

pub fn insert(conn: &Connection, row: &NewAppointmentRow) -> rusqlite::Result<Appointment> {
    conn.execute(
        "INSERT INTO appointments (id, patient_id, title, starts_at, ends_at, modality)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![row.id, row.patient_id, row.title, row.starts_at, row.ends_at, row.modality],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<Appointment>> {
    conn.query_row(
        &format!("SELECT {APPOINTMENT_COLUMNS} FROM appointments a LEFT JOIN patients p ON p.id = a.patient_id WHERE a.id = ?1"),
        params![id],
        map_row,
    )
    .optional()
}

fn list(conn: &Connection, deleted: bool, from: Option<&str>, to: Option<&str>) -> rusqlite::Result<Vec<Appointment>> {
    let deleted_clause = if deleted { "a.deleted_at IS NOT NULL" } else { "a.deleted_at IS NULL" };
    let mut sql = format!(
        "SELECT {APPOINTMENT_COLUMNS} FROM appointments a LEFT JOIN patients p ON p.id = a.patient_id WHERE {deleted_clause}"
    );
    if from.is_some() {
        sql.push_str(" AND a.ends_at >= ?1");
    }
    if to.is_some() {
        sql.push_str(if from.is_some() { " AND a.starts_at <= ?2" } else { " AND a.starts_at <= ?1" });
    }
    sql.push_str(" ORDER BY a.starts_at");

    let mut stmt = conn.prepare(&sql)?;
    let rows = match (from, to) {
        (Some(f), Some(t)) => stmt.query_map(params![f, t], map_row)?,
        (Some(f), None) => stmt.query_map(params![f], map_row)?,
        (None, Some(t)) => stmt.query_map(params![t], map_row)?,
        (None, None) => stmt.query_map([], map_row)?,
    };
    rows.collect()
}

/// Citas activas (no archivadas), opcionalmente acotadas a un rango
/// `[from, to]` sobre las que se solapen con el rango (no solo las que
/// empiecen dentro de él) — así una cita larga que empezó antes de `from`
/// pero sigue vigente durante el rango no se pierde de la vista "Hoy".
pub fn list_active(conn: &Connection, from: Option<&str>, to: Option<&str>) -> rusqlite::Result<Vec<Appointment>> {
    list(conn, false, from, to)
}

pub fn list_deleted(conn: &Connection, from: Option<&str>, to: Option<&str>) -> rusqlite::Result<Vec<Appointment>> {
    list(conn, true, from, to)
}

/// Citas activas, no canceladas, cuyo rango se superpone con
/// `[starts_at, ends_at)`, excluyendo `exclude_id` (la propia cita al
/// editar). Usado únicamente para la advertencia de solapamiento — nunca
/// bloquea el guardado, ver `services::appointments::check_overlap`.
pub fn find_overlapping(
    conn: &Connection,
    starts_at: &str,
    ends_at: &str,
    exclude_id: Option<&str>,
) -> rusqlite::Result<Vec<Appointment>> {
    let sql = format!(
        "SELECT {APPOINTMENT_COLUMNS} FROM appointments a LEFT JOIN patients p ON p.id = a.patient_id
         WHERE a.deleted_at IS NULL AND a.status != 'cancelada'
           AND a.starts_at < ?1 AND a.ends_at > ?2
           AND (?3 IS NULL OR a.id != ?3)
         ORDER BY a.starts_at"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![ends_at, starts_at, exclude_id], map_row)?;
    rows.collect()
}

pub fn update(conn: &Connection, id: &str, row: &AppointmentUpdateRow) -> rusqlite::Result<Option<Appointment>> {
    let affected = conn.execute(
        "UPDATE appointments SET patient_id = ?1, starts_at = ?2, ends_at = ?3, modality = ?4
         WHERE id = ?5 AND deleted_at IS NULL",
        params![row.patient_id, row.starts_at, row.ends_at, row.modality, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

/// Cambia únicamente el estado (usado por `cancel_appointment`). No toca
/// ningún otro campo.
pub fn set_status(conn: &Connection, id: &str, status: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE appointments SET status = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![status, id],
    )?;
    Ok(affected > 0)
}

/// Guarda o limpia el vínculo con Google Calendar. Es el único lugar del
/// repositorio que toca `google_event_id`/`google_calendar_id`/
/// `last_synced_at` — llamado exclusivamente desde `calendar::sync`.
pub fn set_google_link(
    conn: &Connection,
    id: &str,
    google_event_id: Option<&str>,
    google_calendar_id: Option<&str>,
    last_synced_at: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE appointments SET google_event_id = ?1, google_calendar_id = ?2, last_synced_at = ?3 WHERE id = ?4",
        params![google_event_id, google_calendar_id, last_synced_at, id],
    )?;
    Ok(())
}

/// Soft delete únicamente. No existe, en ningún punto de este módulo, una
/// operación de borrado físico alcanzable desde un comando normal de la
/// aplicación.
pub fn soft_delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE appointments SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE appointments SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
        params![id],
    )?;
    Ok(affected > 0)
}
