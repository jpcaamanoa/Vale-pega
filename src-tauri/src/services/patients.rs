//! Reglas de negocio de pacientes: validación autoritativa (la de Zod en el
//! frontend es solo para UX) y orquestación del repositorio. No sabe nada
//! de Tauri ni de si el vault está desbloqueado — eso ya se resolvió antes
//! de llegar aquí (`security::session::VaultSession::with_connection`).

use std::fmt;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::geo;
use crate::repositories::patients::{self, NewPatientRow, Patient, PatientHardDeleteScope, PatientSummary, PatientUpdateRow};
use crate::security::VaultSession;
use crate::services::document_crypto;

use super::rut::{self, RutError};

/// Bajo este umbral (cantidad de pacientes), una categoría de región o
/// comuna se agrupa en `OTHER_CATEGORY_LABEL` en vez de mostrarse individual
/// — evita que una comuna con uno o dos pacientes identifique indirectamente
/// a alguien en un gráfico. Se aplica igual a región y a comuna (Fase 6.1).
const SMALL_CATEGORY_THRESHOLD: i64 = 3;
const OTHER_CATEGORY_LABEL: &str = "Otras";

pub const VALID_STATUSES: &[&str] = &["activo", "inactivo", "alta", "archivado"];
const DEFAULT_STATUS: &str = "activo";
/// La auditoría de transición post-Fase 11 detectó que `'archivado'` era
/// seleccionable manualmente en el formulario normal sin ejecutar el
/// archivado estructural real (que depende exclusivamente de `deleted_at`,
/// vía `archive_patient`/`restore_patient`) — un paciente podía quedar con
/// `status='archivado'` y `deleted_at=NULL` (sigue apareciendo como activo)
/// o viceversa. Este valor permanece en `VALID_STATUSES` y en el `CHECK` de
/// `SCHEMA_V1` **sin ningún cambio de esquema** — filas creadas antes de
/// esta corrección con `status='archivado'` deben poder seguir
/// leyéndose/actualizándose sin perder ni alterar ese valor. Lo que cambia
/// es que ya no puede **elegirse** como una transición nueva: ver `validate`.
const LEGACY_ARCHIVED_STATUS: &str = "archivado";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatientInput {
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
    pub status: Option<String>,
    pub referred_by: Option<String>,
    pub intake_date: Option<String>,
    /// Nombre exacto de una región del catálogo cerrado (`geo::is_known_region`)
    /// o el valor reservado `"Extranjero"`. `None` = "no informado". Nunca
    /// texto libre: se valida contra el catálogo en `validate_geo`.
    pub region: Option<String>,
    /// Nombre exacto de una comuna del catálogo, que debe pertenecer a
    /// `region`. Debe ser `None` cuando `region` es `"Extranjero"` o `None`.
    pub commune: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatientListItem {
    pub id: String,
    pub full_name: String,
    pub preferred_name: Option<String>,
    pub status: String,
    pub intake_date: Option<String>,
}

impl From<PatientSummary> for PatientListItem {
    fn from(p: PatientSummary) -> Self {
        Self {
            id: p.id,
            full_name: p.full_name,
            preferred_name: p.preferred_name,
            status: p.status,
            intake_date: p.intake_date,
        }
    }
}

#[derive(Debug)]
pub enum PatientValidationError {
    EmptyFullName,
    InvalidStatus(String),
    InvalidRut(RutError),
    InvalidDate { field: &'static str },
    /// La región no es ninguna de las 16 del catálogo ni el valor reservado
    /// `"Extranjero"`.
    UnknownRegion(String),
    /// Se informó una comuna sin región (una comuna nunca tiene sentido
    /// sola).
    CommuneRequiresRegion,
    /// La región es `"Extranjero"`, que por definición no tiene comunas.
    ForeignRegionCannotHaveCommune,
    /// La comuna es real, pero no pertenece a la región indicada (p. ej.
    /// Quillota con Región Metropolitana en vez de Región de Valparaíso).
    CommuneNotInRegion { region: String, commune: String },
    /// `status = 'archivado'` fue enviado como una transición nueva (el
    /// paciente no lo tenía ya) — nunca es una elección manual válida. La
    /// fuente de verdad de "archivado" es `deleted_at`
    /// (`archive_patient`/`restore_patient`), nunca este campo.
    ArchivedStatusIsNotManuallySelectable,
}

impl fmt::Display for PatientValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatientValidationError::EmptyFullName => write!(f, "el nombre completo es obligatorio"),
            PatientValidationError::InvalidStatus(s) => {
                write!(f, "estado inválido: '{s}' (debe ser uno de: {})", VALID_STATUSES.join(", "))
            }
            PatientValidationError::InvalidRut(e) => write!(f, "RUT inválido: {e}"),
            PatientValidationError::InvalidDate { field } => {
                write!(f, "fecha inválida en '{field}' (formato esperado: AAAA-MM-DD)")
            }
            PatientValidationError::UnknownRegion(r) => {
                write!(f, "región desconocida: '{r}'")
            }
            PatientValidationError::CommuneRequiresRegion => {
                write!(f, "no se puede indicar una comuna sin región")
            }
            PatientValidationError::ForeignRegionCannotHaveCommune => {
                write!(f, "'{}' no tiene comunas", geo::EXTRANJERO)
            }
            PatientValidationError::CommuneNotInRegion { region, commune } => {
                write!(f, "'{commune}' no pertenece a la región '{region}'")
            }
            PatientValidationError::ArchivedStatusIsNotManuallySelectable => write!(
                f,
                "el estado 'archivado' no se selecciona manualmente — usa las acciones Archivar/Restaurar de la ficha del paciente"
            ),
        }
    }
}
impl std::error::Error for PatientValidationError {}

#[derive(Debug)]
pub enum PatientError {
    Validation(PatientValidationError),
    NotFound,
    Database(rusqlite::Error),
    /// `hard_delete_patient` exige que el paciente esté archivado primero — nunca se ofrece
    /// "Eliminar permanentemente" como acción directa desde un paciente activo.
    MustBeArchivedFirst,
    /// Solo alcanzable desde `hard_delete_patient` (las demás funciones de este módulo no tocan
    /// `VaultSession` — ver el comentario de esa función para por qué esta es la excepción).
    VaultLocked,
}
impl fmt::Display for PatientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatientError::Validation(e) => write!(f, "{e}"),
            PatientError::NotFound => write!(f, "paciente no encontrado"),
            // Nunca se interpola el error de rusqlite con datos de la fila
            // (podría incluir valores) — solo un mensaje genérico técnico.
            PatientError::Database(_) => write!(f, "error interno al acceder a la base de datos"),
            PatientError::MustBeArchivedFirst => write!(f, "el paciente debe estar archivado antes de poder eliminarse permanentemente"),
            PatientError::VaultLocked => write!(f, "el vault está bloqueado"),
        }
    }
}
impl std::error::Error for PatientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PatientError::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for PatientError {
    fn from(e: rusqlite::Error) -> Self {
        PatientError::Database(e)
    }
}
impl From<PatientValidationError> for PatientError {
    fn from(e: PatientValidationError) -> Self {
        PatientError::Validation(e)
    }
}

