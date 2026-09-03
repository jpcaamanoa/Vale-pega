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
    /// Región de residencia (Fase 6.1): nombre exacto del catálogo cerrado
    /// en `geo.rs`, o el valor reservado `"Extranjero"`, o `None` ("no
    /// informado"). Nunca texto libre — ver `services::patients::validate_geo`.
    pub region: Option<String>,
    /// Comuna de residencia: solo tiene sentido junto a `region` (una comuna
    /// real de Chile). Siempre `None` cuando `region` es `"Extranjero"` o
    /// `None`.
    pub commune: Option<String>,
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
    pub region: Option<&'a str>,
    pub commune: Option<&'a str>,
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
    pub region: Option<&'a str>,
    pub commune: Option<&'a str>,
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
        region: row.get("region")?,
        commune: row.get("commune")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        deleted_at: row.get("deleted_at")?,
    })
}

const PATIENT_COLUMNS: &str = "id, full_name, preferred_name, rut, birth_date, phone, email, address, \
     emergency_contact_name, emergency_contact_phone, emergency_contact_relationship, \
     status, referred_by, intake_date, region, commune, created_at, updated_at, deleted_at";

pub fn insert(conn: &Connection, row: &NewPatientRow) -> rusqlite::Result<Patient> {
    conn.execute(
        "INSERT INTO patients (
            id, full_name, preferred_name, rut, birth_date, phone, email, address,
            emergency_contact_name, emergency_contact_phone, emergency_contact_relationship,
            status, referred_by, intake_date, region, commune
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
            row.region,
            row.commune,
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
            emergency_contact_relationship = ?10, status = ?11, referred_by = ?12, intake_date = ?13,
            region = ?14, commune = ?15
         WHERE id = ?16 AND deleted_at IS NULL",
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
            row.region,
            row.commune,
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

/// Un `(etiqueta, cantidad)` sin agrupar todavía — agrupar categorías con
/// menos de tres pacientes en "Otras" es una decisión de negocio que vive en
/// `services::patients::geographic_statistics`, no aquí.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoCount {
    pub label: String,
    pub count: i64,
}

/// Distribución geográfica cruda de los pacientes (Fase 6.1). El conteo
/// siempre ocurre en SQL vía `GROUP BY` — nunca se trae la lista completa de
/// pacientes a Rust para contarlos a mano, y mucho menos al frontend.
#[derive(Debug, Clone)]
pub struct GeographicDistribution {
    pub with_location: i64,
    pub without_location: i64,
    pub by_region: Vec<GeoCount>,
    pub by_commune: Vec<GeoCount>,
}

pub fn geographic_distribution(conn: &Connection, include_archived: bool) -> rusqlite::Result<GeographicDistribution> {
    let status_filter = if include_archived { "" } else { "AND deleted_at IS NULL" };

    let (with_location, without_location) = conn.query_row(
        &format!(
            "SELECT
                SUM(CASE WHEN region IS NOT NULL THEN 1 ELSE 0 END),
                SUM(CASE WHEN region IS NULL THEN 1 ELSE 0 END)
             FROM patients WHERE 1=1 {status_filter}"
        ),
        [],
        |row| {
            let with_loc: Option<i64> = row.get(0)?;
            let without_loc: Option<i64> = row.get(1)?;
            Ok((with_loc.unwrap_or(0), without_loc.unwrap_or(0)))
        },
    )?;

    let mut region_stmt = conn.prepare(&format!(
        "SELECT region, COUNT(*) FROM patients
         WHERE region IS NOT NULL {status_filter}
         GROUP BY region ORDER BY region"
    ))?;
    let by_region = region_stmt
        .query_map([], |row| Ok(GeoCount { label: row.get(0)?, count: row.get(1)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // `commune` solo se informa para pacientes residentes en Chile (nunca
    // para "Extranjero", cuya comuna siempre es NULL por validación), así
    // que este filtro basta — no hace falta excluir la región reservada
    // explícitamente.
    let mut commune_stmt = conn.prepare(&format!(
        "SELECT commune, COUNT(*) FROM patients
         WHERE commune IS NOT NULL {status_filter}
         GROUP BY commune ORDER BY commune"
    ))?;
    let by_commune = commune_stmt
        .query_map([], |row| Ok(GeoCount { label: row.get(0)?, count: row.get(1)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(GeographicDistribution { with_location, without_location, by_region, by_commune })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-patients-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x7bu8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn base_row<'a>(id: &'a str, full_name: &'a str) -> NewPatientRow<'a> {
        NewPatientRow {
            id,
            full_name,
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
        }
    }

    #[test]
    fn inserts_a_patient_with_region_and_commune() {
        let conn = test_conn("insert-region-commune");
        let mut row = base_row("p1", "Ana Pérez");
        row.region = Some("Región de Valparaíso");
        row.commune = Some("Quillota");
        let p = insert(&conn, &row).unwrap();
        assert_eq!(p.region.as_deref(), Some("Región de Valparaíso"));
        assert_eq!(p.commune.as_deref(), Some("Quillota"));
    }

    #[test]
    fn inserts_a_patient_leaving_region_and_commune_null() {
        let conn = test_conn("insert-no-location");
        let row = base_row("p1", "Ana Pérez");
        let p = insert(&conn, &row).unwrap();
        assert!(p.region.is_none());
        assert!(p.commune.is_none());
    }

    #[test]
    fn updates_region_and_commune() {
        let conn = test_conn("update-region-commune");
        let row = base_row("p1", "Ana Pérez");
        insert(&conn, &row).unwrap();

        let update_row = PatientUpdateRow {
            full_name: "Ana Pérez",
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
            region: Some("Región Metropolitana de Santiago"),
            commune: Some("Ñuñoa"),
        };
        let updated = update(&conn, "p1", &update_row).unwrap().unwrap();
        assert_eq!(updated.region.as_deref(), Some("Región Metropolitana de Santiago"));
        assert_eq!(updated.commune.as_deref(), Some("Ñuñoa"));
    }

    #[test]
    fn geographic_distribution_counts_with_and_without_location_via_group_by() {
        let conn = test_conn("geo-dist-with-without");
        let mut a = base_row("p1", "Ana");
        a.region = Some("Región de Valparaíso");
        a.commune = Some("Quillota");
        insert(&conn, &a).unwrap();

        let mut b = base_row("p2", "Bruno");
        b.region = Some("Región de Valparaíso");
        b.commune = Some("Quillota");
        insert(&conn, &b).unwrap();

        insert(&conn, &base_row("p3", "Carla")).unwrap();

        let dist = geographic_distribution(&conn, false).unwrap();
        assert_eq!(dist.with_location, 2);
        assert_eq!(dist.without_location, 1);
        assert_eq!(dist.by_region, vec![GeoCount { label: "Región de Valparaíso".to_string(), count: 2 }]);
        assert_eq!(dist.by_commune, vec![GeoCount { label: "Quillota".to_string(), count: 2 }]);
    }

    #[test]
    fn geographic_distribution_treats_extranjero_as_a_region_without_commune() {
        let conn = test_conn("geo-dist-extranjero");
        let mut a = base_row("p1", "Ana");
        a.region = Some("Extranjero");
        insert(&conn, &a).unwrap();

        let dist = geographic_distribution(&conn, false).unwrap();
        assert_eq!(dist.with_location, 1);
        assert_eq!(dist.by_region, vec![GeoCount { label: "Extranjero".to_string(), count: 1 }]);
        assert!(dist.by_commune.is_empty());
    }

    #[test]
    fn geographic_distribution_respects_the_archived_filter() {
        let conn = test_conn("geo-dist-archived");
        let mut a = base_row("p1", "Ana");
        a.region = Some("Región de Valparaíso");
        a.commune = Some("Quillota");
        insert(&conn, &a).unwrap();
        soft_delete(&conn, "p1").unwrap();

        let active_only = geographic_distribution(&conn, false).unwrap();
        assert_eq!(active_only.with_location, 0);
        assert!(active_only.by_region.is_empty());

        let including_archived = geographic_distribution(&conn, true).unwrap();
        assert_eq!(including_archived.with_location, 1);
        assert_eq!(including_archived.by_region, vec![GeoCount { label: "Región de Valparaíso".to_string(), count: 1 }]);
    }
}
