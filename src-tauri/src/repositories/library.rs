//! Acceso a datos de la Biblioteca global de recursos (fase de continuación post-Fase 19). SQL
//! puro — sin reglas de negocio (eso vive en `services::library`).
//!
//! `library_resources` guarda solo metadata bibliográfica (título, tipo, autor, URL, resumen); el
//! archivo real de un recurso, si lo tiene, es una fila de `documents` con `patient_id = NULL`
//! referenciada por `file_document_id` — nunca una segunda tabla de archivos ni una segunda
//! implementación de cifrado (ver el comentario de `SCHEMA_V12` en `db::migrations`).
//! `library_resource_patients` es la relación N:M con pacientes: un mismo recurso puede asociarse
//! a cualquier número de pacientes sin duplicar nunca el archivo físico.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::repositories::patients::PatientSummary;

/// Fila completa de `library_resources`. Nunca lleva ninguna columna criptográfica — esas viven
/// exclusivamente en `documents`, referenciada por `file_document_id`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryResource {
    pub id: String,
    pub title: String,
    pub resource_type: Option<String>,
    pub author: Option<String>,
    pub source_url: Option<String>,
    pub file_document_id: Option<String>,
    pub summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// Proyección para listar/buscar/abrir — incluye la metadata del archivo adjunto (si existe),
/// obtenida con un `LEFT JOIN` a `documents`, nunca ninguna columna criptográfica de esa tabla.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryResourceSummary {
    pub id: String,
    pub title: String,
    pub resource_type: Option<String>,
    pub author: Option<String>,
    pub source_url: Option<String>,
    pub summary: Option<String>,
    pub file_document_id: Option<String>,
    pub original_filename: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewLibraryResourceRow<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub resource_type: Option<&'a str>,
    pub author: Option<&'a str>,
    pub source_url: Option<&'a str>,
    pub file_document_id: Option<&'a str>,
    pub summary: Option<&'a str>,
}

pub struct LibraryResourceMetadataUpdate<'a> {
    pub title: &'a str,
    pub resource_type: Option<&'a str>,
    pub author: Option<&'a str>,
    pub source_url: Option<&'a str>,
    pub summary: Option<&'a str>,
}

const RESOURCE_COLUMNS: &str = "id, title, resource_type, author, source_url, file_document_id, summary, created_at, updated_at, deleted_at";

const SUMMARY_SELECT: &str = "SELECT r.id, r.title, r.resource_type, r.author, r.source_url, r.summary, r.file_document_id, \
     d.original_filename, d.mime_type, d.size_bytes, r.created_at, r.updated_at \
     FROM library_resources r LEFT JOIN documents d ON d.id = r.file_document_id";

fn map_resource_row(row: &Row) -> rusqlite::Result<LibraryResource> {
    Ok(LibraryResource {
        id: row.get(0)?,
        title: row.get(1)?,
        resource_type: row.get(2)?,
        author: row.get(3)?,
        source_url: row.get(4)?,
        file_document_id: row.get(5)?,
        summary: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        deleted_at: row.get(9)?,
    })
}

