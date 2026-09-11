//! Acceso a datos de `documents` (Fase 16). SQL puro — sin reglas de negocio (eso vive en
//! `services::documents`). El cifrado/descifrado del contenido y la generación de rutas físicas
//! opacas viven en `services::document_crypto`, nunca aquí — este módulo solo persiste las
//! columnas que esas dos capas ya calcularon.
//!
//! `Document` (la fila completa, con `wrapped_file_dek`/`wrap_nonce`/`storage_path`) nunca deriva
//! `Serialize` — no puede cruzar IPC por accidente ni siquiera si algún comando futuro lo
//! retornara directamente. Lo que sí cruza IPC es `DocumentSummary`, que nunca lleva esas
//! columnas ni `sha256_plaintext` (Bloque 18 de la aprobación: "no mostrar storage_path; wrapped
//! key; nonce; hash; UUID físico; detalles criptográficos").

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

/// Fila completa de `documents`, incluida su metadata criptográfica. Nunca se serializa ni cruza
/// IPC — solo lo consume `services::documents` para operaciones que sí necesitan
/// `wrapped_file_dek`/`wrap_nonce`/`storage_path` (abrir, exportar, backup).
#[derive(Debug, Clone)]
pub struct Document {
    pub id: String,
    pub patient_id: Option<String>,
    pub episode_id: Option<String>,
    pub session_id: Option<String>,
    pub category: Option<String>,
    pub original_filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    /// Usado por `services::documents::get_document_content` como verificación adicional de
    /// integridad al descifrar — nunca se expone por IPC.
    pub sha256_plaintext: String,
    pub storage_path: String,
    /// Columna real del esquema (`SCHEMA_V1`), siempre `true` en esta fase — ver
    /// `repositories::documents::insert_document`. Se expone por fidelidad del tipo con la fila
    /// real; ningún código de esta fase necesita leerla de vuelta todavía.
    #[allow(dead_code)]
    pub is_clinical: bool,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Se filtra siempre a nivel de SQL (`WHERE deleted_at IS NULL/NOT NULL`) en cada función de
    /// `repositories::documents` — el código de servicio nunca necesita releer este campo desde
    /// una fila ya obtenida.
    #[allow(dead_code)]
    pub deleted_at: Option<String>,
    /// Base64 de la DEK de este archivo, envuelta con la DEK del vault
    /// (`security::VaultSession::wrap_file_key`). Nunca la clave en claro.
    pub wrapped_file_dek: String,
    /// Base64 del nonce usado al envolver `wrapped_file_dek`.
    pub wrap_nonce: String,
    pub format_version: i64,
    /// Esquema usado para envolver `wrapped_file_dek` (`SCHEMA_V10`, Fase 17/CRYPTO-1) — `1`
    /// (legacy, DEK del vault directa) o `2` (HKDF domain-separated). Columna cruda; la
    /// validación/tipado vive en `security::KeyWrapVersion`, consumida por
    /// `services::documents`. Deliberadamente independiente de `format_version` (formato físico
    /// del ciphertext CCD1) — ver `docs/documents.md`.
    pub key_wrap_version: i64,
}