fn none_if_blank(s: Option<String>) -> Option<String> {
    let trimmed = s.map(|v| v.trim().to_string());
    match trimmed {
        Some(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

fn validate_status(status: &str) -> Result<(), PatientValidationError> {
    if VALID_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(PatientValidationError::InvalidStatus(status.to_string()))
    }
}

fn validate_date_format(value: &str, field: &'static str) -> Result<(), PatientValidationError> {
    let bytes = value.as_bytes();
    let valid_shape = bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-';
    let parse = |s: &str| s.parse::<u32>().ok();
    let parts_ok = valid_shape
        && match (parse(&value[0..4]), parse(&value[5..7]), parse(&value[8..10])) {
            (Some(_year), Some(month), Some(day)) => (1..=12).contains(&month) && (1..=31).contains(&day),
            _ => false,
        };
    if parts_ok {
        Ok(())
    } else {
        Err(PatientValidationError::InvalidDate { field })
    }
}

struct ValidatedFields {
    full_name: String,
    preferred_name: Option<String>,
    rut: Option<String>,
    birth_date: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    address: Option<String>,
    emergency_contact_name: Option<String>,
    emergency_contact_phone: Option<String>,
    emergency_contact_relationship: Option<String>,
    status: String,
    referred_by: Option<String>,
    intake_date: Option<String>,
    region: Option<String>,
    commune: Option<String>,
}

/// Valida región y comuna según las siete reglas de Fase 6.1:
/// 1. ambas `None` → válido ("no informado").
/// 2. región conocida + comuna que pertenece a esa región → válido.
/// 3. comuna que pertenece a otra región → rechazada.
/// 4. región `"Extranjero"` + comuna → rechazada (Extranjero no tiene comunas).
/// 5. comuna sin región → rechazada.
/// 6. ambos strings se normalizan con `none_if_blank` antes de validar.
/// 7. esta función es la única fuente de verdad — el frontend valida para
///    UX, pero nunca se confía en lo que llega desde ahí sin repetir esto.
fn validate_geo(
    region: Option<String>,
    commune: Option<String>,
) -> Result<(Option<String>, Option<String>), PatientValidationError> {
    let region = none_if_blank(region);
    let commune = none_if_blank(commune);

    match (&region, &commune) {
        (None, None) => Ok((None, None)),
        (None, Some(_)) => Err(PatientValidationError::CommuneRequiresRegion),
        (Some(r), None) => {
            if r == geo::EXTRANJERO || geo::is_known_region(r) {
                Ok((region, None))
            } else {
                Err(PatientValidationError::UnknownRegion(r.clone()))
            }
        }
        (Some(r), Some(c)) => {
            if r == geo::EXTRANJERO {
                Err(PatientValidationError::ForeignRegionCannotHaveCommune)
            } else if !geo::is_known_region(r) {
                Err(PatientValidationError::UnknownRegion(r.clone()))
            } else if !geo::commune_belongs_to_region(r, c) {
                Err(PatientValidationError::CommuneNotInRegion { region: r.clone(), commune: c.clone() })
            } else {
                Ok((region, commune))
            }
        }
    }
}

/// Validación autoritativa: se ejecuta siempre en Rust, sin importar lo que
/// haya validado (o no) el formulario en React.
///
/// `current_status` es el `status` ya guardado del paciente (`None` al
/// crear uno nuevo) — se usa exclusivamente para permitir que una fila
/// legacy con `status='archivado'` (de antes de la corrección de
/// transición post-Fase 11) se siga guardando sin forzar un cambio de
/// valor, sin abrir la puerta a que un paciente **distinto** de ese
/// adquiera `'archivado'` por esta vía.
fn validate(input: PatientInput, current_status: Option<&str>) -> Result<ValidatedFields, PatientValidationError> {
    let full_name = input.full_name.trim().to_string();
    if full_name.is_empty() {
        return Err(PatientValidationError::EmptyFullName);
    }

    let status = input.status.unwrap_or_else(|| DEFAULT_STATUS.to_string());
    validate_status(&status)?;
    if status == LEGACY_ARCHIVED_STATUS && current_status != Some(LEGACY_ARCHIVED_STATUS) {
        return Err(PatientValidationError::ArchivedStatusIsNotManuallySelectable);
    }

    let rut = none_if_blank(input.rut);
    let rut = match rut {
        Some(value) => {
            rut::validate_chilean_rut(&value).map_err(PatientValidationError::InvalidRut)?;
            Some(rut::normalize_chilean_rut(&value))
        }
        None => None,
    };

    let birth_date = none_if_blank(input.birth_date);
    if let Some(ref d) = birth_date {
        validate_date_format(d, "birthDate")?;
    }
    let intake_date = none_if_blank(input.intake_date);
    if let Some(ref d) = intake_date {
        validate_date_format(d, "intakeDate")?;
    }

    let (region, commune) = validate_geo(input.region, input.commune)?;

    Ok(ValidatedFields {
        full_name,
        preferred_name: none_if_blank(input.preferred_name),
        rut,
        birth_date,
        phone: none_if_blank(input.phone),
        email: none_if_blank(input.email),
        address: none_if_blank(input.address),
        emergency_contact_name: none_if_blank(input.emergency_contact_name),
        emergency_contact_phone: none_if_blank(input.emergency_contact_phone),
        emergency_contact_relationship: none_if_blank(input.emergency_contact_relationship),
        status,
        referred_by: none_if_blank(input.referred_by),
        intake_date,
        region,
        commune,
    })
}

pub fn create_patient(conn: &Connection, input: PatientInput) -> Result<Patient, PatientError> {
    let f = validate(input, None)?;
    let id = uuid::Uuid::new_v4().to_string();
    let row = NewPatientRow {
        id: &id,
        full_name: &f.full_name,
        preferred_name: f.preferred_name.as_deref(),
        rut: f.rut.as_deref(),
        birth_date: f.birth_date.as_deref(),
        phone: f.phone.as_deref(),
        email: f.email.as_deref(),
        address: f.address.as_deref(),
        emergency_contact_name: f.emergency_contact_name.as_deref(),
        emergency_contact_phone: f.emergency_contact_phone.as_deref(),
        emergency_contact_relationship: f.emergency_contact_relationship.as_deref(),
        status: &f.status,
        referred_by: f.referred_by.as_deref(),
        intake_date: f.intake_date.as_deref(),
        region: f.region.as_deref(),
        commune: f.commune.as_deref(),
    };
    Ok(patients::insert(conn, &row)?)
}

pub fn get_patient(conn: &Connection, id: &str) -> Result<Patient, PatientError> {
    patients::find_by_id(conn, id)?.ok_or(PatientError::NotFound)
}

pub fn list_patients(conn: &Connection, search: Option<String>) -> Result<Vec<PatientListItem>, PatientError> {
    let search = search.filter(|s| !s.trim().is_empty());
    let rows = patients::list_active(conn, search.as_deref())?;
    Ok(rows.into_iter().map(PatientListItem::from).collect())
}

/// Vista de "archivados": pacientes con soft delete aplicado, para la
/// papelera desde la que se pueden revisar y restaurar. Nunca se mezcla con
/// `list_patients` (que solo devuelve pacientes activos) — son dos
/// consultas explícitamente separadas, igual que en el repositorio.
pub fn list_archived_patients(conn: &Connection, search: Option<String>) -> Result<Vec<PatientListItem>, PatientError> {
    let search = search.filter(|s| !s.trim().is_empty());
    let rows = patients::list_deleted(conn, search.as_deref())?;
    Ok(rows.into_iter().map(PatientListItem::from).collect())
}

pub fn update_patient(conn: &Connection, id: &str, input: PatientInput) -> Result<Patient, PatientError> {
    let existing = patients::find_by_id(conn, id)?.ok_or(PatientError::NotFound)?;
    let f = validate(input, Some(&existing.status))?;
    let row = PatientUpdateRow {
        full_name: &f.full_name,
        preferred_name: f.preferred_name.as_deref(),
        rut: f.rut.as_deref(),
        birth_date: f.birth_date.as_deref(),
        phone: f.phone.as_deref(),
        email: f.email.as_deref(),
        address: f.address.as_deref(),
        emergency_contact_name: f.emergency_contact_name.as_deref(),
        emergency_contact_phone: f.emergency_contact_phone.as_deref(),
        emergency_contact_relationship: f.emergency_contact_relationship.as_deref(),
        status: &f.status,
        referred_by: f.referred_by.as_deref(),
        intake_date: f.intake_date.as_deref(),
        region: f.region.as_deref(),
        commune: f.commune.as_deref(),
    };
    patients::update(conn, id, &row)?.ok_or(PatientError::NotFound)
}

/// Soft delete únicamente. No existe, en ningún punto de este servicio ni
/// del repositorio, una operación de borrado físico alcanzable desde un
/// comando normal de la aplicación.
pub fn archive_patient(conn: &Connection, id: &str) -> Result<(), PatientError> {
    if patients::soft_delete(conn, id)? {
        Ok(())
    } else {
        Err(PatientError::NotFound)
    }
}

pub fn restore_patient(conn: &Connection, id: &str) -> Result<(), PatientError> {
    if patients::restore(conn, id)? {
        Ok(())
    } else {
        Err(PatientError::NotFound)
    }
}

/// Resumen de solo lectura de todo lo que se perdería al eliminar permanentemente este paciente —
/// para el modal de confirmación del frontend (FASE 2B). Nunca borra nada.
pub fn hard_delete_scope(conn: &Connection, id: &str) -> Result<PatientHardDeleteScope, PatientError> {
    patients::find_by_id(conn, id)?.ok_or(PatientError::NotFound)?;
    Ok(patients::compute_hard_delete_scope(conn, id)?)
}

/// Borrado físico e irreversible del paciente y de todo lo que le pertenece exclusivamente (ver
/// `repositories::patients::hard_delete` para el inventario completo de tablas y el orden de
/// borrado). Excepción deliberada a la regla del encabezado de este archivo ("no sabe nada de
/// `VaultSession`"): a diferencia del resto de este módulo, este borrado también debe eliminar
/// los archivos cifrados de los documentos privados del paciente del disco — mismo motivo y mismo
/// patrón ya usado por `services::library::hard_delete_resource`.
///
/// Exige que el paciente esté ya archivado (`MustBeArchivedFirst` si no) — nunca se ofrece
/// "Eliminar permanentemente" como acción directa desde un paciente activo.
pub fn hard_delete_patient(session: &VaultSession, files_root: &Path, id: &str) -> Result<(), PatientError> {
    let storage_paths: Vec<String> = session
        .with_connection(|conn| -> Result<Vec<String>, PatientError> {
            let patient = patients::find_by_id(conn, id)?.ok_or(PatientError::NotFound)?;
            if patient.deleted_at.is_none() {
                return Err(PatientError::MustBeArchivedFirst);
            }
            Ok(patients::hard_delete(conn, id)?)
        })
        .map_err(|_locked| PatientError::VaultLocked)??;

    for storage_path in storage_paths {
        if let Ok(abs_path) = document_crypto::resolve_within_files_root(files_root, &storage_path) {
            let _ = std::fs::remove_file(&abs_path);
        }
    }
    Ok(())
}

#[allow(dead_code)] // se usa desde los tests de este módulo para inspeccionar un registro eliminado
pub fn get_patient_including_deleted(conn: &Connection, id: &str) -> Result<Patient, PatientError> {
    patients::find_by_id(conn, id)?.ok_or(PatientError::NotFound)
}

/// Una categoría (nombre de región o de comuna) con su cantidad de
/// pacientes, ya agrupada — nunca contiene ningún dato identificable de un
/// paciente individual (ver `docs/geographic-stats.md`).
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GeoDistributionItem {
    pub label: String,
    pub count: i64,
}

/// Estadísticas geográficas agregadas para la pantalla "Estadísticas"
/// (Fase 6.1). Minimización estructural: no existe ningún campo aquí que
/// permita llegar a un paciente concreto — solo etiquetas y conteos.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeographicStatistics {
    pub with_location: i64,
    pub without_location: i64,
    pub by_region: Vec<GeoDistributionItem>,
    pub by_commune: Vec<GeoDistributionItem>,
}

/// Agrupa en `"Otras"` cualquier categoría con menos de
/// `SMALL_CATEGORY_THRESHOLD` pacientes, para que ninguna categoría pequeña
/// (potencialmente 1 o 2 personas) sea identificable en un gráfico. Se
/// aplica igual a la distribución por región y por comuna. Orden resultante:
/// de mayor a menor cantidad.
fn group_small_categories(rows: Vec<patients::GeoCount>) -> Vec<GeoDistributionItem> {
    let mut result = Vec::new();
    let mut other_count: i64 = 0;
    for row in rows {
        if row.count < SMALL_CATEGORY_THRESHOLD {
            other_count += row.count;
        } else {
            result.push(GeoDistributionItem { label: row.label, count: row.count });
        }
    }
    if other_count > 0 {
        result.push(GeoDistributionItem { label: OTHER_CATEGORY_LABEL.to_string(), count: other_count });
    }
    result.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    result
}

/// Misma cantidad y orden que `group_small_categories`, pero sin fusionar
/// ninguna categoría pequeña en `"Otras"` — cada región/comuna registrada se
/// muestra tal cual, incluso con un solo paciente. Pensada para la pantalla
/// "Estadísticas" (uso privado, local, solo la psicóloga tratante ve sus
/// propios pacientes): la supresión de categorías pequeñas existe para
/// proteger de identificación indirecta en un informe/exportación
/// *compartible* con terceros (ver `group_small_categories`), no tiene
/// sentido en una pantalla que nadie más que la propia clínica puede ver.
fn list_all_categories(rows: Vec<patients::GeoCount>) -> Vec<GeoDistributionItem> {
    let mut result: Vec<GeoDistributionItem> = rows.into_iter().map(|row| GeoDistributionItem { label: row.label, count: row.count }).collect();
    result.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    result
}

/// `include_archived = false` → solo pacientes activos (no eliminados);
/// `true` → todos, incluidos los archivados. La agregación real (`GROUP BY`)
/// ocurre en `repositories::patients::geographic_distribution`, nunca aquí
/// contando una lista completa de pacientes traída a memoria.
///
/// `suppress_small_categories`: `true` aplica `group_small_categories` (la
/// protección de identificación indirecta pensada para un futuro informe
/// compartible); `false` (lo que usa hoy la pantalla "Estadísticas", de uso
/// privado) devuelve cada categoría tal cual, sin agrupar ninguna en
/// `"Otras"`. La función de agrupación (`group_small_categories`) no se
/// elimina: sigue disponible para cuando exista esa exportación.
pub fn geographic_statistics(conn: &Connection, include_archived: bool, suppress_small_categories: bool) -> Result<GeographicStatistics, PatientError> {
    let raw = patients::geographic_distribution(conn, include_archived)?;
    let group = if suppress_small_categories { group_small_categories } else { list_all_categories };
    Ok(GeographicStatistics {
        with_location: raw.with_location,
        without_location: raw.without_location,
        by_region: group(raw.by_region),
        by_commune: group(raw.by_commune),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-patients-service-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x42u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    /// Sesión real desbloqueada (no solo una `Connection` cruda) — necesaria para probar
    /// `hard_delete_patient`, que también borra archivos del disco vía `VaultSession`.
    fn test_session(name: &str) -> (VaultSession, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("cc-patients-svc-test-{}-{}", std::process::id(), name));
        if base.exists() {
            std::fs::remove_dir_all(&base).unwrap();
        }
        let vault_dir = base.join("vault");
        std::fs::create_dir_all(&vault_dir).unwrap();
        let files_root = vault_dir.join("files");
        std::fs::create_dir_all(&files_root).unwrap();

        let session = VaultSession::new(&vault_dir);
        session.begin_creation("una-contraseña-de-prueba-larga-1").unwrap();
        session.confirm_creation().unwrap();
        (session, files_root)
    }

    fn minimal_input(name: &str) -> PatientInput {
        PatientInput {
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
        }
    }

    #[test]
    fn creates_a_patient_with_defaults() {
        let conn = test_conn("create-defaults");
        let p = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        assert_eq!(p.full_name, "Ana Pérez");
        assert_eq!(p.status, "activo");
        assert!(p.deleted_at.is_none());
    }

    #[test]
    fn rejects_empty_full_name() {
        let conn = test_conn("reject-empty-name");
        let err = create_patient(&conn, minimal_input("   ")).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::EmptyFullName)
        ));
    }

    #[test]
    fn rejects_invalid_status() {
        let conn = test_conn("reject-invalid-status");
        let mut input = minimal_input("Ana Pérez");
        input.status = Some("no_existe".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::InvalidStatus(_))
        ));
    }

    #[test]
    fn rejects_invalid_rut() {
        let conn = test_conn("reject-invalid-rut");
        let mut input = minimal_input("Ana Pérez");
        input.rut = Some("12345678-9".to_string()); // dígito verificador incorrecto
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::InvalidRut(_))
        ));
    }

    #[test]
    fn accepts_and_normalizes_a_valid_rut() {
        let conn = test_conn("accept-valid-rut");
        let mut input = minimal_input("Ana Pérez");
        input.rut = Some("12.345.678-5".to_string());
        let p = create_patient(&conn, input).unwrap();
        assert_eq!(p.rut.as_deref(), Some("12345678-5"));
    }

    #[test]
    fn rejects_malformed_birth_date() {
        let conn = test_conn("reject-bad-birthdate");
        let mut input = minimal_input("Ana Pérez");
        input.birth_date = Some("31-02-2000".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::InvalidDate { field: "birthDate" })
        ));
    }

    #[test]
    fn reads_a_created_patient_back() {
        let conn = test_conn("read-back");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        let fetched = get_patient(&conn, &created.id).unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.full_name, "Ana Pérez");
    }

    #[test]
    fn updates_a_patient() {
        let conn = test_conn("update");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        let mut update = minimal_input("Ana María Pérez");
        update.phone = Some("+56911112222".to_string());
        let updated = update_patient(&conn, &created.id, update).unwrap();
        assert_eq!(updated.full_name, "Ana María Pérez");
        assert_eq!(updated.phone.as_deref(), Some("+56911112222"));
    }

    #[test]
    fn archiving_soft_deletes_and_hides_from_listing_but_keeps_the_row() {
        let conn = test_conn("archive-soft-delete");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();

        archive_patient(&conn, &created.id).unwrap();

        let listed = list_patients(&conn, None).unwrap();
        assert!(!listed.iter().any(|p| p.id == created.id), "no debe aparecer en el listado normal");

        let still_in_db = get_patient_including_deleted(&conn, &created.id).unwrap();
        assert!(still_in_db.deleted_at.is_some(), "el registro debe seguir existiendo en la base");
    }

    #[test]
    fn restoring_a_soft_deleted_patient_brings_it_back_to_the_listing() {
        let conn = test_conn("restore");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        archive_patient(&conn, &created.id).unwrap();

        restore_patient(&conn, &created.id).unwrap();

        let listed = list_patients(&conn, None).unwrap();
        assert!(listed.iter().any(|p| p.id == created.id));
    }

    #[test]
    fn searches_patients_by_name_against_the_real_database() {
        let conn = test_conn("search");
        create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        create_patient(&conn, minimal_input("Bruno Soto")).unwrap();

        let results = list_patients(&conn, Some("ana".to_string())).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].full_name, "Ana Pérez");

        let no_results = list_patients(&conn, Some("zzz_no_deberia_existir".to_string())).unwrap();
        assert!(no_results.is_empty());
    }

    #[test]
    fn list_items_never_include_the_rut_field() {
        // Verificación estructural: PatientListItem ni siquiera tiene un
        // campo `rut` — este test documenta esa garantía y falla en
        // tiempo de compilación (no en runtime) si alguien lo agrega sin
        // querer, porque el struct-literal de abajo dejaría de compilar
        // por campos inesperados si `PatientListItem` cambiara de forma
        // incompatible en otro lugar del archivo.
        let item = PatientListItem {
            id: "x".into(),
            full_name: "x".into(),
            preferred_name: None,
            status: "activo".into(),
            intake_date: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("rut"));
    }

    // --- Microcorrección post-Fase 11: 'archivado' no es seleccionable ---

    #[test]
    fn rejects_creating_a_patient_with_status_archivado() {
        let conn = test_conn("reject-create-archivado");
        let mut input = minimal_input("Ana Pérez");
        input.status = Some("archivado".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::ArchivedStatusIsNotManuallySelectable)
        ));
    }

    #[test]
    fn rejects_updating_a_patient_to_status_archivado_from_a_different_status() {
        let conn = test_conn("reject-update-archivado");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        assert_eq!(created.status, "activo");

        let mut update = minimal_input("Ana Pérez");
        update.status = Some("archivado".to_string());
        let err = update_patient(&conn, &created.id, update).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::ArchivedStatusIsNotManuallySelectable)
        ));

        // La fila no debe haber cambiado de estado por el intento rechazado.
        let unchanged = get_patient(&conn, &created.id).unwrap();
        assert_eq!(unchanged.status, "activo");
    }

    #[test]
    fn a_legacy_row_with_status_archivado_can_still_be_updated_without_changing_that_status() {
        // Simula una fila legacy anterior a esta corrección: `status =
        // 'archivado'` escrito directamente (no vía `create_patient`, que ya
        // lo rechaza), sin que `deleted_at` esté seteado — exactamente la
        // inconsistencia que detectó la auditoría.
        let conn = test_conn("legacy-archivado-row");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        conn.execute("UPDATE patients SET status = 'archivado' WHERE id = ?1", rusqlite::params![created.id]).unwrap();

        // Reenviar el mismo status ya existente (como haría el formulario al
        // reabrir esta fila legacy sin tocar el campo Estado) debe seguir
        // funcionando — nunca romper la aplicación ni perder datos.
        let mut update = minimal_input("Ana María Pérez");
        update.status = Some("archivado".to_string());
        let updated = update_patient(&conn, &created.id, update).unwrap();
        assert_eq!(updated.status, "archivado");
        assert_eq!(updated.full_name, "Ana María Pérez");

        // Sigue pudiendo leerse con normalidad.
        let fetched = get_patient(&conn, &created.id).unwrap();
        assert_eq!(fetched.status, "archivado");
    }

    #[test]
    fn a_legacy_row_with_status_archivado_can_transition_to_a_normal_status() {
        let conn = test_conn("legacy-archivado-transition");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        conn.execute("UPDATE patients SET status = 'archivado' WHERE id = ?1", rusqlite::params![created.id]).unwrap();

        let mut update = minimal_input("Ana Pérez");
        update.status = Some("activo".to_string());
        let updated = update_patient(&conn, &created.id, update).unwrap();
        assert_eq!(updated.status, "activo");
    }

    #[test]
    fn archive_patient_and_restore_patient_never_touch_the_status_column() {
        let conn = test_conn("archive-restore-status-untouched");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        assert_eq!(created.status, "activo");

        archive_patient(&conn, &created.id).unwrap();
        let archived = get_patient_including_deleted(&conn, &created.id).unwrap();
        assert_eq!(archived.status, "activo", "archivar es exclusivamente deleted_at, nunca status");
        assert!(archived.deleted_at.is_some());

        restore_patient(&conn, &created.id).unwrap();
        let restored = get_patient(&conn, &created.id).unwrap();
        assert_eq!(restored.status, "activo");
        assert!(restored.deleted_at.is_none());
    }

    #[test]
    fn updating_a_nonexistent_patient_reports_not_found() {
        let conn = test_conn("update-not-found");
        let err = update_patient(&conn, "no-existe", minimal_input("Ana Pérez")).unwrap_err();
        assert!(matches!(err, PatientError::NotFound));
    }

    #[test]
    fn activo_inactivo_and_alta_remain_selectable_on_update() {
        let conn = test_conn("selectable-statuses-still-work");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        for status in ["inactivo", "alta", "activo"] {
            let mut update = minimal_input("Ana Pérez");
            update.status = Some(status.to_string());
            let updated = update_patient(&conn, &created.id, update).unwrap();
            assert_eq!(updated.status, status);
        }
    }

    #[test]
    fn archiving_a_nonexistent_patient_reports_not_found() {
        let conn = test_conn("archive-not-found");
        let err = archive_patient(&conn, "no-existe").unwrap_err();
        assert!(matches!(err, PatientError::NotFound));
    }

    #[test]
    fn archived_list_shows_only_soft_deleted_patients_and_hides_active_ones() {
        let conn = test_conn("archived-list");
        let active = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        let archived = create_patient(&conn, minimal_input("Bruno Soto")).unwrap();
        archive_patient(&conn, &archived.id).unwrap();

        let archived_list = list_archived_patients(&conn, None).unwrap();
        assert_eq!(archived_list.len(), 1);
        assert_eq!(archived_list[0].id, archived.id);
        assert!(!archived_list.iter().any(|p| p.id == active.id));

        let active_list = list_patients(&conn, None).unwrap();
        assert!(active_list.iter().any(|p| p.id == active.id));
        assert!(!active_list.iter().any(|p| p.id == archived.id));
    }

    #[test]
    fn restoring_a_patient_removes_it_from_the_archived_list() {
        let conn = test_conn("archived-list-restore");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        archive_patient(&conn, &created.id).unwrap();
        assert_eq!(list_archived_patients(&conn, None).unwrap().len(), 1);

        restore_patient(&conn, &created.id).unwrap();

        assert!(list_archived_patients(&conn, None).unwrap().is_empty());
    }

    #[test]
    fn searches_archived_patients_by_name() {
        let conn = test_conn("archived-search");
        let ana = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        let bruno = create_patient(&conn, minimal_input("Bruno Soto")).unwrap();
        archive_patient(&conn, &ana.id).unwrap();
        archive_patient(&conn, &bruno.id).unwrap();

        let results = list_archived_patients(&conn, Some("ana".to_string())).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].full_name, "Ana Pérez");
    }

    // --- Fase 6.1: ubicación geográfica -------------------------------

    #[test]
    fn creates_a_patient_without_location_by_default() {
        let conn = test_conn("geo-create-no-location");
        let p = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();
        assert!(p.region.is_none());
        assert!(p.commune.is_none());
    }

    #[test]
    fn accepts_a_valid_region_and_commune_pair() {
        let conn = test_conn("geo-valid-pair");
        let mut input = minimal_input("Ana Pérez");
        input.region = Some("Región Metropolitana de Santiago".to_string());
        input.commune = Some("Ñuñoa".to_string());
        let p = create_patient(&conn, input).unwrap();
        assert_eq!(p.region.as_deref(), Some("Región Metropolitana de Santiago"));
        assert_eq!(p.commune.as_deref(), Some("Ñuñoa"));
    }

    #[test]
    fn accepts_a_region_alone_without_commune() {
        let conn = test_conn("geo-region-alone");
        let mut input = minimal_input("Ana Pérez");
        input.region = Some("Región de Valparaíso".to_string());
        let p = create_patient(&conn, input).unwrap();
        assert_eq!(p.region.as_deref(), Some("Región de Valparaíso"));
        assert!(p.commune.is_none());
    }

    #[test]
    fn rejects_an_unknown_region() {
        let conn = test_conn("geo-unknown-region");
        let mut input = minimal_input("Ana Pérez");
        input.region = Some("Región Inventada".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(err, PatientError::Validation(PatientValidationError::UnknownRegion(_))));
    }

    #[test]
    fn rejects_a_commune_that_does_not_belong_to_a_real_region() {
        let conn = test_conn("geo-unknown-commune");
        let mut input = minimal_input("Ana Pérez");
        input.region = Some("Región Metropolitana de Santiago".to_string());
        input.commune = Some("Comuna Inventada".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::CommuneNotInRegion { .. })
        ));
    }

    #[test]
    fn rejects_a_commune_belonging_to_a_different_region() {
        let conn = test_conn("geo-commune-wrong-region");
        let mut input = minimal_input("Ana Pérez");
        // Quillota es real, pero de la Región de Valparaíso, no de la RM.
        input.region = Some("Región Metropolitana de Santiago".to_string());
        input.commune = Some("Quillota".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::CommuneNotInRegion { .. })
        ));
    }

    #[test]
    fn accepts_extranjero_without_commune() {
        let conn = test_conn("geo-extranjero-ok");
        let mut input = minimal_input("Ana Pérez");
        input.region = Some("Extranjero".to_string());
        let p = create_patient(&conn, input).unwrap();
        assert_eq!(p.region.as_deref(), Some("Extranjero"));
        assert!(p.commune.is_none());
    }

    #[test]
    fn rejects_extranjero_with_a_commune() {
        let conn = test_conn("geo-extranjero-with-commune");
        let mut input = minimal_input("Ana Pérez");
        input.region = Some("Extranjero".to_string());
        input.commune = Some("Ñuñoa".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::ForeignRegionCannotHaveCommune)
        ));
    }

    #[test]
    fn rejects_a_commune_without_a_region() {
        let conn = test_conn("geo-commune-without-region");
        let mut input = minimal_input("Ana Pérez");
        input.commune = Some("Ñuñoa".to_string());
        let err = create_patient(&conn, input).unwrap_err();
        assert!(matches!(
            err,
            PatientError::Validation(PatientValidationError::CommuneRequiresRegion)
        ));
    }

    #[test]
    fn blank_region_and_commune_are_treated_as_not_informed() {
        let conn = test_conn("geo-blank-strings");
        let mut input = minimal_input("Ana Pérez");
        input.region = Some("   ".to_string());
        input.commune = Some("".to_string());
        let p = create_patient(&conn, input).unwrap();
        assert!(p.region.is_none());
        assert!(p.commune.is_none());
    }

    #[test]
    fn updates_a_patients_location_including_clearing_it_back_to_not_informed() {
        let conn = test_conn("geo-update");
        let created = create_patient(&conn, minimal_input("Ana Pérez")).unwrap();

        let mut with_location = minimal_input("Ana Pérez");
        with_location.region = Some("Región de Ñuble".to_string());
        with_location.commune = Some("Chillán".to_string());
        let updated = update_patient(&conn, &created.id, with_location).unwrap();
        assert_eq!(updated.region.as_deref(), Some("Región de Ñuble"));
        assert_eq!(updated.commune.as_deref(), Some("Chillán"));

        let cleared = update_patient(&conn, &created.id, minimal_input("Ana Pérez")).unwrap();
        assert!(cleared.region.is_none());
        assert!(cleared.commune.is_none());
    }

    fn create_patient_in(conn: &Connection, name: &str, region: &str, commune: &str) -> Patient {
        let mut input = minimal_input(name);
        input.region = Some(region.to_string());
        input.commune = Some(commune.to_string());
        create_patient(conn, input).unwrap()
    }

    #[test]
    fn geographic_statistics_counts_with_and_without_location() {
        let conn = test_conn("geo-stats-with-without");
        create_patient_in(&conn, "Ana Pérez", "Región de Valparaíso", "Quillota");
        create_patient_in(&conn, "Bruno Soto", "Región de Valparaíso", "Quillota");
        create_patient_in(&conn, "Carla Muñoz", "Región de Valparaíso", "Quillota");
        create_patient(&conn, minimal_input("Diego Ruiz")).unwrap();

        let stats = geographic_statistics(&conn, false, false).unwrap();
        assert_eq!(stats.with_location, 3);
        assert_eq!(stats.without_location, 1);
    }

    #[test]
    fn categories_with_fewer_than_three_patients_are_grouped_into_otras() {
        let conn = test_conn("geo-stats-otras-threshold");
        // Región de Valparaíso: 3 pacientes en Quillota (no se agrupa) y
        // 2 en Viña del Mar (se agrupa en "Otras").
        create_patient_in(&conn, "P1", "Región de Valparaíso", "Quillota");
        create_patient_in(&conn, "P2", "Región de Valparaíso", "Quillota");
        create_patient_in(&conn, "P3", "Región de Valparaíso", "Quillota");
        create_patient_in(&conn, "P4", "Región de Valparaíso", "Viña del Mar");
        create_patient_in(&conn, "P5", "Región de Valparaíso", "Viña del Mar");

        let stats = geographic_statistics(&conn, false, true).unwrap();

        // Por región: los 5 pacientes están en la misma región (5 >= 3), no
        // se agrupa.
        assert_eq!(stats.by_region, vec![GeoDistributionItem { label: "Región de Valparaíso".to_string(), count: 5 }]);

        // Por comuna: Quillota (3) queda individual; Viña del Mar (2) se
        // agrupa en "Otras".
        assert_eq!(
            stats.by_commune,
            vec![
                GeoDistributionItem { label: "Quillota".to_string(), count: 3 },
                GeoDistributionItem { label: "Otras".to_string(), count: 2 },
            ]
        );
    }

    #[test]
    fn a_category_with_exactly_two_patients_is_grouped_and_with_exactly_three_is_not() {
        let conn = test_conn("geo-stats-boundary");
        create_patient_in(&conn, "P1", "Región de Ñuble", "Chillán");
        create_patient_in(&conn, "P2", "Región de Ñuble", "Chillán");
        create_patient_in(&conn, "P3", "Región del Biobío", "Concepción");
        create_patient_in(&conn, "P4", "Región del Biobío", "Concepción");
        create_patient_in(&conn, "P5", "Región del Biobío", "Concepción");

        let stats = geographic_statistics(&conn, false, true).unwrap();

        let nuble = stats.by_region.iter().find(|r| r.label == "Otras");
        assert_eq!(nuble.map(|r| r.count), Some(2), "Ñuble con 2 pacientes debe caer en Otras");
        let biobio = stats.by_region.iter().find(|r| r.label == "Región del Biobío");
        assert_eq!(biobio.map(|r| r.count), Some(3), "Biobío con 3 pacientes no debe agruparse");
    }

    #[test]
    fn suppress_small_categories_false_shows_every_category_even_with_a_single_patient() {
        let conn = test_conn("geo-stats-no-suppression");
        create_patient_in(&conn, "P1", "Región de Valparaíso", "Quillota");
        create_patient_in(&conn, "P2", "Región de Valparaíso", "Quillota");
        create_patient_in(&conn, "P3", "Región de Valparaíso", "Viña del Mar");
        create_patient_in(&conn, "P4", "Región de Ñuble", "Chillán");

        let stats = geographic_statistics(&conn, false, false).unwrap();

        // Ninguna categoría debe fusionarse en "Otras", ni siquiera Viña del
        // Mar (1 paciente) o Región de Ñuble (1 paciente).
        assert!(stats.by_region.iter().all(|r| r.label != "Otras"));
        assert!(stats.by_commune.iter().all(|c| c.label != "Otras"));
        assert_eq!(
            stats.by_commune,
            vec![
                GeoDistributionItem { label: "Quillota".to_string(), count: 2 },
                GeoDistributionItem { label: "Chillán".to_string(), count: 1 },
                GeoDistributionItem { label: "Viña del Mar".to_string(), count: 1 },
            ]
        );
        assert_eq!(
            stats.by_region,
            vec![
                GeoDistributionItem { label: "Región de Valparaíso".to_string(), count: 3 },
                GeoDistributionItem { label: "Región de Ñuble".to_string(), count: 1 },
            ]
        );
    }

    #[test]
    fn geographic_statistics_excludes_archived_patients_by_default_and_includes_them_when_asked() {
        let conn = test_conn("geo-stats-archived");
        let a = create_patient_in(&conn, "P1", "Región de Valparaíso", "Quillota");
        create_patient_in(&conn, "P2", "Región de Valparaíso", "Quillota");
        create_patient_in(&conn, "P3", "Región de Valparaíso", "Quillota");
        archive_patient(&conn, &a.id).unwrap();

        let active_only = geographic_statistics(&conn, false, false).unwrap();
        assert_eq!(active_only.with_location, 2);

        let including_archived = geographic_statistics(&conn, true, false).unwrap();
        assert_eq!(including_archived.with_location, 3);
    }

    #[test]
    fn geographic_distribution_items_never_carry_identifying_patient_data() {
        // Verificación estructural: GeoDistributionItem solo tiene `label` y
        // `count` — no hay ningún campo por el que un id/nombre/RUT de
        // paciente pudiera colarse en la respuesta serializada a JSON.
        let item = GeoDistributionItem { label: "Región de Valparaíso".to_string(), count: 5 };
        let json = serde_json::to_string(&item).unwrap();
        assert_eq!(json, r#"{"label":"Región de Valparaíso","count":5}"#);
    }

    #[test]
    fn geographic_statistics_with_no_patients_returns_empty_distributions() {
        let conn = test_conn("geo-stats-empty");
        let stats = geographic_statistics(&conn, false, false).unwrap();
        assert_eq!(stats.with_location, 0);
        assert_eq!(stats.without_location, 0);
        assert!(stats.by_region.is_empty());
        assert!(stats.by_commune.is_empty());
    }

    // ---- hard_delete_patient / hard_delete_scope (FASE 2B) ----

    #[test]
    fn hard_delete_rejects_an_active_patient() {
        let (session, files_root) = test_session("hard-delete-active-rejected");
        let id = session.with_connection(|conn| create_patient(conn, minimal_input("Paciente Activo"))).unwrap().unwrap().id;
        let err = hard_delete_patient(&session, &files_root, &id).unwrap_err();
        assert!(matches!(err, PatientError::MustBeArchivedFirst));
        // nunca borra nada si rechaza
        assert!(session.with_connection(|conn| get_patient(conn, &id)).unwrap().is_ok());
    }

    #[test]
    fn hard_delete_removes_a_patient_with_no_related_data_at_all() {
        let (session, files_root) = test_session("hard-delete-empty-patient");
        let id = session.with_connection(|conn| create_patient(conn, minimal_input("Paciente Vacío"))).unwrap().unwrap().id;
        session.with_connection(|conn| archive_patient(conn, &id)).unwrap().unwrap();

        hard_delete_patient(&session, &files_root, &id).unwrap();
        let err = session.with_connection(|conn| get_patient(conn, &id)).unwrap().unwrap_err();
        assert!(matches!(err, PatientError::NotFound));
    }

    /// El caso más exigente: un paciente con al menos una fila en cada tabla relacionada
    /// auditada (ver `docs/patients-vertical.md`), incluido un documento propio con archivo real
    /// en disco y una asociación de Biblioteca. Verifica en un solo test: (a) todas las tablas
    /// quedan en cero filas para este paciente, (b) el archivo cifrado del documento se borra del
    /// disco, (c) el recurso global de Biblioteca sobrevive intacto (solo se borra la relación),
    /// y (d) un segundo paciente de control con su propia sesión no se ve afectado en absoluto.
    #[test]
    fn hard_delete_removes_every_related_row_across_every_audited_table_and_nothing_else() {
        let (session, files_root) = test_session("hard-delete-kitchen-sink");

        let patient_id = session.with_connection(|conn| create_patient(conn, minimal_input("Paciente Completo"))).unwrap().unwrap().id;
        let control_id = session.with_connection(|conn| create_patient(conn, minimal_input("Paciente Control"))).unwrap().unwrap().id;

        // Documento propio con archivo real en disco.
        let document_id = uuid::Uuid::new_v4();
        let storage_path = crate::services::document_crypto::opaque_storage_path(document_id);
        let abs_path = crate::services::document_crypto::resolve_within_files_root(&files_root, &storage_path).unwrap();
        std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
        std::fs::write(&abs_path, b"ciphertext ficticio").unwrap();

        // Recurso de Biblioteca global, asociado al paciente — debe sobrevivir.
        let resource = crate::services::library::create_resource(
            &session,
            &files_root,
            crate::services::library::NewLibraryResourceInput { title: "Guía ficticia".to_string(), resource_type: None, author: None, source_url: None, summary: None, source_path: None, mime_type: None },
        )
        .unwrap();

        session
            .with_connection(|conn| -> rusqlite::Result<()> {
                conn.execute("INSERT INTO patient_clinical_profile (patient_id, presenting_problem) VALUES (?1, 'motivo ficticio')", [&patient_id])?;

                conn.execute(
                    "INSERT INTO case_formulations (id, patient_id, title) VALUES ('cf1', ?1, 'Formulación ficticia')",
                    [&patient_id],
                )?;
                conn.execute("INSERT INTO formulation_versions (id, formulation_id, version_number) VALUES ('fv1', 'cf1', 1)", [])?;
                conn.execute(
                    "INSERT INTO formulation_nodes (id, formulation_version_id, node_type, label, position_x, position_y) VALUES ('n1', 'fv1', 'factor', 'Nodo 1', 0, 0)",
                    [],
                )?;
                conn.execute(
                    "INSERT INTO formulation_nodes (id, formulation_version_id, node_type, label, position_x, position_y) VALUES ('n2', 'fv1', 'factor', 'Nodo 2', 1, 1)",
                    [],
                )?;
                conn.execute("INSERT INTO formulation_edges (id, formulation_version_id, source_node_id, target_node_id) VALUES ('e1', 'fv1', 'n1', 'n2')", [])?;

                conn.execute(
                    "INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES ('te1', ?1, '2026-01-01', 'activo')",
                    [&patient_id],
                )?;
                conn.execute("INSERT INTO episode_clinical_profile (episode_id, presenting_problem) VALUES ('te1', 'motivo')", [])?;
                conn.execute(
                    "INSERT INTO episode_closures (id, episode_id, closed_at, reason, outcome) VALUES ('ec1', 'te1', '2026-02-01', 'alta', 'objetivos_logrados')",
                    [],
                )?;

                conn.execute(
                    "INSERT INTO sessions (id, patient_id, session_date, episode_id) VALUES ('s1', ?1, '2026-01-05', 'te1')",
                    [&patient_id],
                )?;
                conn.execute("INSERT INTO session_notes (id, session_id, content) VALUES ('sn1', 's1', 'nota ficticia')", [])?;

                conn.execute(
                    "INSERT INTO therapeutic_goals (id, patient_id, episode_id, title) VALUES ('g1', ?1, 'te1', 'Objetivo ficticio')",
                    [&patient_id],
                )?;
                conn.execute("INSERT INTO goal_indicators (id, goal_id, description) VALUES ('gi1', 'g1', 'indicador ficticio')", [])?;
                conn.execute("INSERT INTO goal_interventions (id, goal_id, description) VALUES ('gv1', 'g1', 'intervención ficticia')", [])?;
                conn.execute("INSERT INTO session_goals (session_id, goal_id) VALUES ('s1', 'g1')", [])?;

                conn.execute("INSERT INTO assessment_instruments (id, name) VALUES ('ai1', 'Instrumento Ficticio')", [])?;
                conn.execute(
                    "INSERT INTO assessment_administrations (id, patient_id, instrument_id, administered_at) VALUES ('aa1', ?1, 'ai1', '2026-01-10')",
                    [&patient_id],
                )?;

                conn.execute("INSERT INTO payments (id, patient_id, amount) VALUES ('pay1', ?1, 10000)", [&patient_id])?;
                conn.execute("INSERT INTO patient_prep_notes (id, patient_id, content) VALUES ('ppn1', ?1, 'nota de preparación ficticia')", [&patient_id])?;
                conn.execute("INSERT INTO therapy_tasks (id, patient_id, description) VALUES ('tt1', ?1, 'tarea ficticia')", [&patient_id])?;

                conn.execute("INSERT INTO safety_plans (id, patient_id, version, status) VALUES ('sp1', ?1, 1, 'borrador')", [&patient_id])?;
                conn.execute("INSERT INTO safety_plan_contacts (id, safety_plan_id, contact_type, name) VALUES ('spc1', 'sp1', 'support_person', 'Contacto Ficticio')", [])?;
                conn.execute("INSERT INTO safety_plan_list_items (id, safety_plan_id, item_type, content) VALUES ('spli1', 'sp1', 'warning_sign', 'Señal ficticia')", [])?;

                conn.execute(
                    "INSERT INTO documents (id, patient_id, original_filename, mime_type, size_bytes, sha256_plaintext, storage_path, wrapped_file_dek, wrap_nonce) \
                     VALUES (?1, ?2, 'informe.pdf', 'application/pdf', 20, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', ?3, 'a2V5', 'bm9uY2U=')",
                    rusqlite::params![document_id.to_string(), patient_id, storage_path],
                )?;

                conn.execute(
                    "INSERT INTO appointments (id, patient_id, title, starts_at, ends_at) VALUES ('apt1', ?1, 'Cita ficticia', '2026-01-01T10:00:00Z', '2026-01-01T11:00:00Z')",
                    [&patient_id],
                )?;
                conn.execute("INSERT INTO reminders (id, patient_id, title) VALUES ('rem1', ?1, 'Recordatorio ficticio')", [&patient_id])?;

                conn.execute("INSERT INTO library_resource_patients (resource_id, patient_id) VALUES (?1, ?2)", rusqlite::params![resource.id, patient_id])?;

                // Datos del paciente de control — deben sobrevivir intactos.
                conn.execute("INSERT INTO sessions (id, patient_id, session_date) VALUES ('s-control', ?1, '2026-01-05')", [&control_id])?;
                Ok(())
            })
            .unwrap()
            .unwrap();

        // El resumen de alcance ve todo lo insertado.
        let scope = session.with_connection(|conn| hard_delete_scope(conn, &patient_id)).unwrap().unwrap();
        assert!(scope.has_clinical_profile);
        assert_eq!(scope.sessions, 1);
        assert_eq!(scope.case_formulations, 1);
        assert_eq!(scope.therapeutic_goals, 1);
        assert_eq!(scope.assessment_administrations, 1);
        assert_eq!(scope.payments, 1);
        assert_eq!(scope.treatment_episodes, 1);
        assert_eq!(scope.safety_plans, 1);
        assert_eq!(scope.documents, 1);
        assert_eq!(scope.appointments, 1);
        assert_eq!(scope.reminders, 1);
        assert_eq!(scope.library_associations, 1);

        session.with_connection(|conn| archive_patient(conn, &patient_id)).unwrap().unwrap();
        hard_delete_patient(&session, &files_root, &patient_id).unwrap();

        // El paciente en sí y cada tabla relacionada quedan en cero para este id.
        let err = session.with_connection(|conn| get_patient(conn, &patient_id)).unwrap().unwrap_err();
        assert!(matches!(err, PatientError::NotFound));

        session
            .with_connection(|conn| -> rusqlite::Result<()> {
                for (table, column) in [
                    ("patient_clinical_profile", "patient_id"),
                    ("case_formulations", "patient_id"),
                    ("treatment_episodes", "patient_id"),
                    ("sessions", "patient_id"),
                    ("therapeutic_goals", "patient_id"),
                    ("assessment_administrations", "patient_id"),
                    ("payments", "patient_id"),
                    ("patient_prep_notes", "patient_id"),
                    ("therapy_tasks", "patient_id"),
                    ("safety_plans", "patient_id"),
                    ("documents", "patient_id"),
                    ("appointments", "patient_id"),
                    ("reminders", "patient_id"),
                    ("library_resource_patients", "patient_id"),
                ] {
                    let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"), [&patient_id], |r| r.get(0))?;
                    assert_eq!(count, 0, "{table} debería quedar en cero filas para el paciente eliminado");
                }
                for table in ["formulation_versions", "formulation_nodes", "formulation_edges", "episode_clinical_profile", "episode_closures", "session_notes", "goal_indicators", "goal_interventions", "session_goals", "safety_plan_contacts", "safety_plan_list_items"] {
                    let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
                    assert_eq!(count, 0, "{table} (hoja transitiva) debería quedar vacía");
                }
                Ok(())
            })
            .unwrap()
            .unwrap();

        // El ciphertext del documento del paciente se borró del disco — ningún archivo huérfano.
        assert!(!abs_path.exists(), "el ciphertext del documento del paciente debe borrarse");

        // El recurso global de Biblioteca sobrevive intacto — solo se borró la relación.
        let survives = session.with_connection(|conn| crate::services::library::get_resource(conn, &resource.id)).unwrap();
        assert!(survives.is_ok(), "el recurso global de Biblioteca nunca debe borrarse al eliminar un paciente");

        // El paciente de control y su sesión no se ven afectados en absoluto.
        assert!(session.with_connection(|conn| get_patient(conn, &control_id)).unwrap().is_ok());
        let control_sessions: i64 = session
            .with_connection(|conn| conn.query_row("SELECT COUNT(*) FROM sessions WHERE patient_id = ?1", [&control_id], |r| r.get(0)))
            .unwrap()
            .unwrap();
        assert_eq!(control_sessions, 1, "el paciente de control no debe verse afectado");
    }
}
