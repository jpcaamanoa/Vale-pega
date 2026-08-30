//! Esquema completo de la base de datos (Fase 1.3).
//!
//! Reglas de diseño seguidas aquí (ver `docs/ARCHITECTURE.md` sección 4 y
//! `docs/db-schema.md` para el detalle completo de las decisiones tomadas en
//! esta fase):
//!
//! - Cada migración se ejecuta dentro de su propia transacción
//!   (`rusqlite_migration` lo hace automáticamente). Por eso **no** hay
//!   ninguna `PRAGMA` dentro del SQL de una migración — `PRAGMA foreign_keys`
//!   en particular sería un no-op ahí, tal como advierte la documentación de
//!   la propia librería. Esa pragma se controla desde `connection.rs`, una
//!   vez por conexión.
//! - Cada tabla "real" (no puente, no versión inmutable) tiene
//!   `created_at`/`updated_at`, y las que tienen `updated_at` reciben un
//!   trigger que lo actualiza automáticamente en cada `UPDATE` — SQLite no
//!   tiene un equivalente nativo a `ON UPDATE CURRENT_TIMESTAMP`.
//! - `.foreign_key_check()` en la migración hace que SQLite verifique con
//!   `PRAGMA foreign_key_check` que no quedó ninguna fila con una referencia
//!   rota antes de dar la migración por buena.
//! - Todo esto se compila a una única migración `V1` porque es la primera:
//!   no hay una versión anterior con datos que preservar. Las fases
//!   siguientes que agreguen columnas o tablas nuevas lo harán como `V2`,
//!   `V3`, etc., nunca modificando este texto una vez publicado (ver test
//!   `applying_a_new_migration_preserves_existing_data`).

use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