fn map_summary_row(row: &Row) -> rusqlite::Result<LibraryResourceSummary> {
    Ok(LibraryResourceSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        resource_type: row.get(2)?,
        author: row.get(3)?,
        source_url: row.get(4)?,
        summary: row.get(5)?,
        file_document_id: row.get(6)?,
        original_filename: row.get(7)?,
        mime_type: row.get(8)?,
        size_bytes: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

pub fn insert(conn: &Connection, row: &NewLibraryResourceRow) -> rusqlite::Result<LibraryResource> {
    conn.execute(
        "INSERT INTO library_resources (id, title, resource_type, author, source_url, file_document_id, summary) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![row.id, row.title, row.resource_type, row.author, row.source_url, row.file_document_id, row.summary],
    )?;
    find_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

pub fn find_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<LibraryResource>> {
    conn.query_row(&format!("SELECT {RESOURCE_COLUMNS} FROM library_resources WHERE id = ?1"), params![id], map_resource_row).optional()
}

pub fn find_summary_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<LibraryResourceSummary>> {
    conn.query_row(&format!("{SUMMARY_SELECT} WHERE r.id = ?1"), params![id], map_summary_row).optional()
}

/// Recursos activos, alfabético por título — el listado principal de la Biblioteca. `search`/
/// `sort` (por nombre de archivo, fecha, etc.) se resuelven en el frontend sobre esta misma lista,
/// mismo criterio ya usado por `PatientsListScreen` (búsqueda por nombre en cliente).
pub fn list_active(conn: &Connection) -> rusqlite::Result<Vec<LibraryResourceSummary>> {
    let sql = format!("{SUMMARY_SELECT} WHERE r.deleted_at IS NULL ORDER BY r.title COLLATE NOCASE");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_summary_row)?;
    rows.collect()
}

pub fn list_archived(conn: &Connection) -> rusqlite::Result<Vec<LibraryResourceSummary>> {
    let sql = format!("{SUMMARY_SELECT} WHERE r.deleted_at IS NOT NULL ORDER BY r.title COLLATE NOCASE");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_summary_row)?;
    rows.collect()
}

/// Solo metadata bibliográfica — nunca `file_document_id` (el archivo adjunto es inmutable una
/// vez creado el recurso, mismo criterio que Documentos: "reemplazar el archivo" no existe).
pub fn update_metadata(conn: &Connection, id: &str, update: &LibraryResourceMetadataUpdate) -> rusqlite::Result<Option<LibraryResource>> {
    let affected = conn.execute(
        "UPDATE library_resources SET title = ?1, resource_type = ?2, author = ?3, source_url = ?4, summary = ?5 \
         WHERE id = ?6 AND deleted_at IS NULL",
        params![update.title, update.resource_type, update.author, update.source_url, update.summary, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_by_id(conn, id)
}

/// Soft delete — nunca borra el archivo físico ni ninguna fila de `library_resource_patients`
/// (los enlaces sobreviven intactos, restaurables junto con el recurso).
pub fn archive(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE library_resources SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("UPDATE library_resources SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL", params![id])?;
    Ok(affected > 0)
}

// ---- library_resource_patients (relación N:M) ----

/// Idempotente: si el enlace ya existía, no falla ni lo duplica (`INSERT OR IGNORE`) — devuelve si
/// se creó una fila nueva.
pub fn link_to_patient(conn: &Connection, resource_id: &str, patient_id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "INSERT OR IGNORE INTO library_resource_patients (resource_id, patient_id) VALUES (?1, ?2)",
        params![resource_id, patient_id],
    )?;
    Ok(affected > 0)
}

pub fn unlink_from_patient(conn: &Connection, resource_id: &str, patient_id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "DELETE FROM library_resource_patients WHERE resource_id = ?1 AND patient_id = ?2",
        params![resource_id, patient_id],
    )?;
    Ok(affected > 0)
}

pub fn count_patients_for_resource(conn: &Connection, resource_id: &str) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM library_resource_patients WHERE resource_id = ?1", params![resource_id], |r| r.get(0))
}

/// Pacientes asociados a un recurso — mismo `PatientSummary` minimizado ya usado por el listado
/// de pacientes, nunca la ficha completa.
pub fn list_patients_for_resource(conn: &Connection, resource_id: &str) -> rusqlite::Result<Vec<PatientSummary>> {
    let sql = "SELECT p.id, p.full_name, p.preferred_name, p.status, p.intake_date \
               FROM library_resource_patients lp JOIN patients p ON p.id = lp.patient_id \
               WHERE lp.resource_id = ?1 ORDER BY p.full_name COLLATE NOCASE";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![resource_id], |row| {
        Ok(PatientSummary {
            id: row.get(0)?,
            full_name: row.get(1)?,
            preferred_name: row.get(2)?,
            status: row.get(3)?,
            intake_date: row.get(4)?,
        })
    })?;
    rows.collect()
}

