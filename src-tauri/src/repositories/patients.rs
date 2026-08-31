//! Acceso a datos de `patients`. SQL puro — sin reglas de negocio (eso vive
//! en `services::patients`) y sin ninguna noción de si el vault está
//! desbloqueado (eso lo decide `security::session::VaultSession`, el único
//! lugar de todo el código que entrega una `&Connection`).

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

/// Ficha completa de un paciente — lo que se necesita para ver/editar su
/// detalle. Nunca se usa para el listado (ver `PatientSummary`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Patient {
    pub id: String,
    pub full_name: String,
    pub preferred_name: Option<String>,
    pub rut: Option<String>,
    pub birth_date: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub emergency_contact_name: Option<String>,
    pub emergency_contact_phone: Option<String>,
    pub emergency_contact_relationship: Option<String>,
    pub status: String,
    pub referred_by: Option<String>,
    pub intake_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// Fila mínima para el listado. Deliberadamente **no incluye el RUT** ni
/// otros datos de contacto — minimización de exposición (ver
/// `docs/ARCHITECTURE.md` sección 13.A): el listado no necesita esa
/// información para cumplir su función, así que el tipo que sale del
/// backend hacia la UI ni siquiera la contiene.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatientSummary {
    pub id: String,
    pub full_name: String,
    pub preferred_name: Option<String>,
    pub status: String,
    pub intake_date: Option<String>,
}

/// Datos ya validados (por `services::patients`) para crear un paciente.
pub struct NewPatientRow<'a> {
    pub id: &'a str,
    pub full_name: &'a str,
    pub preferred_name: Option<&'a str>,
    pub rut: Option<&'a str>,
    pub birth_date: Option<&'a str>,
    pub phone: Option<&'a str>,
    pub email: Option<&'a str>,
    pub address: Option<&'a str>,
    pub emergency_contact_name: Option<&'a str>,
    pub emergency_contact_phone: Option<&'a str>,
    pub emergency_contact_relationship: Option<&'a str>,
    pub status: &'a str,
    pub referred_by: Option<&'a str>,
    pub intake_date: Option<&'a str>,
}

/// Datos ya validados para actualizar un paciente existente. Todos los
/// campos (salvo `id`) se sobrescriben con lo que venga aquí — la fusión
/// "solo cambiar lo que llegó" ya ocurrió en `services::patients`.
pub struct PatientUpdateRow<'a> {
    pub full_name: &'a str,
    pub preferred_name: Option<&'a str>,
    pub rut: Option<&'a str>,
    pub birth_date: Option<&'a str>,
    pub phone: Option<&'a str>,
    pub email: Option<&'a str>,
    pub address: Option<&'a str>,
    pub emergency_contact_name: Option<&'a str>,
    pub emergency_contact_phone: Option<&'a str>,
    pub emergency_contact_relationship: Option<&'a str>,
    pub status: &'a str,
    pub referred_by: Option<&'a str>,
    pub intake_date: Option<&'a str>,
}

fn map_row(row: &Row) -> rusqlite::Result<Patient> {
    Ok(Patient {
        id: row.get("id")?,
        full_name: row.get("full_name")?,
        preferred_name: row.get("preferred_name")?,
        rut: row.get("rut")?,
        birth_date: row.get("birth_date")?,
        phone: row.get("phone")?,
        email: row.get("email")?,
        address: row.get("address")?,
        emergency_contact_name: row.get("emergency_contact_name")?,
        emergency_contact_phone: row.get("emergency_contact_phone")?,
        emergency_contact_relationship: row.get("emergency_contact_relationship")?,
        status: row.get("status")?,
        referred_by: row.get("referred_by")?,
        intake_date: row.get("intake_date")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        deleted_at: row.get("deleted_at")?,
    })
}

const PATIENT_COLUMNS: &str = "id, full_name, preferred_name, rut, birth_date, phone, email, address, \
     emergency_contact_name, emergency_contact_phone, emergency_contact_relationship, \
     status, referred_by, intake_date, created_at, updated_at, deleted_at";