impl Document {
    /// Proyección minimizada para IPC — nunca incluye `storage_path`, `wrapped_file_dek`,
    /// `wrap_nonce`, `format_version` ni `sha256_plaintext` (Bloque 18 de la aprobación).
    pub fn to_summary(&self) -> DocumentSummary {
        DocumentSummary {
            id: self.id.clone(),
            patient_id: self.patient_id.clone(),
            episode_id: self.episode_id.clone(),
            session_id: self.session_id.clone(),
            category: self.category.clone(),
            original_filename: self.original_filename.clone(),
            mime_type: self.mime_type.clone(),
            size_bytes: self.size_bytes,
            description: self.description.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

/// Fila de listado — minimización de IPC (mismo criterio que `FormulationSummary`/
/// `SafetyPlanSummary`/`AssessmentAdministrationSummary`): nunca lleva ninguna columna
/// criptográfica ni la ruta física.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummary {
    pub id: String,
    pub patient_id: Option<String>,
    pub episode_id: Option<String>,
    pub session_id: Option<String>,
    pub category: Option<String>,
    pub original_filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewDocumentRow<'a> {
    pub id: &'a str,
    pub patient_id: &'a str,
    pub episode_id: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub category: Option<&'a str>,
    pub original_filename: &'a str,
    pub mime_type: &'a str,
    pub size_bytes: i64,
    pub sha256_plaintext: &'a str,
    pub storage_path: &'a str,
    pub description: Option<&'a str>,
    pub wrapped_file_dek: &'a str,
    pub wrap_nonce: &'a str,
    /// Ver `Document::key_wrap_version`. Obligatorio (no `Option`, mismo criterio que
    /// `wrapped_file_dek`/`wrap_nonce`): `services::documents::create_document` siempre lo fija
    /// explícitamente a `KeyWrapVersion::DomainSeparated.as_i64()` (2) para documentos nuevos —
    /// nunca depende del `DEFAULT 1` de la columna, que existe únicamente para las filas creadas
    /// antes de esta fase.
    pub key_wrap_version: i64,
}

pub struct DocumentMetadataUpdate<'a> {
    pub category: Option<&'a str>,
    pub description: Option<&'a str>,
}

const DOCUMENT_COLUMNS: &str = "id, patient_id, episode_id, session_id, category, original_filename, mime_type, size_bytes, \
     sha256_plaintext, storage_path, is_clinical, description, created_at, updated_at, deleted_at, \
     wrapped_file_dek, wrap_nonce, format_version, key_wrap_version";

const SUMMARY_COLUMNS: &str =
    "id, patient_id, episode_id, session_id, category, original_filename, mime_type, size_bytes, description, created_at, updated_at";

fn map_document_row(row: &Row) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        episode_id: row.get(2)?,
        session_id: row.get(3)?,
        category: row.get(4)?,
        original_filename: row.get(5)?,
        mime_type: row.get(6)?,
        size_bytes: row.get(7)?,
        sha256_plaintext: row.get(8)?,
        storage_path: row.get(9)?,
        is_clinical: row.get::<_, i64>(10)? != 0,
        description: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        deleted_at: row.get(14)?,
        wrapped_file_dek: row.get(15)?,
        wrap_nonce: row.get(16)?,
        format_version: row.get(17)?,
        key_wrap_version: row.get(18)?,
    })
}