/// Recursos activos asociados a un paciente — la vista "Biblioteca" desde la ficha del paciente.
pub fn list_resources_for_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<LibraryResourceSummary>> {
    let sql = format!(
        "{SUMMARY_SELECT} JOIN library_resource_patients lp ON lp.resource_id = r.id \
         WHERE lp.patient_id = ?1 AND r.deleted_at IS NULL ORDER BY r.title COLLATE NOCASE"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], map_summary_row)?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-library-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x4Cu8; VAULT_KEY_LEN]);
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

    fn minimal_row<'a>(id: &'a str, title: &'a str) -> NewLibraryResourceRow<'a> {
        NewLibraryResourceRow { id, title, resource_type: None, author: None, source_url: None, file_document_id: None, summary: None }
    }

    #[test]
    fn inserts_and_finds_a_resource() {
        let conn = test_conn("insert-find");
        let resource = insert(&conn, &minimal_row("r1", "Guía ficticia")).unwrap();
        assert_eq!(resource.title, "Guía ficticia");
        assert!(resource.deleted_at.is_none());
    }

    #[test]
    fn list_active_orders_alphabetically_and_excludes_archived() {
        let conn = test_conn("list-active");
        insert(&conn, &minimal_row("r1", "Zeta")).unwrap();
        insert(&conn, &minimal_row("r2", "Alfa")).unwrap();
        insert(&conn, &minimal_row("r3", "Beta")).unwrap();
        archive(&conn, "r3").unwrap();

        let active = list_active(&conn).unwrap();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].title, "Alfa");
        assert_eq!(active[1].title, "Zeta");

        let archived = list_archived(&conn).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].title, "Beta");
    }

    #[test]
    fn update_metadata_never_touches_file_document_id() {
        let conn = test_conn("update-metadata");
        insert(&conn, &minimal_row("r1", "Original")).unwrap();
        let updated = update_metadata(
            &conn,
            "r1",
            &LibraryResourceMetadataUpdate { title: "Editado", resource_type: Some("protocolo"), author: Some("Autora"), source_url: None, summary: Some("Resumen") },
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.title, "Editado");
        assert_eq!(updated.resource_type.as_deref(), Some("protocolo"));
        assert!(updated.file_document_id.is_none());
    }

    #[test]
    fn archive_and_restore_round_trip() {
        let conn = test_conn("archive-restore");
        insert(&conn, &minimal_row("r1", "Recurso")).unwrap();
        assert!(archive(&conn, "r1").unwrap());
        assert_eq!(list_active(&conn).unwrap().len(), 0);
        assert!(restore(&conn, "r1").unwrap());
        assert_eq!(list_active(&conn).unwrap().len(), 1);
    }

    #[test]
    fn linking_is_idempotent_and_unlinking_removes_exactly_one_link() {
        let conn = test_conn("link-idempotent");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        insert(&conn, &minimal_row("r1", "Recurso")).unwrap();

        assert!(link_to_patient(&conn, "r1", &patient_id).unwrap());
        assert!(!link_to_patient(&conn, "r1", &patient_id).unwrap(), "un segundo enlace idéntico no debe duplicarse");
        assert_eq!(count_patients_for_resource(&conn, "r1").unwrap(), 1);

        assert!(unlink_from_patient(&conn, "r1", &patient_id).unwrap());
        assert_eq!(count_patients_for_resource(&conn, "r1").unwrap(), 0);
        assert!(!unlink_from_patient(&conn, "r1", &patient_id).unwrap(), "desvincular algo ya desvinculado no debe reportar éxito");
    }

    #[test]
    fn lists_patients_for_a_resource_and_resources_for_a_patient() {
        let conn = test_conn("bidirectional-listing");
        let patient_a = create_test_patient(&conn, "Paciente A");
        let patient_b = create_test_patient(&conn, "Paciente B");
        insert(&conn, &minimal_row("r1", "Recurso Uno")).unwrap();
        insert(&conn, &minimal_row("r2", "Recurso Dos")).unwrap();

        link_to_patient(&conn, "r1", &patient_a).unwrap();
        link_to_patient(&conn, "r1", &patient_b).unwrap();
        link_to_patient(&conn, "r2", &patient_a).unwrap();

        let patients_of_r1 = list_patients_for_resource(&conn, "r1").unwrap();
        assert_eq!(patients_of_r1.len(), 2);

        let resources_of_a = list_resources_for_patient(&conn, &patient_a).unwrap();
        assert_eq!(resources_of_a.len(), 2);
        let resources_of_b = list_resources_for_patient(&conn, &patient_b).unwrap();
        assert_eq!(resources_of_b.len(), 1);
        assert_eq!(resources_of_b[0].id, "r1");
    }

    #[test]
    fn an_archived_resource_disappears_from_a_patients_active_list_but_the_link_survives() {
        let conn = test_conn("archived-link-survives");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        insert(&conn, &minimal_row("r1", "Recurso")).unwrap();
        link_to_patient(&conn, "r1", &patient_id).unwrap();

        archive(&conn, "r1").unwrap();

        assert_eq!(list_resources_for_patient(&conn, &patient_id).unwrap().len(), 0, "un recurso archivado no aparece en la lista activa del paciente");
        assert_eq!(count_patients_for_resource(&conn, "r1").unwrap(), 1, "archivar nunca borra la asociación");
    }

    #[test]
    fn summary_includes_the_linked_documents_file_metadata() {
        let conn = test_conn("summary-file-metadata");
        conn.execute(
            "INSERT INTO documents (id, patient_id, original_filename, mime_type, size_bytes, sha256_plaintext, storage_path, wrapped_file_dek, wrap_nonce) \
             VALUES ('d1', NULL, 'guia.pdf', 'application/pdf', 2048, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'files/ab/d1.enc', 'a2V5', 'bm9uY2U=')",
            [],
        )
        .unwrap();
        insert(
            &conn,
            &NewLibraryResourceRow { id: "r1", title: "Con archivo", resource_type: None, author: None, source_url: None, file_document_id: Some("d1"), summary: None },
        )
        .unwrap();

        let summary = find_summary_by_id(&conn, "r1").unwrap().unwrap();
        assert_eq!(summary.original_filename.as_deref(), Some("guia.pdf"));
        assert_eq!(summary.mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(summary.size_bytes, Some(2048));
    }

    #[test]
    fn a_resource_without_a_file_has_null_file_metadata_in_its_summary() {
        let conn = test_conn("summary-no-file");
        insert(&conn, &minimal_row("r1", "Solo enlace")).unwrap();
        let summary = find_summary_by_id(&conn, "r1").unwrap().unwrap();
        assert!(summary.original_filename.is_none());
        assert!(summary.mime_type.is_none());
        assert!(summary.size_bytes.is_none());
    }
}