pub fn insert(conn: &Connection, row: &NewPatientRow) -> rusqlite::Result<Patient> {
    conn.execute(
        "INSERT INTO patients (
            id, full_name, preferred_name, rut, birth_date, phone, email, address,
            emergency_contact_name, emergency_contact_phone, emergency_contact_relationship,
            status, referred_by, intake_date
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            row.id,
            row.full_name,
            row.preferred_name,
            row.rut,
            row.birth_date,
            row.phone,
            row.email,
            row.address,
            row.emergency_contact_name,
            row.emergency_contact_phone,
            row.emergency_contact_relationship,
            row.status,
            row.referred_by,
            row.intake_date,
        ],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<Patient>> {
    conn.query_row(
        &format!("SELECT {PATIENT_COLUMNS} FROM patients WHERE id = ?1"),
        params![id],
        map_row,
    )
    .optional()
}

/// Pacientes no eliminados, opcionalmente filtrados por `search` contra
/// nombre completo o nombre preferido (no contra el RUT: no es necesario
/// para encontrar un paciente en el uso diario, y evita tener que decidir
/// si el término de búsqueda cuenta como "mostrar el RUT").
pub fn list_active(conn: &Connection, search: Option<&str>) -> rusqlite::Result<Vec<PatientSummary>> {
    let mut stmt;
    let rows = match search {
        Some(term) => {
            stmt = conn.prepare(
                "SELECT id, full_name, preferred_name, status, intake_date FROM patients
                 WHERE deleted_at IS NULL
                   AND (full_name LIKE ?1 ESCAPE '\\' OR preferred_name LIKE ?1 ESCAPE '\\')
                 ORDER BY full_name COLLATE NOCASE",
            )?;
            let pattern = format!("%{}%", escape_like(term));
            stmt.query_map(params![pattern], map_summary_row)?
        }
        None => {
            stmt = conn.prepare(
                "SELECT id, full_name, preferred_name, status, intake_date FROM patients
                 WHERE deleted_at IS NULL
                 ORDER BY full_name COLLATE NOCASE",
            )?;
            stmt.query_map([], map_summary_row)?
        }
    };
    rows.collect()
}

/// Pacientes eliminados (soft delete), para la vista de "archivados". Mismo
/// filtro de búsqueda que `list_active`, pero sobre `deleted_at IS NOT NULL`
/// y ordenados por fecha de eliminación más reciente primero — es una
/// papelera, no un listado alfabético de trabajo diario.
pub fn list_deleted(conn: &Connection, search: Option<&str>) -> rusqlite::Result<Vec<PatientSummary>> {
    let mut stmt;
    let rows = match search {
        Some(term) => {
            stmt = conn.prepare(
                "SELECT id, full_name, preferred_name, status, intake_date FROM patients
                 WHERE deleted_at IS NOT NULL
                   AND (full_name LIKE ?1 ESCAPE '\\' OR preferred_name LIKE ?1 ESCAPE '\\')
                 ORDER BY deleted_at DESC",
            )?;
            let pattern = format!("%{}%", escape_like(term));
            stmt.query_map(params![pattern], map_summary_row)?
        }
        None => {
            stmt = conn.prepare(
                "SELECT id, full_name, preferred_name, status, intake_date FROM patients
                 WHERE deleted_at IS NOT NULL
                 ORDER BY deleted_at DESC",
            )?;
            stmt.query_map([], map_summary_row)?
        }
    };
    rows.collect()
}

fn map_summary_row(row: &Row) -> rusqlite::Result<PatientSummary> {
    Ok(PatientSummary {
        id: row.get(0)?,
        full_name: row.get(1)?,
        preferred_name: row.get(2)?,
        status: row.get(3)?,
        intake_date: row.get(4)?,
    })
}

fn escape_like(input: &str) -> String {
    input.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

pub fn update(conn: &Connection, id: &str, row: &PatientUpdateRow) -> rusqlite::Result<Option<Patient>> {
    let affected = conn.execute(
        "UPDATE patients SET
            full_name = ?1, preferred_name = ?2, rut = ?3, birth_date = ?4, phone = ?5,
            email = ?6, address = ?7, emergency_contact_name = ?8, emergency_contact_phone = ?9,
            emergency_contact_relationship = ?10, status = ?11, referred_by = ?12, intake_date = ?13
         WHERE id = ?14 AND deleted_at IS NULL",
        params![
            row.full_name,
            row.preferred_name,
            row.rut,
            row.birth_date,
            row.phone,
            row.email,
            row.address,
            row.emergency_contact_name,
            row.emergency_contact_phone,
            row.emergency_contact_relationship,
            row.status,
            row.referred_by,
            row.intake_date,
            id,
        ],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

/// Soft delete: nunca borra la fila. Es la única operación de "eliminar"
/// que existe en este módulo — no hay ninguna función `hard_delete` /
/// `DELETE FROM patients` en todo el código, así que un borrado físico no
/// es posible a través de una operación normal de la aplicación.
pub fn soft_delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE patients SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE patients SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
        params![id],
    )?;
    Ok(affected > 0)
}