fn map_summary_row(row: &Row) -> rusqlite::Result<DocumentSummary> {
    Ok(DocumentSummary {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        episode_id: row.get(2)?,
        session_id: row.get(3)?,
        category: row.get(4)?,
        original_filename: row.get(5)?,
        mime_type: row.get(6)?,
        size_bytes: row.get(7)?,
        description: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

/// Inserta un documento nuevo. `is_clinical` siempre se inserta en `1` — esta fase no ofrece
/// ningún flujo para marcar un documento como no clínico (mismo criterio que `is_custom` en
/// `assessment_instruments`, Fase 13: la columna existe desde `SCHEMA_V1` pero esta fase no la
/// expone como una decisión de usuario).
pub fn insert_document(conn: &Connection, row: &NewDocumentRow) -> rusqlite::Result<Document> {
    conn.execute(
        "INSERT INTO documents (id, patient_id, episode_id, session_id, category, original_filename, mime_type, size_bytes, \
             sha256_plaintext, storage_path, is_clinical, description, wrapped_file_dek, wrap_nonce, format_version, key_wrap_version) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12, ?13, 1, ?14)",
        params![
            row.id,
            row.patient_id,
            row.episode_id,
            row.session_id,
            row.category,
            row.original_filename,
            row.mime_type,
            row.size_bytes,
            row.sha256_plaintext,
            row.storage_path,
            row.description,
            row.wrapped_file_dek,
            row.wrap_nonce,
            row.key_wrap_version,
        ],
    )?;
    find_document_by_id(conn, row.id).map(|opt| opt.expect("se acaba de insertar"))
}

/// Devuelve el documento exista o no `deleted_at` — igual criterio que
/// `repositories::formulations::find_formulation_by_id`. Incluye las columnas criptográficas:
/// pensado para uso interno de `services::documents` (abrir/exportar/backup), nunca para
/// devolverse tal cual por IPC.
pub fn find_document_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<Document>> {
    conn.query_row(&format!("SELECT {DOCUMENT_COLUMNS} FROM documents WHERE id = ?1"), params![id], map_document_row).optional()
}

/// Documentos activos de un paciente, más recientes primero.
pub fn list_documents_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<DocumentSummary>> {
    let sql = format!("SELECT {SUMMARY_COLUMNS} FROM documents WHERE patient_id = ?1 AND deleted_at IS NULL ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], map_summary_row)?;
    rows.collect()
}

/// Documentos archivados (soft-deleted) de un paciente, más recientes primero.
pub fn list_archived_documents_by_patient(conn: &Connection, patient_id: &str) -> rusqlite::Result<Vec<DocumentSummary>> {
    let sql = format!("SELECT {SUMMARY_COLUMNS} FROM documents WHERE patient_id = ?1 AND deleted_at IS NOT NULL ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![patient_id], map_summary_row)?;
    rows.collect()
}

/// Edita únicamente metadata administrativa (categoría/descripción) — nunca el contenido cifrado
/// ni ninguna asociación (paciente/proceso/sesión son inmutables tras crear el documento, mismo
/// criterio que el resto del proyecto no permite "mover" un registro clínico a otro paciente).
pub fn update_document_metadata(conn: &Connection, id: &str, update: &DocumentMetadataUpdate) -> rusqlite::Result<Option<Document>> {
    let affected = conn.execute(
        "UPDATE documents SET category = ?1, description = ?2 WHERE id = ?3 AND deleted_at IS NULL",
        params![update.category, update.description, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    find_document_by_id(conn, id)
}

/// Soft delete únicamente. No existe, en ningún punto de este módulo, una operación de borrado
/// físico del ciphertext alcanzable desde un comando normal — ver `docs/documents.md`.
pub fn archive_document(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute(
        "UPDATE documents SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn restore_document(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("UPDATE documents SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL", params![id])?;
    Ok(affected > 0)
}

/// Todos los `storage_path` de documentos no archivados físicamente (activos o archivados
/// lógicamente — el ciphertext de un documento archivado sigue existiendo, ver Bloque 17 de la
/// aprobación) — usado por el reconciliador de arranque y por Backup para saber qué ciphertexts
/// deberían existir físicamente.
pub fn list_all_storage_paths(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT storage_path FROM documents")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_vault, run_migrations, VaultKey, VAULT_KEY_LEN};
    use crate::repositories::patients::{self, NewPatientRow};
    use crate::repositories::sessions::{self, NewSessionRow};
    use crate::repositories::treatment_episodes::{self, NewTreatmentEpisodeRow};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-documents-repo-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x91u8; VAULT_KEY_LEN]);
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

    fn create_test_episode(conn: &Connection, patient_id: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        treatment_episodes::insert(conn, &NewTreatmentEpisodeRow { id: &id, patient_id, started_at: "2026-01-01", status: "activo" }).unwrap();
        id
    }

    fn create_test_session(conn: &Connection, patient_id: &str, episode_id: Option<&str>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        sessions::insert(
            conn,
            &NewSessionRow {
                id: &id,
                patient_id,
                appointment_id: None,
                episode_id,
                session_date: "2026-01-05",
                start_time: None,
                duration_minutes: None,
                modality: None,
                status: "realizada",
            },
        )
        .unwrap();
        id
    }

    const SAMPLE_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn sample_row<'a>(id: &'a str, patient_id: &'a str) -> NewDocumentRow<'a> {
        // `storage_path` debe ser único por fila (misma restricción UNIQUE que en producción) —
        // se deriva del `id` de prueba y se "leakea" solo para simplificar la firma de esta
        // función auxiliar de test (vive el resto del proceso de test, sin impacto real).
        let storage_path: &'static str = Box::leak(format!("files/ab/{id}.enc").into_boxed_str());
        NewDocumentRow {
            id,
            patient_id,
            episode_id: None,
            session_id: None,
            category: Some("informe"),
            original_filename: "informe.pdf",
            mime_type: "application/pdf",
            size_bytes: 1024,
            sha256_plaintext: SAMPLE_SHA256,
            storage_path,
            description: None,
            wrapped_file_dek: "d2VsbA==",
            wrap_nonce: "bm9uY2U=",
            key_wrap_version: 2,
        }
    }

    #[test]
    fn inserts_and_finds_a_document() {
        let conn = test_conn("insert-find");
        let patient_id = create_test_patient(&conn, "Paciente Uno");
        let doc = insert_document(&conn, &sample_row("d1", &patient_id)).unwrap();
        assert_eq!(doc.original_filename, "informe.pdf");
        assert_eq!(doc.category.as_deref(), Some("informe"));
        assert!(doc.is_clinical);
        assert_eq!(doc.format_version, 1);
        assert_eq!(doc.key_wrap_version, 2, "sample_row simula lo que produce el código de creación actual: siempre 2");
        assert!(find_document_by_id(&conn, "d1").unwrap().is_some());
    }

    #[test]
    fn key_wrap_version_persists_exactly_as_given_for_both_supported_values() {
        let conn = test_conn("key-wrap-version-persists");
        let patient_id = create_test_patient(&conn, "Paciente Once");

        let mut legacy_row = sample_row("d1", &patient_id);
        legacy_row.key_wrap_version = 1;
        let legacy = insert_document(&conn, &legacy_row).unwrap();
        assert_eq!(legacy.key_wrap_version, 1);

        let mut v2_row = sample_row("d2", &patient_id);
        v2_row.key_wrap_version = 2;
        let v2 = insert_document(&conn, &v2_row).unwrap();
        assert_eq!(v2.key_wrap_version, 2);

        // Releer desde cero confirma que el valor persistido en disco (no solo el devuelto por
        // insert_document) es el correcto para cada fila.
        assert_eq!(find_document_by_id(&conn, "d1").unwrap().unwrap().key_wrap_version, 1);
        assert_eq!(find_document_by_id(&conn, "d2").unwrap().unwrap().key_wrap_version, 2);
    }

    #[test]
    fn document_can_associate_to_episode_and_session_of_the_same_patient() {
        let conn = test_conn("episode-session");
        let patient_id = create_test_patient(&conn, "Paciente Dos");
        let episode_id = create_test_episode(&conn, &patient_id);
        let session_id = create_test_session(&conn, &patient_id, Some(&episode_id));

        let mut row = sample_row("d1", &patient_id);
        row.episode_id = Some(&episode_id);
        row.session_id = Some(&session_id);
        let doc = insert_document(&conn, &row).unwrap();
        assert_eq!(doc.episode_id.as_deref(), Some(episode_id.as_str()));
        assert_eq!(doc.session_id.as_deref(), Some(session_id.as_str()));
    }

    #[test]
    fn document_can_exist_without_episode_or_session_longitudinal_to_the_patient() {
        let conn = test_conn("longitudinal");
        let patient_id = create_test_patient(&conn, "Paciente Tres");
        let doc = insert_document(&conn, &sample_row("d1", &patient_id)).unwrap();
        assert!(doc.episode_id.is_none());
        assert!(doc.session_id.is_none());
    }

    #[test]
    fn multiple_documents_are_allowed_for_the_same_patient() {
        let conn = test_conn("multiple");
        let patient_id = create_test_patient(&conn, "Paciente Cuatro");
        insert_document(&conn, &sample_row("d1", &patient_id)).unwrap();
        insert_document(&conn, &sample_row("d2", &patient_id)).unwrap();
        assert_eq!(list_documents_by_patient(&conn, &patient_id).unwrap().len(), 2);
    }

    #[test]
    fn list_documents_by_patient_excludes_archived_and_never_carries_crypto_fields() {
        let conn = test_conn("list-excludes-archived");
        let patient_id = create_test_patient(&conn, "Paciente Cinco");
        insert_document(&conn, &sample_row("d1", &patient_id)).unwrap();
        insert_document(&conn, &sample_row("d2", &patient_id)).unwrap();
        archive_document(&conn, "d2").unwrap();

        let active = list_documents_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "d1");
    }

    #[test]
    fn list_archived_documents_by_patient_returns_only_archived() {
        let conn = test_conn("list-archived-only");
        let patient_id = create_test_patient(&conn, "Paciente Seis");
        insert_document(&conn, &sample_row("d1", &patient_id)).unwrap();
        insert_document(&conn, &sample_row("d2", &patient_id)).unwrap();
        archive_document(&conn, "d2").unwrap();

        let archived = list_archived_documents_by_patient(&conn, &patient_id).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, "d2");
    }

    #[test]
    fn update_metadata_edits_category_and_description_never_content() {
        let conn = test_conn("update-metadata");
        let patient_id = create_test_patient(&conn, "Paciente Siete");
        insert_document(&conn, &sample_row("d1", &patient_id)).unwrap();

        let updated = update_document_metadata(&conn, "d1", &DocumentMetadataUpdate { category: Some("derivacion"), description: Some("Nota") })
            .unwrap()
            .unwrap();
        assert_eq!(updated.category.as_deref(), Some("derivacion"));
        assert_eq!(updated.description.as_deref(), Some("Nota"));
        assert_eq!(updated.storage_path, "files/ab/d1.enc", "el contenido/ruta nunca cambia al editar metadata");
    }

    #[test]
    fn update_metadata_on_an_archived_document_is_rejected_at_repository_level() {
        let conn = test_conn("update-archived");
        let patient_id = create_test_patient(&conn, "Paciente Ocho");
        insert_document(&conn, &sample_row("d1", &patient_id)).unwrap();
        archive_document(&conn, "d1").unwrap();

        let result = update_document_metadata(&conn, "d1", &DocumentMetadataUpdate { category: Some("otro"), description: None }).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn archive_and_restore_round_trip() {
        let conn = test_conn("archive-restore");
        let patient_id = create_test_patient(&conn, "Paciente Nueve");
        insert_document(&conn, &sample_row("d1", &patient_id)).unwrap();

        assert!(archive_document(&conn, "d1").unwrap());
        assert!(find_document_by_id(&conn, "d1").unwrap().unwrap().deleted_at.is_some());
        assert!(!archive_document(&conn, "d1").unwrap(), "archivar dos veces no afecta ninguna fila la segunda vez");

        assert!(restore_document(&conn, "d1").unwrap());
        assert!(find_document_by_id(&conn, "d1").unwrap().unwrap().deleted_at.is_none());
    }

    #[test]
    fn list_all_storage_paths_includes_archived_documents_ciphertext_is_never_deleted() {
        let conn = test_conn("all-storage-paths");
        let patient_id = create_test_patient(&conn, "Paciente Diez");
        insert_document(&conn, &sample_row("d1", &patient_id)).unwrap();
        archive_document(&conn, "d1").unwrap();
        let paths = list_all_storage_paths(&conn).unwrap();
        assert_eq!(paths, vec!["files/ab/d1.enc".to_string()]);
    }
}