const SCHEMA_V1: &str = r#"
-- =========================================================================
-- Pacientes
-- =========================================================================
CREATE TABLE patients (
  id TEXT PRIMARY KEY,
  full_name TEXT NOT NULL,
  preferred_name TEXT,
  rut TEXT,
  birth_date TEXT,
  phone TEXT,
  email TEXT,
  address TEXT,
  emergency_contact_name TEXT,
  emergency_contact_phone TEXT,
  emergency_contact_relationship TEXT,
  status TEXT NOT NULL DEFAULT 'activo'
    CHECK (status IN ('activo','inactivo','alta','archivado')),
  referred_by TEXT,
  intake_date TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_patients_status ON patients(status) WHERE deleted_at IS NULL;
CREATE TRIGGER trg_patients_touch_updated_at
AFTER UPDATE ON patients
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE patients SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

CREATE TABLE patient_clinical_profile (
  patient_id TEXT PRIMARY KEY REFERENCES patients(id) ON DELETE RESTRICT,
  presenting_problem TEXT,
  primary_diagnosis_code TEXT,
  diagnosis_notes TEXT,
  risk_flags TEXT,
  relevant_medical_notes TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TRIGGER trg_patient_clinical_profile_touch_updated_at
AFTER UPDATE ON patient_clinical_profile
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE patient_clinical_profile SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
    WHERE patient_id = NEW.patient_id;
END;

-- =========================================================================
-- Agenda
-- =========================================================================
CREATE TABLE appointments (
  id TEXT PRIMARY KEY,
  patient_id TEXT REFERENCES patients(id) ON DELETE SET NULL,
  title TEXT NOT NULL,
  starts_at TEXT NOT NULL,
  ends_at TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'programada'
    CHECK (status IN ('programada','confirmada','cancelada','completada','no_asistio')),
  modality TEXT,
  google_event_id TEXT,
  google_calendar_id TEXT,
  last_synced_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT,
  CHECK (ends_at > starts_at)
);
CREATE UNIQUE INDEX idx_appointments_google_event
  ON appointments(google_event_id) WHERE google_event_id IS NOT NULL;
CREATE INDEX idx_appointments_starts_at ON appointments(starts_at) WHERE deleted_at IS NULL;
CREATE TRIGGER trg_appointments_touch_updated_at
AFTER UPDATE ON appointments
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE appointments SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

-- =========================================================================
-- Sesiones y notas (con versionado: Borrador -> Cerrada)
-- =========================================================================
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  appointment_id TEXT REFERENCES appointments(id) ON DELETE SET NULL,
  session_date TEXT NOT NULL,
  start_time TEXT,
  duration_minutes INTEGER,
  modality TEXT CHECK (modality IN ('presencial','online','telefonico')),
  status TEXT NOT NULL DEFAULT 'programada'
    CHECK (status IN ('programada','realizada','cancelada','no_asistio')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT,
  CHECK (duration_minutes IS NULL OR duration_minutes > 0)
);
CREATE INDEX idx_sessions_patient_date ON sessions(patient_id, session_date);
CREATE TRIGGER trg_sessions_touch_updated_at
AFTER UPDATE ON sessions
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE sessions SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

CREATE TABLE session_notes (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE RESTRICT,
  content TEXT,
  interventions TEXT,
  homework_tasks TEXT,
  next_focus TEXT,
  version INTEGER NOT NULL DEFAULT 1 CHECK (version >= 1),
  is_locked INTEGER NOT NULL DEFAULT 0 CHECK (is_locked IN (0,1)),
  is_current INTEGER NOT NULL DEFAULT 1 CHECK (is_current IN (0,1)),
  closed_at TEXT,
  superseded_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  -- Un borrador nunca tiene closed_at; una nota cerrada siempre lo tiene.
  CHECK ((is_locked = 0 AND closed_at IS NULL) OR (is_locked = 1 AND closed_at IS NOT NULL)),
  -- La versión vigente nunca tiene superseded_at; una versión reemplazada siempre lo tiene.
  CHECK ((is_current = 1 AND superseded_at IS NULL) OR (is_current = 0 AND superseded_at IS NOT NULL))
);
CREATE INDEX idx_session_notes_session ON session_notes(session_id);
-- Garantiza a nivel de base de datos que solo existe una versión vigente por sesión.
CREATE UNIQUE INDEX idx_session_notes_current ON session_notes(session_id) WHERE is_current = 1;
CREATE TRIGGER trg_session_notes_touch_updated_at
AFTER UPDATE ON session_notes
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE session_notes SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

-- =========================================================================
-- Documentos
-- =========================================================================
CREATE TABLE documents (
  id TEXT PRIMARY KEY,
  patient_id TEXT REFERENCES patients(id) ON DELETE SET NULL,
  session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  category TEXT CHECK (category IN
    ('informe','consentimiento','evaluacion_adjunta','receta','correspondencia','otro')),
  original_filename TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
  sha256_plaintext TEXT NOT NULL CHECK (length(sha256_plaintext) = 64),
  storage_path TEXT NOT NULL UNIQUE,
  is_clinical INTEGER NOT NULL DEFAULT 1 CHECK (is_clinical IN (0,1)),
  description TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_documents_patient ON documents(patient_id);
CREATE TRIGGER trg_documents_touch_updated_at
AFTER UPDATE ON documents
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE documents SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

-- =========================================================================
-- Formulación clínica (versionada; nodos y conexiones para React Flow)
-- =========================================================================
CREATE TABLE case_formulations (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  title TEXT NOT NULL,
  model_type TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE TRIGGER trg_case_formulations_touch_updated_at
AFTER UPDATE ON case_formulations
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE case_formulations SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

CREATE TABLE formulation_versions (
  id TEXT PRIMARY KEY,
  formulation_id TEXT NOT NULL REFERENCES case_formulations(id) ON DELETE CASCADE,
  version_number INTEGER NOT NULL CHECK (version_number >= 1),
  summary_text TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (formulation_id, version_number)
);

CREATE TABLE formulation_nodes (
  id TEXT PRIMARY KEY,
  formulation_version_id TEXT NOT NULL REFERENCES formulation_versions(id) ON DELETE CASCADE,
  node_type TEXT NOT NULL,
  label TEXT NOT NULL,
  description TEXT,
  position_x REAL NOT NULL,
  position_y REAL NOT NULL
);
CREATE INDEX idx_formulation_nodes_version ON formulation_nodes(formulation_version_id);

CREATE TABLE formulation_edges (
  id TEXT PRIMARY KEY,
  formulation_version_id TEXT NOT NULL REFERENCES formulation_versions(id) ON DELETE CASCADE,
  source_node_id TEXT NOT NULL REFERENCES formulation_nodes(id) ON DELETE CASCADE,
  target_node_id TEXT NOT NULL REFERENCES formulation_nodes(id) ON DELETE CASCADE,
  relation_label TEXT,
  CHECK (source_node_id <> target_node_id)
);
CREATE INDEX idx_formulation_edges_version ON formulation_edges(formulation_version_id);

-- Un edge solo puede conectar nodos que pertenezcan a SU MISMA versión de
-- formulación. El FK a formulation_nodes(id) por sí solo no lo garantiza
-- (un nodo de otra versión también tiene un id válido), así que se refuerza
-- con un trigger explícito en vez de confiar en que la capa de aplicación lo
-- recuerde siempre.
CREATE TRIGGER trg_formulation_edges_same_version_insert
BEFORE INSERT ON formulation_edges
BEGIN
  SELECT RAISE(ABORT, 'formulation_edges: los nodos deben pertenecer a la misma formulation_version del edge')
  WHERE
    (SELECT formulation_version_id FROM formulation_nodes WHERE id = NEW.source_node_id)
      IS NOT NEW.formulation_version_id
    OR
    (SELECT formulation_version_id FROM formulation_nodes WHERE id = NEW.target_node_id)
      IS NOT NEW.formulation_version_id;
END;
CREATE TRIGGER trg_formulation_edges_same_version_update
BEFORE UPDATE ON formulation_edges
BEGIN
  SELECT RAISE(ABORT, 'formulation_edges: los nodos deben pertenecer a la misma formulation_version del edge')
  WHERE
    (SELECT formulation_version_id FROM formulation_nodes WHERE id = NEW.source_node_id)
      IS NOT NEW.formulation_version_id
    OR
    (SELECT formulation_version_id FROM formulation_nodes WHERE id = NEW.target_node_id)
      IS NOT NEW.formulation_version_id;
END;

-- =========================================================================
-- Herramientas clínicas (catálogo propio; se crea antes de objetivos y
-- materiales porque ambos lo referencian)
-- =========================================================================
CREATE TABLE technique_categories (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  parent_category_id TEXT REFERENCES technique_categories(id) ON DELETE SET NULL
);

CREATE TABLE clinical_techniques (
  id TEXT PRIMARY KEY,
  category_id TEXT REFERENCES technique_categories(id) ON DELETE SET NULL,
  name TEXT NOT NULL,
  description TEXT,
  when_to_use TEXT,
  contraindications TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE TRIGGER trg_clinical_techniques_touch_updated_at
AFTER UPDATE ON clinical_techniques
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE clinical_techniques SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

CREATE TABLE technique_materials (
  id TEXT PRIMARY KEY,
  technique_id TEXT NOT NULL REFERENCES clinical_techniques(id) ON DELETE CASCADE,
  document_id TEXT REFERENCES documents(id) ON DELETE SET NULL,
  title TEXT NOT NULL,
  notes TEXT
);
CREATE INDEX idx_technique_materials_technique ON technique_materials(technique_id);

-- =========================================================================
-- Objetivos terapéuticos
-- =========================================================================
CREATE TABLE therapeutic_goals (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  formulation_id TEXT REFERENCES case_formulations(id) ON DELETE SET NULL,
  title TEXT NOT NULL,
  description TEXT,
  status TEXT NOT NULL DEFAULT 'activo'
    CHECK (status IN ('activo','logrado','pausado','descartado')),
  target_date TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_therapeutic_goals_patient ON therapeutic_goals(patient_id);
CREATE TRIGGER trg_therapeutic_goals_touch_updated_at
AFTER UPDATE ON therapeutic_goals
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE therapeutic_goals SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

CREATE TABLE goal_indicators (
  id TEXT PRIMARY KEY,
  goal_id TEXT NOT NULL REFERENCES therapeutic_goals(id) ON DELETE CASCADE,
  description TEXT NOT NULL,
  baseline_value TEXT,
  target_value TEXT
);
CREATE INDEX idx_goal_indicators_goal ON goal_indicators(goal_id);

CREATE TABLE goal_interventions (
  id TEXT PRIMARY KEY,
  goal_id TEXT NOT NULL REFERENCES therapeutic_goals(id) ON DELETE CASCADE,
  technique_id TEXT REFERENCES clinical_techniques(id) ON DELETE SET NULL,
  description TEXT NOT NULL
);
CREATE INDEX idx_goal_interventions_goal ON goal_interventions(goal_id);

-- Tabla puente N:M sesión <-> objetivo
CREATE TABLE session_goals (
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  goal_id TEXT NOT NULL REFERENCES therapeutic_goals(id) ON DELETE CASCADE,
  progress_note TEXT,
  PRIMARY KEY (session_id, goal_id)
);
CREATE INDEX idx_session_goals_goal ON session_goals(goal_id);

-- =========================================================================
-- Evaluaciones psicológicas
-- =========================================================================
CREATE TABLE assessment_instruments (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  is_custom INTEGER NOT NULL DEFAULT 0 CHECK (is_custom IN (0,1))
);

CREATE TABLE assessment_administrations (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  instrument_id TEXT NOT NULL REFERENCES assessment_instruments(id) ON DELETE RESTRICT,
  administered_at TEXT NOT NULL,
  context TEXT CHECK (context IN ('ingreso','seguimiento','alta')),
  raw_responses TEXT,
  total_score REAL,
  subscale_scores TEXT,
  interpretation_text TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_assessments_patient_instrument
  ON assessment_administrations(patient_id, instrument_id, administered_at);
CREATE TRIGGER trg_assessment_administrations_touch_updated_at
AFTER UPDATE ON assessment_administrations
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE assessment_administrations SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

-- =========================================================================
-- Pagos
-- =========================================================================
CREATE TABLE payments (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  amount REAL NOT NULL CHECK (amount >= 0),
  currency TEXT NOT NULL DEFAULT 'CLP',
  method TEXT CHECK (method IN ('efectivo','transferencia','tarjeta','otro')),
  status TEXT NOT NULL DEFAULT 'pendiente'
    CHECK (status IN ('pendiente','pagado','atrasado','condonado')),
  due_date TEXT,
  paid_at TEXT,
  notes TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_payments_status_due ON payments(status, due_date);
CREATE TRIGGER trg_payments_touch_updated_at
AFTER UPDATE ON payments
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE payments SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

-- =========================================================================
-- Biblioteca profesional
-- =========================================================================
CREATE TABLE library_resources (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  resource_type TEXT CHECK (resource_type IN
    ('articulo','libro','protocolo','escala','video','enlace')),
  author TEXT,
  source_url TEXT,
  file_document_id TEXT REFERENCES documents(id) ON DELETE SET NULL,
  summary TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE TRIGGER trg_library_resources_touch_updated_at
AFTER UPDATE ON library_resources
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE library_resources SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

CREATE TABLE library_tags (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
);

CREATE TABLE library_resource_tags (
  resource_id TEXT NOT NULL REFERENCES library_resources(id) ON DELETE CASCADE,
  tag_id TEXT NOT NULL REFERENCES library_tags(id) ON DELETE CASCADE,
  PRIMARY KEY (resource_id, tag_id)
);
CREATE INDEX idx_library_resource_tags_tag ON library_resource_tags(tag_id);

-- =========================================================================
-- Recordatorios y pendientes
-- =========================================================================
CREATE TABLE reminders (
  id TEXT PRIMARY KEY,
  patient_id TEXT REFERENCES patients(id) ON DELETE SET NULL,
  related_entity_type TEXT
    CHECK (related_entity_type IS NULL OR related_entity_type IN
      ('session','payment','document','goal','assessment')),
  related_entity_id TEXT,
  title TEXT NOT NULL,
  description TEXT,
  due_at TEXT,
  status TEXT NOT NULL DEFAULT 'pendiente'
    CHECK (status IN ('pendiente','completado','descartado')),
  priority TEXT NOT NULL DEFAULT 'media' CHECK (priority IN ('baja','media','alta')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  completed_at TEXT
);
CREATE INDEX idx_reminders_due_status ON reminders(due_at, status);

-- =========================================================================
-- Configuración de la aplicación
-- =========================================================================
CREATE TABLE app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TRIGGER trg_app_settings_touch_updated_at
AFTER UPDATE ON app_settings
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE app_settings SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE key = NEW.key;
END;
"#;

/// Todas las migraciones de la aplicación, en orden. Nunca se edita una
/// migración ya publicada — los cambios de esquema futuros se agregan como
/// una nueva entrada al final de este `vec!`.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(SCHEMA_V1).foreign_key_check()])
}

/// Lleva `conn` al esquema más reciente, creándolo desde cero si es una base
/// nueva. Sigue el patrón documentado por `rusqlite_migration`: las
/// migraciones corren con `foreign_keys` desactivado (cada migración es su
/// propia transacción, así que la pragma sería un no-op si se dejara dentro
/// del SQL) y se reactiva inmediatamente después, verificado por
/// `.foreign_key_check()`.
pub fn run_migrations(conn: &mut Connection) -> rusqlite_migration::Result<()> {
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let result = migrations().to_latest(conn);
    conn.pragma_update(None, "foreign_keys", "ON")?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::{key, temp_db_path};
    use crate::db::{open_vault, VaultError, VaultKey};
    use rusqlite::{params, Error as SqliteError};
    use std::fs;

    const EXPECTED_TABLES: &[&str] = &[
        "patients",
        "patient_clinical_profile",
        "appointments",
        "sessions",
        "session_notes",
        "documents",
        "case_formulations",
        "formulation_versions",
        "formulation_nodes",
        "formulation_edges",
        "technique_categories",
        "clinical_techniques",
        "technique_materials",
        "therapeutic_goals",
        "goal_indicators",
        "goal_interventions",
        "session_goals",
        "assessment_instruments",
        "assessment_administrations",
        "payments",
        "library_resources",
        "library_tags",
        "library_resource_tags",
        "reminders",
        "app_settings",
    ];

    fn migrated_vault(name: &str) -> (rusqlite::Connection, std::path::PathBuf, VaultKey) {
        let path = temp_db_path(name);
        let k = key(0x99);
        let mut conn = open_vault(&path, &k).expect("debería abrir/crear el vault");
        run_migrations(&mut conn).expect("las migraciones deberían aplicarse sin error");
        (conn, path, k)
    }

    // ---------------------------------------------------------------
    // 1-2: crear una base nueva únicamente vía migraciones, y verificar
    // que todas las tablas esperadas existen.
    // ---------------------------------------------------------------
    #[test]
    fn fresh_database_is_created_from_migrations_alone_with_all_expected_tables() {
        let (conn, _path, _key) = migrated_vault("fresh-db-all-tables");

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
            .unwrap();
        let mut existing: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        existing.sort();

        let mut expected: Vec<String> = EXPECTED_TABLES.iter().map(|s| s.to_string()).collect();
        expected.sort();

        assert_eq!(existing, expected, "el conjunto de tablas creadas por las migraciones no coincide con el esperado");
    }

    #[test]
    fn running_migrations_twice_on_the_same_database_is_a_safe_no_op() {
        let (mut conn, _path, _key) = migrated_vault("idempotent-migrations");
        conn.execute(
            "INSERT INTO patients (id, full_name) VALUES ('p1', 'Paciente Idempotencia')",
            [],
        )
        .unwrap();

        // Volver a aplicar migraciones (como ocurre en cada arranque de la
        // app) no debe recrear tablas ni perder filas ya insertadas.
        run_migrations(&mut conn).expect("reaplicar migraciones ya vigentes no debería fallar");

        let name: String = conn
            .query_row("SELECT full_name FROM patients WHERE id = 'p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "Paciente Idempotencia");
    }

    // ---------------------------------------------------------------
    // 3: foreign keys realmente activas.
    // ---------------------------------------------------------------
    #[test]
    fn foreign_keys_are_enforced_after_migration() {
        let (conn, _path, _key) = migrated_vault("fk-enforced");

        let err = conn
            .execute(
                "INSERT INTO sessions (id, patient_id, session_date) VALUES ('s1', 'no-existe', '2026-01-01')",
                [],
            )
            .expect_err("insertar una sesión con patient_id inexistente debe violar la foreign key");

        match err {
            SqliteError::SqliteFailure(e, _) => {
                assert_eq!(e.code, rusqlite::ErrorCode::ConstraintViolation);
            }
            other => panic!("se esperaba una violación de constraint, se obtuvo: {other:?}"),
        }
    }

    #[test]
    fn deleting_a_patient_with_sessions_is_restricted() {
        let (conn, _path, _key) = migrated_vault("fk-restrict-patient");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, patient_id, session_date) VALUES ('s1', 'p1', '2026-01-01')",
            [],
        )
        .unwrap();

        let err = conn
            .execute("DELETE FROM patients WHERE id = 'p1'", [])
            .expect_err("no debería poder borrarse físicamente un paciente con sesiones asociadas");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    // ---------------------------------------------------------------
    // 4: índices y restricciones importantes existen.
    // ---------------------------------------------------------------
    #[test]
    fn important_indexes_exist() {
        let (conn, _path, _key) = migrated_vault("indexes-exist");
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index'")
            .unwrap();
        let indexes: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        for expected in [
            "idx_patients_status",
            "idx_appointments_google_event",
            "idx_session_notes_current",
            "idx_sessions_patient_date",
            "idx_documents_patient",
            "idx_payments_status_due",
        ] {
            assert!(indexes.contains(&expected.to_string()), "falta el índice {expected}");
        }
    }

    #[test]
    fn only_one_current_session_note_version_is_allowed_per_session() {
        let (conn, _path, _key) = migrated_vault("one-current-note");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, patient_id, session_date) VALUES ('s1', 'p1', '2026-01-01')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_notes (id, session_id, version, is_current) VALUES ('n1', 's1', 1, 1)",
            [],
        )
        .unwrap();

        let err = conn
            .execute(
                "INSERT INTO session_notes (id, session_id, version, is_current) VALUES ('n2', 's1', 2, 1)",
                [],
            )
            .expect_err("no debería permitirse una segunda versión vigente para la misma sesión");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    // ---------------------------------------------------------------
    // 5: datos relacionados reales de prueba, a través de todos los
    // dominios, comprobando que las relaciones funcionan de punta a punta.
    // ---------------------------------------------------------------
    #[test]
    fn realistic_related_data_can_be_inserted_and_queried_across_all_domains() {
        let (conn, _path, _key) = migrated_vault("full-domain-chain");

        conn.execute(
            "INSERT INTO patients (id, full_name, status) VALUES ('pat-1', 'Paciente de Prueba', 'activo')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO patient_clinical_profile (patient_id, presenting_problem) VALUES ('pat-1', 'Ansiedad')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO appointments (id, patient_id, title, starts_at, ends_at)
             VALUES ('appt-1', 'pat-1', 'Sesión clínica', '2026-01-10T15:00:00Z', '2026-01-10T16:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO sessions (id, patient_id, appointment_id, session_date, status)
             VALUES ('sess-1', 'pat-1', 'appt-1', '2026-01-10', 'realizada')",
            [],
        )
        .unwrap();

        // Nota en borrador, luego cerrada.
        conn.execute(
            "INSERT INTO session_notes (id, session_id, content, version, is_locked, is_current)
             VALUES ('note-1', 'sess-1', 'Contenido inicial', 1, 0, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE session_notes SET is_locked = 1, closed_at = '2026-01-10T16:05:00Z' WHERE id = 'note-1'",
            [],
        )
        .unwrap();
        // Nueva versión al modificar la nota cerrada: la anterior deja de ser vigente.
        conn.execute(
            "UPDATE session_notes SET is_current = 0, superseded_at = '2026-01-11T09:00:00Z' WHERE id = 'note-1'",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_notes (id, session_id, content, version, is_locked, is_current)
             VALUES ('note-2', 'sess-1', 'Contenido revisado', 2, 0, 1)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO case_formulations (id, patient_id, title) VALUES ('form-1', 'pat-1', 'Formulación inicial')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_versions (id, formulation_id, version_number) VALUES ('fver-1', 'form-1', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_nodes (id, formulation_version_id, node_type, label, position_x, position_y)
             VALUES ('node-1', 'fver-1', 'problema', 'Evitación', 0.0, 0.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_nodes (id, formulation_version_id, node_type, label, position_x, position_y)
             VALUES ('node-2', 'fver-1', 'factor_mantenedor', 'Rumiación', 200.0, 0.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_edges (id, formulation_version_id, source_node_id, target_node_id, relation_label)
             VALUES ('edge-1', 'fver-1', 'node-1', 'node-2', 'mantiene')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO technique_categories (id, name) VALUES ('cat-1', 'Terapia cognitivo-conductual')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO clinical_techniques (id, category_id, name) VALUES ('tech-1', 'cat-1', 'Reestructuración cognitiva')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO therapeutic_goals (id, patient_id, formulation_id, title) VALUES ('goal-1', 'pat-1', 'form-1', 'Reducir evitación')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO goal_indicators (id, goal_id, description) VALUES ('ind-1', 'goal-1', 'Frecuencia semanal de exposición')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO goal_interventions (id, goal_id, technique_id, description) VALUES ('gint-1', 'goal-1', 'tech-1', 'Exposición gradual')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_goals (session_id, goal_id, progress_note) VALUES ('sess-1', 'goal-1', 'Buen avance')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO assessment_instruments (id, name) VALUES ('instr-1', 'BDI-II')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assessment_administrations (id, patient_id, instrument_id, administered_at, total_score)
             VALUES ('admin-1', 'pat-1', 'instr-1', '2026-01-05', 18.0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO documents (id, patient_id, session_id, original_filename, mime_type, size_bytes, sha256_plaintext, storage_path)
             VALUES ('doc-1', 'pat-1', 'sess-1', 'informe.pdf', 'application/pdf', 1024, ?1, '/vault/doc-1.enc')",
            params!["a".repeat(64)],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO payments (id, patient_id, session_id, amount, status) VALUES ('pay-1', 'pat-1', 'sess-1', 30000.0, 'pagado')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO library_resources (id, title, file_document_id) VALUES ('lib-1', 'Protocolo de exposición', 'doc-1')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO library_tags (id, name) VALUES ('tag-1', 'ansiedad')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO library_resource_tags (resource_id, tag_id) VALUES ('lib-1', 'tag-1')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO reminders (id, patient_id, related_entity_type, related_entity_id, title)
             VALUES ('rem-1', 'pat-1', 'session', 'sess-1', 'Revisar tarea de exposición')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('theme', '\"light\"')",
            [],
        )
        .unwrap();

        // Verificaciones de extremo a extremo a través de las relaciones.
        let note_count: i64 = conn
            .query_row("SELECT count(*) FROM session_notes WHERE session_id = 'sess-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(note_count, 2, "deben existir ambas versiones de la nota");

        let current_note_content: String = conn
            .query_row(
                "SELECT content FROM session_notes WHERE session_id = 'sess-1' AND is_current = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(current_note_content, "Contenido revisado");

        let goal_title_via_session: String = conn
            .query_row(
                "SELECT tg.title FROM session_goals sg
                 JOIN therapeutic_goals tg ON tg.id = sg.goal_id
                 WHERE sg.session_id = 'sess-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(goal_title_via_session, "Reducir evitación");

        let edge_nodes: (String, String) = conn
            .query_row(
                "SELECT n1.label, n2.label FROM formulation_edges e
                 JOIN formulation_nodes n1 ON n1.id = e.source_node_id
                 JOIN formulation_nodes n2 ON n2.id = e.target_node_id
                 WHERE e.id = 'edge-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(edge_nodes, ("Evitación".to_string(), "Rumiación".to_string()));

        let tag_for_resource: String = conn
            .query_row(
                "SELECT lt.name FROM library_resource_tags lrt
                 JOIN library_tags lt ON lt.id = lrt.tag_id
                 WHERE lrt.resource_id = 'lib-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tag_for_resource, "ansiedad");

        let payment_amount: f64 = conn
            .query_row("SELECT amount FROM payments WHERE session_id = 'sess-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(payment_amount, 30000.0);
    }

    // ---------------------------------------------------------------
    // 6: las restricciones impiden estados inválidos.
    // ---------------------------------------------------------------
    #[test]
    fn invalid_enum_value_is_rejected_by_check_constraint() {
        let (conn, _path, _key) = migrated_vault("invalid-enum");
        let err = conn
            .execute(
                "INSERT INTO patients (id, full_name, status) VALUES ('p1', 'X', 'no_es_un_estado_valido')",
                [],
            )
            .expect_err("un valor de status fuera del enum debe rechazarse");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn negative_payment_amount_is_rejected() {
        let (conn, _path, _key) = migrated_vault("negative-payment");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", [])
            .unwrap();
        let err = conn
            .execute(
                "INSERT INTO payments (id, patient_id, amount) VALUES ('pay-1', 'p1', -100.0)",
                [],
            )
            .expect_err("un pago con monto negativo debe rechazarse");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn appointment_ending_before_it_starts_is_rejected() {
        let (conn, _path, _key) = migrated_vault("bad-appointment-times");
        let err = conn
            .execute(
                "INSERT INTO appointments (id, title, starts_at, ends_at)
                 VALUES ('a1', 'Sesión clínica', '2026-01-10T16:00:00Z', '2026-01-10T15:00:00Z')",
                [],
            )
            .expect_err("una cita que termina antes de empezar debe rechazarse");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn closed_session_note_without_closed_at_is_rejected() {
        let (conn, _path, _key) = migrated_vault("closed-note-needs-closed-at");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, patient_id, session_date) VALUES ('s1', 'p1', '2026-01-01')",
            [],
        )
        .unwrap();
        let err = conn
            .execute(
                "INSERT INTO session_notes (id, session_id, is_locked, closed_at) VALUES ('n1', 's1', 1, NULL)",
                [],
            )
            .expect_err("una nota marcada como cerrada sin closed_at debe rechazarse");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn formulation_edge_across_different_versions_is_rejected() {
        let (conn, _path, _key) = migrated_vault("edge-cross-version");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO case_formulations (id, patient_id, title) VALUES ('f1', 'p1', 'F')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_versions (id, formulation_id, version_number) VALUES ('v1', 'f1', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_versions (id, formulation_id, version_number) VALUES ('v2', 'f1', 2)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_nodes (id, formulation_version_id, node_type, label, position_x, position_y)
             VALUES ('n1', 'v1', 'problema', 'A', 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_nodes (id, formulation_version_id, node_type, label, position_x, position_y)
             VALUES ('n2', 'v2', 'problema', 'B', 0, 0)",
            [],
        )
        .unwrap();

        let err = conn
            .execute(
                "INSERT INTO formulation_edges (id, formulation_version_id, source_node_id, target_node_id)
                 VALUES ('e1', 'v1', 'n1', 'n2')",
                [],
            )
            .expect_err("un edge no debe poder conectar nodos de versiones distintas");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn formulation_edge_self_loop_is_rejected() {
        let (conn, _path, _key) = migrated_vault("edge-self-loop");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO case_formulations (id, patient_id, title) VALUES ('f1', 'p1', 'F')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_versions (id, formulation_id, version_number) VALUES ('v1', 'f1', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO formulation_nodes (id, formulation_version_id, node_type, label, position_x, position_y)
             VALUES ('n1', 'v1', 'problema', 'A', 0, 0)",
            [],
        )
        .unwrap();

        let err = conn
            .execute(
                "INSERT INTO formulation_edges (id, formulation_version_id, source_node_id, target_node_id)
                 VALUES ('e1', 'v1', 'n1', 'n1')",
                [],
            )
            .expect_err("un nodo no debe poder conectarse consigo mismo");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn document_hash_with_wrong_length_is_rejected() {
        let (conn, _path, _key) = migrated_vault("bad-hash-length");
        let err = conn
            .execute(
                "INSERT INTO documents (id, original_filename, mime_type, size_bytes, sha256_plaintext, storage_path)
                 VALUES ('d1', 'x.pdf', 'application/pdf', 10, 'demasiado-corto', '/vault/d1.enc')",
                [],
            )
            .expect_err("un hash que no mide 64 caracteres debe rechazarse");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn updated_at_is_bumped_automatically_on_update() {
        let (conn, _path, _key) = migrated_vault("touch-updated-at");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", [])
            .unwrap();
        let before: String = conn
            .query_row("SELECT updated_at FROM patients WHERE id = 'p1'", [], |r| r.get(0))
            .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));
        conn.execute("UPDATE patients SET full_name = 'Y' WHERE id = 'p1'", [])
            .unwrap();

        let after: String = conn
            .query_row("SELECT updated_at FROM patients WHERE id = 'p1'", [], |r| r.get(0))
            .unwrap();
        assert!(after > before, "updated_at debería avanzar automáticamente tras un UPDATE");
    }

    // ---------------------------------------------------------------
    // 7: el esquema funciona sobre SQLCipher y no sobre SQLite plano.
    // ---------------------------------------------------------------
    #[test]
    fn schema_and_data_are_unreadable_as_plain_sqlite_on_disk() {
        let path = temp_db_path("schema-over-sqlcipher");
        let k = key(0xAA);
        {
            let mut conn = open_vault(&path, &k).unwrap();
            run_migrations(&mut conn).unwrap();
            conn.execute(
                "INSERT INTO patients (id, full_name) VALUES ('p1', 'Nombre Clinico Sensible')",
                [],
            )
            .unwrap();
        }

        let bytes = fs::read(&path).unwrap();
        const SQLITE_PLAINTEXT_HEADER: &[u8] = b"SQLite format 3\0";
        assert_ne!(&bytes[..SQLITE_PLAINTEXT_HEADER.len()], SQLITE_PLAINTEXT_HEADER);
        assert!(
            !bytes.windows(b"Nombre Clinico Sensible".len()).any(|w| w == b"Nombre Clinico Sensible"),
            "el nombre del paciente no debe aparecer en claro en el archivo"
        );
        // Ni siquiera el nombre de una tabla debe verse en el archivo cifrado.
        assert!(
            !bytes.windows(b"CREATE TABLE patients".len()).any(|w| w == b"CREATE TABLE patients"),
            "el esquema SQL tampoco debe ser legible en el archivo cifrado"
        );

        // Y, tal como en la Fase 1.2, la clave incorrecta debe ser rechazada
        // incluso ya con el esquema completo y datos reales cargados.
        let wrong_key_err = open_vault(&path, &key(0xBB))
            .expect_err("el esquema completo con datos reales tampoco debe abrirse con otra clave");
        assert!(matches!(wrong_key_err, VaultError::WrongKeyOrCorrupt));
    }

    // ---------------------------------------------------------------
    // 8a: reaplicar migraciones sobre el esquema real no destruye datos
    // (variante ya cubierta arriba con running_migrations_twice..., se repite
    // aquí cerrando y reabriendo la conexión para acercarse más a un reinicio
    // real de la aplicación).
    // ---------------------------------------------------------------
    #[test]
    fn reopening_and_remigrating_an_existing_vault_preserves_data() {
        let path = temp_db_path("reopen-remigrate");
        let k = key(0xCC);
        {
            let mut conn = open_vault(&path, &k).unwrap();
            run_migrations(&mut conn).unwrap();
            conn.execute(
                "INSERT INTO patients (id, full_name) VALUES ('p1', 'Paciente Persistente')",
                [],
            )
            .unwrap();
        }

        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).expect("re-migrar un vault ya existente no debería fallar");
        let name: String = conn
            .query_row("SELECT full_name FROM patients WHERE id = 'p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "Paciente Persistente");
    }

    // ---------------------------------------------------------------
    // 8b: el mecanismo de rusqlite_migration realmente aplica solo las
    // migraciones nuevas sin perder datos de las anteriores. Se usa un
    // esquema sintético de 2 versiones (no el esquema real de la app,
    // que hoy solo tiene V1) para probar el mecanismo en sí.
    // ---------------------------------------------------------------
    #[test]
    fn applying_a_new_migration_preserves_existing_data() {
        let path = temp_db_path("incremental-migration");
        let k = key(0xDD);

        let v1_only = Migrations::new(vec![M::up(
            "CREATE TABLE scratch (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
        )]);
        {
            let mut conn = open_vault(&path, &k).unwrap();
            conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
            v1_only.to_latest(&mut conn).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            conn.execute("INSERT INTO scratch (id, name) VALUES (1, 'dato preexistente')", [])
                .unwrap();
        }

        // "Fase futura": se agrega una migración V2 que solo añade una
        // columna nueva, sin tocar el texto de V1.
        let v1_and_v2 = Migrations::new(vec![
            M::up("CREATE TABLE scratch (id INTEGER PRIMARY KEY, name TEXT NOT NULL);"),
            M::up("ALTER TABLE scratch ADD COLUMN notes TEXT;"),
        ]);
        let mut conn = open_vault(&path, &k).unwrap();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        v1_and_v2.to_latest(&mut conn).expect("la migración incremental V2 debería aplicarse");
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();

        let name: String = conn
            .query_row("SELECT name FROM scratch WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "dato preexistente", "V2 no debe haber tocado los datos de V1");

        conn.execute("UPDATE scratch SET notes = 'agregado en V2' WHERE id = 1", [])
            .expect("la columna nueva de V2 debe existir y ser usable");
    }
}
