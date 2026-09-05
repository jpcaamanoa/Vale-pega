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

/// V2 (Fase 6.1): ubicación geográfica general del paciente. Puramente
/// aditiva — dos columnas nullable sobre `patients`, sin `DEFAULT`
/// obligatorio y sin backfill. Todo paciente creado bajo V1 queda con
/// `region = NULL, commune = NULL` tras aplicar esta migración, sin error y
/// sin perder ningún dato existente (ver
/// `region_and_commune_are_null_for_patients_created_before_v2` y
/// `v2_migration_preserves_all_existing_patient_data`).
///
/// Deliberadamente NO se agregan aquí: tablas `regions`/`communes`, FK
/// geográficas, ni un `CHECK` de comuna — la validación del catálogo
/// (región válida, comuna perteneciente a esa región) vive en
/// `services::patients`, no en el esquema. Ver `src-tauri/src/geo.rs` y
/// `docs/geographic-stats.md`.
const SCHEMA_V2: &str = r#"
ALTER TABLE patients ADD COLUMN region TEXT;
ALTER TABLE patients ADD COLUMN commune TEXT;
"#;

/// V3 (Fase 8): continuidad entre sesiones — dos tablas nuevas,
/// completamente aditivas, sin tocar ninguna tabla de V1/V2. Ver
/// `docs/session-continuity.md` para el diseño completo.
///
/// `session_notes.next_focus`/`homework_tasks` (V1) no se tocan ni se
/// reinterpretan: siguen siendo texto histórico dentro de una nota
/// versionada e inmutable una vez cerrada. Las dos tablas de aquí son
/// **operativas**, con su propio ciclo de vida, independientes de cualquier
/// nota concreta — ver la nota de diseño en `docs/session-continuity.md`
/// sobre por qué no bastaba con esos dos campos.
///
/// `patient_prep_notes`: "quiero acordarme de esto la próxima vez que vea a
/// este paciente". Deliberadamente **sin `deleted_at`** — a diferencia de
/// therapeutic_goals/payments/sessions, el ciclo de vida completo de esta
/// entidad ya queda representado por el propio `status`
/// (`pendiente`/`abordado`/`descartado`, ninguno de los tres oculta la fila
/// de ningún listado permanentemente): no hay ningún caso de uso de "ocultar
/// esta fila de todos lados sin perder el dato" distinto de simplemente
/// marcarla `descartado`, así que agregar soft-delete encima sería un
/// segundo mecanismo redundante para el mismo propósito. `origin_session_id`
/// es opcional a propósito (regla 7 de la aprobación de Fase 8): una
/// preparación nunca depende de que exista una cita futura agendada.
///
/// `therapy_tasks`: entidad con ciclo de vida propio, distinta de
/// `reminders` (que no se implementa en esta fase) — una tarea terapéutica
/// pertenece al proceso clínico, no es una alerta temporal genérica. Cinco
/// estados: `pendiente`/`parcial`/`realizada`/`no_realizada` (los cuatro
/// pedidos explícitamente) más `descartada` (agregado, justificado en
/// `docs/session-continuity.md`: cubre una tarea que deja de ser relevante
/// *antes* de llegar a revisarse en ninguna sesión — un caso distinto de
/// `no_realizada`, que sí implica que hubo una revisión con resultado
/// negativo). `goal_id` es opcional y usa `ON DELETE SET NULL` — igual
/// patrón que `payments.session_id`/`therapeutic_goals.formulation_id` — y
/// nunca se activa en la práctica porque los objetivos solo se archivan
/// (soft delete), nunca se borran físicamente: una tarea vinculada a un
/// objetivo archivado conserva el vínculo intacto. `deleted_at` sí existe
/// aquí (a diferencia de `patient_prep_notes`): archivar una tarea es un
/// acto administrativo distinto de cualquiera de sus cinco estados
/// clínicos, mismo criterio que separar `archived`/`status` en Objetivos y
/// Pagos.
const SCHEMA_V3: &str = r#"
CREATE TABLE patient_prep_notes (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  origin_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  content TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pendiente'
    CHECK (status IN ('pendiente','abordado','descartado')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_patient_prep_notes_patient_status ON patient_prep_notes(patient_id, status);
CREATE TRIGGER trg_patient_prep_notes_touch_updated_at
AFTER UPDATE ON patient_prep_notes
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE patient_prep_notes SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

CREATE TABLE therapy_tasks (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  assigned_in_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  goal_id TEXT REFERENCES therapeutic_goals(id) ON DELETE SET NULL,
  description TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pendiente'
    CHECK (status IN ('pendiente','parcial','realizada','no_realizada','descartada')),
  assigned_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  review_due_at TEXT,
  reviewed_in_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  reviewed_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_therapy_tasks_patient_status ON therapy_tasks(patient_id, status);
CREATE INDEX idx_therapy_tasks_goal ON therapy_tasks(goal_id);
CREATE TRIGGER trg_therapy_tasks_touch_updated_at
AFTER UPDATE ON therapy_tasks
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE therapy_tasks SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;
"#;

/// V4 (Fase 9): episodios/procesos terapéuticos mínimos — resuelve el
/// problema estructural "paciente ≠ proceso" identificado en la auditoría
/// post Fase 8. Ver `docs/treatment-episodes.md` para el diseño completo.
///
/// `treatment_episodes` es deliberadamente pequeña: solo `started_at` y
/// `status` (`activo`/`pausado`/`cerrado`). NO lleva `reason_for_end`,
/// `closure_summary`, `recommendations` ni ningún campo de cierre
/// estructurado — esos pertenecen a la futura Fase 10 (Cierre/Alta), que
/// vivirá en una tabla `episode_closures` separada, todavía sin crear. El
/// valor `'cerrado'` ya existe en el `CHECK` para que el modelo esté
/// preparado, pero la capa de servicio de esta fase (`services::
/// treatment_episodes::set_status`) deliberadamente NO permite alcanzarlo
/// desde la UI — solo la migración legacy de abajo lo escribe directamente,
/// para pacientes cuyo `patients.status` ya era `'alta'`.
///
/// Un solo proceso `'activo'` por paciente, reforzado en dos capas
/// independientes (mismo criterio de defensa en profundidad que
/// `idx_session_notes_current` en `SCHEMA_V1`): el servicio lo verifica
/// explícitamente antes de escribir, y el índice único parcial
/// `idx_treatment_episodes_one_active_per_patient` lo garantiza también a
/// nivel de base de datos como último recurso.
///
/// `episode_clinical_profile` es 1:1 con el episodio (mismo patrón que
/// `patient_clinical_profile` 1:1 con el paciente desde `SCHEMA_V1`, con
/// `episode_id` como su propia `PRIMARY KEY`). Contiene únicamente
/// `presenting_problem`/`primary_diagnosis_code`/`diagnosis_notes` — los
/// tres campos que la auditoría post Fase 8 clasificó como específicos de
/// proceso. `relevant_medical_notes` y `risk_flags` **no se copian aquí**:
/// permanecen exclusivamente en `patient_clinical_profile` como
/// longitudinales del paciente (política conservadora aprobada — `risk_flags`
/// en particular se trata como longitudinal mientras no exista una
/// taxonomía que distinga riesgo histórico de riesgo específico de proceso,
/// decisión clínica explícitamente diferida, no tomada por esta migración).
///
/// **`patient_clinical_profile` no se modifica de ninguna forma** — ni sus
/// columnas ni sus datos. La migración copia (nunca mueve, nunca borra)
/// `presenting_problem`/`primary_diagnosis_code`/`diagnosis_notes` hacia el
/// `episode_clinical_profile` del proceso legacy correspondiente, dejando el
/// original completamente intacto — preservación ante todo, sin arriesgar
/// nunca dejar un diagnóstico histórico en `NULL` por error de migración.
///
/// `sessions.episode_id`/`therapeutic_goals.episode_id` son columnas
/// puramente aditivas (`ALTER TABLE ADD COLUMN`, nullable, `ON DELETE SET
/// NULL`) — `patient_id` sigue siendo obligatorio en ambas tablas y esta
/// migración no lo toca. Una sesión o un objetivo pueden seguir existiendo
/// sin proceso (ej. una entrevista única previa a decidir iniciar un
/// proceso formal) — `episode_id NOT NULL` nunca se impone.
///
/// **Migración legacy** (backfill, automática y no destructiva): se crea
/// como máximo **un** proceso legacy por paciente (id determinístico
/// `'legacy-' || patient_id`, nunca un UUID aleatorio — a propósito, para
/// que el origen de la fila sea auditable a simple vista), y solo para
/// pacientes que tengan al menos una sesión, un objetivo terapéutico, o un
/// `patient_clinical_profile` ya registrado — un paciente cuya única
/// actividad sean preparaciones/tareas/pagos (que no reciben `episode_id`
/// en esta fase, ver más abajo) no recibe ningún proceso legacy, para no
/// crear "episodios basura" sin ningún dato real que agrupar. `started_at`
/// se deriva, en orden de preferencia: la fecha de la sesión más antigua del
/// paciente, luego `patients.intake_date`, luego la fecha (sin hora) de
/// `patients.created_at` — siempre existe al menos el último. El `status`
/// del proceso legacy se deriva del único dato ya existente que se le
/// parece: `'cerrado'` si `patients.status = 'alta'`, `'activo'` en
/// cualquier otro caso (`activo`/`inactivo`/`archivado`) — sin inventar
/// ninguna heurística de fechas ("si pasaron X meses…"), tal como exigía la
/// aprobación de esta fase. Todas las sesiones y objetivos existentes de un
/// paciente con proceso legacy quedan asociados a ese único proceso — nunca
/// se intenta reconstruir múltiples procesos antiguos a partir de fechas.
///
/// **Deliberadamente sin `episode_id` en esta fase**: `payments`,
/// `patient_prep_notes`, `therapy_tasks`, `documents`,
/// `assessment_administrations`, `case_formulations`, `reminders`,
/// `appointments` — ninguna de estas tablas se modifica. Ver
/// `docs/treatment-episodes.md` para la justificación completa de por qué
/// Fase 9 se mantiene deliberadamente pequeña.
const SCHEMA_V4: &str = r#"
CREATE TABLE treatment_episodes (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  started_at TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'activo'
    CHECK (status IN ('activo','pausado','cerrado')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_treatment_episodes_patient_status ON treatment_episodes(patient_id, status);
-- Defensa en profundidad (mismo criterio que idx_session_notes_current en
-- SCHEMA_V1): un solo proceso activo por paciente, garantizado también a
-- nivel de base de datos, no solo en el servicio.
CREATE UNIQUE INDEX idx_treatment_episodes_one_active_per_patient
  ON treatment_episodes(patient_id) WHERE status = 'activo' AND deleted_at IS NULL;
CREATE TRIGGER trg_treatment_episodes_touch_updated_at
AFTER UPDATE ON treatment_episodes
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE treatment_episodes SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;

CREATE TABLE episode_clinical_profile (
  episode_id TEXT PRIMARY KEY REFERENCES treatment_episodes(id) ON DELETE RESTRICT,
  presenting_problem TEXT,
  primary_diagnosis_code TEXT,
  diagnosis_notes TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TRIGGER trg_episode_clinical_profile_touch_updated_at
AFTER UPDATE ON episode_clinical_profile
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE episode_clinical_profile SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
    WHERE episode_id = NEW.episode_id;
END;

ALTER TABLE sessions ADD COLUMN episode_id TEXT REFERENCES treatment_episodes(id) ON DELETE SET NULL;
CREATE INDEX idx_sessions_episode ON sessions(episode_id);

ALTER TABLE therapeutic_goals ADD COLUMN episode_id TEXT REFERENCES treatment_episodes(id) ON DELETE SET NULL;
CREATE INDEX idx_therapeutic_goals_episode ON therapeutic_goals(episode_id);

-- Migración legacy: un proceso como máximo por paciente con actividad
-- clínica real ya registrada (sesiones, objetivos o antecedentes).
INSERT INTO treatment_episodes (id, patient_id, started_at, status)
SELECT
  'legacy-' || p.id,
  p.id,
  COALESCE(
    (SELECT MIN(s.session_date) FROM sessions s WHERE s.patient_id = p.id),
    p.intake_date,
    substr(p.created_at, 1, 10)
  ),
  CASE WHEN p.status = 'alta' THEN 'cerrado' ELSE 'activo' END
FROM patients p
WHERE
  EXISTS (SELECT 1 FROM sessions s WHERE s.patient_id = p.id)
  OR EXISTS (SELECT 1 FROM therapeutic_goals g WHERE g.patient_id = p.id)
  OR EXISTS (SELECT 1 FROM patient_clinical_profile cp WHERE cp.patient_id = p.id);

UPDATE sessions SET episode_id = 'legacy-' || patient_id;
UPDATE therapeutic_goals SET episode_id = 'legacy-' || patient_id;

INSERT INTO episode_clinical_profile (episode_id, presenting_problem, primary_diagnosis_code, diagnosis_notes)
SELECT 'legacy-' || cp.patient_id, cp.presenting_problem, cp.primary_diagnosis_code, cp.diagnosis_notes
FROM patient_clinical_profile cp;
"#;

/// V5 (Fase 11): cierre estructurado de un proceso terapéutico — la tabla
/// que el propio comentario de `SCHEMA_V4` ya anunciaba ("vivirá en una
/// tabla `episode_closures` separada, todavía sin crear"). Ver
/// `docs/episode-closure.md` para el diseño completo, resuelto en la
/// auditoría "Fase 11 — Cierre/Alta estructurado".
///
/// Tabla nueva, completamente aditiva — `SCHEMA_V1`–`V4` quedan intactos.
/// Ningún `ALTER TABLE` sobre `treatment_episodes`, `sessions`,
/// `therapeutic_goals`, `patients` ni ninguna tabla existente: el estado
/// operativo del proceso (`activo`/`pausado`/`cerrado`, ya en `SCHEMA_V4`)
/// se mantiene separado del evento clínico de cierre, que vive aquí.
///
/// `reason` y `outcome` son taxonomías cerradas e independientes entre sí
/// (una derivación puede coexistir con objetivos parcialmente logrados —
/// nunca se fuerza una combinación). `reason_detail` se usa libremente y es
/// obligatorio en la capa de servicio cuando `reason = 'otro'`, pero el
/// `CHECK` de esquema no lo exige (evita duplicar esa regla de negocio en
/// SQL).
///
/// **Inmutable tras crearse** (decisión explícita de la aprobación de Fase
/// 11: corregir un error de fondo usa anular + crear un cierre nuevo, nunca
/// editar el contenido de uno existente — a propósito distinto del patrón
/// mutable de `episode_clinical_profile`). La única escritura posterior
/// permitida es marcar `reverted_at`/`reverted_reason` (anulación
/// auditable): la fila original nunca se borra ni se sobrescribe, queda
/// como historia. `idx_episode_closures_active` (índice único parcial,
/// mismo patrón exacto que `idx_treatment_episodes_one_active_per_patient`
/// de `SCHEMA_V4`) garantiza a nivel de base de datos que nunca hay más de
/// un cierre vigente (no anulado) por proceso, sin impedir conservar
/// cierres anulados anteriores como historia completa.
const SCHEMA_V5: &str = r#"
CREATE TABLE episode_closures (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL REFERENCES treatment_episodes(id) ON DELETE RESTRICT,
  closed_at TEXT NOT NULL,
  reason TEXT NOT NULL CHECK (reason IN
    ('alta','cierre_acordado','interrupcion','derivacion','decision_profesional','otro')),
  reason_detail TEXT,
  outcome TEXT NOT NULL CHECK (outcome IN
    ('objetivos_logrados','parcialmente_logrados','no_logrados','no_evaluable')),
  summary TEXT,
  recommendations TEXT,
  reverted_at TEXT,
  reverted_reason TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  -- Una anulación siempre lleva motivo, y un cierre vigente nunca tiene
  -- ninguno de los dos campos de anulación a medias.
  CHECK ((reverted_at IS NULL AND reverted_reason IS NULL)
      OR (reverted_at IS NOT NULL AND reverted_reason IS NOT NULL))
);
CREATE INDEX idx_episode_closures_episode ON episode_closures(episode_id);
CREATE UNIQUE INDEX idx_episode_closures_active
  ON episode_closures(episode_id) WHERE reverted_at IS NULL;
CREATE TRIGGER trg_episode_closures_touch_updated_at
AFTER UPDATE ON episode_closures
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE episode_closures SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;
"#;

/// Todas las migraciones de la aplicación, en orden. Nunca se edita una
/// migración ya publicada — los cambios de esquema futuros se agregan como
/// una nueva entrada al final de este `vec!`.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(SCHEMA_V1).foreign_key_check(),
        M::up(SCHEMA_V2).foreign_key_check(),
        M::up(SCHEMA_V3).foreign_key_check(),
        M::up(SCHEMA_V4).foreign_key_check(),
        M::up(SCHEMA_V5).foreign_key_check(),
    ])
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
        "patient_prep_notes",
        "therapy_tasks",
        "treatment_episodes",
        "episode_clinical_profile",
        "episode_closures",
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
    // 2b (Fase 6.1): migración V2 — region/commune — es aditiva y no
    // destructiva sobre un vault V1 real con pacientes ya insertados.
    // ---------------------------------------------------------------
    #[test]
    fn v2_migration_preserves_all_existing_patient_data() {
        let path = temp_db_path("v2-preserves-data");
        let k = key(0xE1);

        // 1. Llevar un vault únicamente a V1 — simula el estado real de un
        //    vault creado antes de Fase 6.1, con pacientes ficticios ya
        //    cargados y todos sus campos completos.
        {
            let mut conn = open_vault(&path, &k).unwrap();
            conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
            Migrations::new(vec![M::up(SCHEMA_V1).foreign_key_check()]).to_latest(&mut conn).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();

            conn.execute(
                "INSERT INTO patients (id, full_name, preferred_name, rut, birth_date, phone, email, address, status)
                 VALUES ('p1', 'Paciente Ficticio Uno', 'Uno', '11111111-1', '1990-01-01', '+56900000001',
                         'uno@ejemplo.test', 'Calle Falsa 123', 'activo')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO patients (id, full_name, status) VALUES ('p2', 'Paciente Ficticio Dos', 'archivado')",
                [],
            )
            .unwrap();
        }

        // 2. Reabrir y aplicar el mecanismo normal de migraciones (V1+V2) —
        //    exactamente lo que hace la app en cada arranque real.
        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).expect("V2 debe aplicarse limpiamente sobre un vault V1 con datos reales");

        // 3. Ambos pacientes siguen existiendo con TODOS sus campos
        //    antiguos intactos, y las columnas nuevas quedan en NULL sin
        //    error ni valor inventado.
        let (name, rut, address, region, commune): (String, Option<String>, Option<String>, Option<String>, Option<String>) = conn
            .query_row("SELECT full_name, rut, address, region, commune FROM patients WHERE id = 'p1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();
        assert_eq!(name, "Paciente Ficticio Uno");
        assert_eq!(rut.as_deref(), Some("11111111-1"));
        assert_eq!(address.as_deref(), Some("Calle Falsa 123"));
        assert_eq!(region, None, "un paciente creado antes de V2 debe quedar con region = NULL");
        assert_eq!(commune, None, "un paciente creado antes de V2 debe quedar con commune = NULL");

        let status: String = conn.query_row("SELECT status FROM patients WHERE id = 'p2'", [], |r| r.get(0)).unwrap();
        assert_eq!(status, "archivado", "el segundo paciente ficticio también sobrevive intacto");

        // 4. La aplicación sigue funcionando después: se puede editar el
        //    paciente, incluyendo las columnas nuevas, con un UPDATE normal.
        conn.execute(
            "UPDATE patients SET region = 'Región Metropolitana de Santiago', commune = 'Ñuñoa' WHERE id = 'p1'",
            [],
        )
        .unwrap();
        let (region2, commune2): (Option<String>, Option<String>) = conn
            .query_row("SELECT region, commune FROM patients WHERE id = 'p1'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(region2.as_deref(), Some("Región Metropolitana de Santiago"));
        assert_eq!(commune2.as_deref(), Some("Ñuñoa"));
    }

    #[test]
    fn fresh_database_gets_region_and_commune_columns_from_v1_plus_v2() {
        let (conn, _path, _key) = migrated_vault("fresh-db-has-v2-columns");
        let mut stmt = conn.prepare("PRAGMA table_info(patients)").unwrap();
        let columns: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap().map(|c| c.unwrap()).collect();
        assert!(columns.contains(&"region".to_string()), "una base nueva debe tener la columna region desde el arranque");
        assert!(columns.contains(&"commune".to_string()), "una base nueva debe tener la columna commune desde el arranque");
    }

    #[test]
    fn v2_migration_is_idempotent_like_v1() {
        let path = temp_db_path("v2-idempotent");
        let k = key(0xE2);
        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).unwrap();
        conn.execute("INSERT INTO patients (id, full_name, region) VALUES ('p1', 'X', 'Región de Valparaíso')", [])
            .unwrap();

        // Reaplicar migraciones (como en cada arranque) no debe fallar ni
        // intentar re-agregar las columnas de V2 sobre sí mismas.
        run_migrations(&mut conn).expect("reaplicar V1+V2 ya vigentes no debería fallar");

        let region: Option<String> = conn.query_row("SELECT region FROM patients WHERE id = 'p1'", [], |r| r.get(0)).unwrap();
        assert_eq!(region.as_deref(), Some("Región de Valparaíso"));
    }

    // ---------------------------------------------------------------
    // 2c (Fase 8): migración V3 — patient_prep_notes/therapy_tasks — es
    // aditiva y no destructiva sobre un vault V1+V2 real con datos ya
    // insertados.
    // ---------------------------------------------------------------
    #[test]
    fn v3_migration_preserves_all_existing_data() {
        let path = temp_db_path("v3-preserves-data");
        let k = key(0xE3);

        // 1. Llevar un vault a V1+V2 — simula el estado real de un vault
        //    creado antes de Fase 8, con un paciente, una sesión y un
        //    objetivo ya cargados.
        {
            let mut conn = open_vault(&path, &k).unwrap();
            conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
            Migrations::new(vec![M::up(SCHEMA_V1).foreign_key_check(), M::up(SCHEMA_V2).foreign_key_check()])
                .to_latest(&mut conn)
                .unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();

            conn.execute("INSERT INTO patients (id, full_name, status) VALUES ('p1', 'Paciente Ficticio Uno', 'activo')", [])
                .unwrap();
            conn.execute(
                "INSERT INTO sessions (id, patient_id, session_date) VALUES ('s1', 'p1', '2026-01-01')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO therapeutic_goals (id, patient_id, title) VALUES ('g1', 'p1', 'Objetivo previo a Fase 8')",
                [],
            )
            .unwrap();
        }

        // 2. Reabrir y aplicar el mecanismo normal de migraciones
        //    (V1+V2+V3) — exactamente lo que hace la app en cada arranque.
        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).expect("V3 debe aplicarse limpiamente sobre un vault V1+V2 con datos reales");

        // 3. El paciente, la sesión y el objetivo siguen intactos.
        let name: String = conn.query_row("SELECT full_name FROM patients WHERE id = 'p1'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "Paciente Ficticio Uno");
        let session_date: String = conn.query_row("SELECT session_date FROM sessions WHERE id = 's1'", [], |r| r.get(0)).unwrap();
        assert_eq!(session_date, "2026-01-01");
        let goal_title: String =
            conn.query_row("SELECT title FROM therapeutic_goals WHERE id = 'g1'", [], |r| r.get(0)).unwrap();
        assert_eq!(goal_title, "Objetivo previo a Fase 8");

        // 4. Las tablas nuevas existen y aceptan datos reales, referenciando
        //    el paciente/sesión/objetivo que ya existían antes de V3.
        conn.execute(
            "INSERT INTO patient_prep_notes (id, patient_id, origin_session_id, content) VALUES ('pn1', 'p1', 's1', 'Retomar exposición')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO therapy_tasks (id, patient_id, assigned_in_session_id, goal_id, description) \
             VALUES ('t1', 'p1', 's1', 'g1', 'Registro de pensamientos')",
            [],
        )
        .unwrap();
        let prep_status: String = conn.query_row("SELECT status FROM patient_prep_notes WHERE id = 'pn1'", [], |r| r.get(0)).unwrap();
        assert_eq!(prep_status, "pendiente", "el estado por defecto de una preparación nueva es 'pendiente'");
        let task_status: String = conn.query_row("SELECT status FROM therapy_tasks WHERE id = 't1'", [], |r| r.get(0)).unwrap();
        assert_eq!(task_status, "pendiente", "el estado por defecto de una tarea nueva es 'pendiente'");
    }

    #[test]
    fn fresh_database_has_v3_tables() {
        let (conn, _path, _key) = migrated_vault("fresh-db-has-v3-tables");
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('patient_prep_notes', 'therapy_tasks')")
            .unwrap();
        let names: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|n| n.unwrap()).collect();
        assert_eq!(names.len(), 2, "una base nueva debe tener ambas tablas de Fase 8 desde el arranque");
    }

    #[test]
    fn v3_migration_is_idempotent() {
        let path = temp_db_path("v3-idempotent");
        let k = key(0xE4);
        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).unwrap();
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        conn.execute("INSERT INTO patient_prep_notes (id, patient_id, content) VALUES ('pn1', 'p1', 'Nota')", []).unwrap();

        // Reaplicar migraciones (como en cada arranque) no debe fallar ni
        // intentar recrear las tablas de V3 sobre sí mismas.
        run_migrations(&mut conn).expect("reaplicar V1+V2+V3 ya vigentes no debería fallar");

        let content: String = conn.query_row("SELECT content FROM patient_prep_notes WHERE id = 'pn1'", [], |r| r.get(0)).unwrap();
        assert_eq!(content, "Nota");
    }

    #[test]
    fn therapy_task_rejects_invalid_status() {
        let (conn, _path, _key) = migrated_vault("v3-task-invalid-status");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        let err = conn
            .execute(
                "INSERT INTO therapy_tasks (id, patient_id, description, status) VALUES ('t1', 'p1', 'Tarea', 'inventado')",
                [],
            )
            .expect_err("un estado fuera del CHECK debe rechazarse a nivel de base de datos");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn patient_prep_note_rejects_invalid_status() {
        let (conn, _path, _key) = migrated_vault("v3-prep-invalid-status");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        let err = conn
            .execute(
                "INSERT INTO patient_prep_notes (id, patient_id, content, status) VALUES ('pn1', 'p1', 'Nota', 'inventado')",
                [],
            )
            .expect_err("un estado fuera del CHECK debe rechazarse a nivel de base de datos");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    // ---------------------------------------------------------------
    // 2d (Fase 9): migración V4 — treatment_episodes/episode_clinical_profile
    // + episode_id en sessions/therapeutic_goals + backfill legacy — es
    // aditiva y no destructiva sobre un vault V1+V2+V3 real con datos ya
    // insertados.
    // ---------------------------------------------------------------
    #[test]
    fn fresh_database_has_v4_tables_and_columns() {
        let (conn, _path, _key) = migrated_vault("fresh-db-has-v4");
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('treatment_episodes', 'episode_clinical_profile')")
            .unwrap();
        let names: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|n| n.unwrap()).collect();
        assert_eq!(names.len(), 2, "una base nueva debe tener ambas tablas de Fase 9 desde el arranque");

        let mut stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
        let cols: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap().map(|c| c.unwrap()).collect();
        assert!(cols.contains(&"episode_id".to_string()), "sessions debe tener episode_id desde el arranque");

        let mut stmt = conn.prepare("PRAGMA table_info(therapeutic_goals)").unwrap();
        let cols: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap().map(|c| c.unwrap()).collect();
        assert!(cols.contains(&"episode_id".to_string()), "therapeutic_goals debe tener episode_id desde el arranque");
    }

    #[test]
    fn v4_migration_is_idempotent() {
        let path = temp_db_path("v4-idempotent");
        let k = key(0xE5);
        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).unwrap();
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        conn.execute("INSERT INTO treatment_episodes (id, patient_id, started_at) VALUES ('ep1', 'p1', '2026-01-01')", []).unwrap();

        run_migrations(&mut conn).expect("reaplicar V1+V2+V3+V4 ya vigentes no debería fallar");

        let started_at: String = conn.query_row("SELECT started_at FROM treatment_episodes WHERE id = 'ep1'", [], |r| r.get(0)).unwrap();
        assert_eq!(started_at, "2026-01-01");
    }

    #[test]
    fn treatment_episode_rejects_invalid_status() {
        let (conn, _path, _key) = migrated_vault("v4-episode-invalid-status");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        let err = conn
            .execute(
                "INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES ('ep1', 'p1', '2026-01-01', 'inventado')",
                [],
            )
            .expect_err("un estado fuera del CHECK debe rechazarse a nivel de base de datos");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn a_second_active_episode_for_the_same_patient_is_rejected_at_database_level() {
        let (conn, _path, _key) = migrated_vault("v4-one-active-episode");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        conn.execute("INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES ('ep1', 'p1', '2026-01-01', 'activo')", []).unwrap();

        let err = conn
            .execute("INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES ('ep2', 'p1', '2026-02-01', 'activo')", [])
            .expect_err("un segundo proceso activo para el mismo paciente debe rechazarse a nivel de base de datos");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));

        // Pero un segundo proceso PAUSADO sí es aceptado — el índice único
        // parcial solo restringe status = 'activo'.
        conn.execute("INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES ('ep3', 'p1', '2026-02-01', 'pausado')", [])
            .unwrap();
    }

    #[test]
    fn v4_migration_preserves_all_existing_data_and_does_not_touch_patient_clinical_profile() {
        let path = temp_db_path("v4-preserves-data");
        let k = key(0xE6);

        // 1. Llevar un vault a V1+V2+V3 — simula el estado real de un vault
        //    creado antes de Fase 9, con un paciente con sesión, objetivo,
        //    antecedentes clínicos y una tarea/preparación ya cargados.
        {
            let mut conn = open_vault(&path, &k).unwrap();
            conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
            Migrations::new(vec![
                M::up(SCHEMA_V1).foreign_key_check(),
                M::up(SCHEMA_V2).foreign_key_check(),
                M::up(SCHEMA_V3).foreign_key_check(),
            ])
            .to_latest(&mut conn)
            .unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();

            conn.execute(
                "INSERT INTO patients (id, full_name, status, intake_date) VALUES ('p1', 'Paciente Ficticio Uno', 'activo', '2025-03-01')",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO sessions (id, patient_id, session_date) VALUES ('s1', 'p1', '2025-03-10')", []).unwrap();
            conn.execute("INSERT INTO sessions (id, patient_id, session_date) VALUES ('s2', 'p1', '2025-04-05')", []).unwrap();
            conn.execute("INSERT INTO therapeutic_goals (id, patient_id, title) VALUES ('g1', 'p1', 'Objetivo previo a Fase 9')", []).unwrap();
            conn.execute(
                "INSERT INTO patient_clinical_profile (patient_id, presenting_problem, primary_diagnosis_code, diagnosis_notes, risk_flags, relevant_medical_notes) \
                 VALUES ('p1', 'Duelo', 'F43.2', 'Notas diagnósticas previas', '[\"riesgo histórico\"]', 'Antecedente médico longitudinal')",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO patient_prep_notes (id, patient_id, content) VALUES ('pn1', 'p1', 'Preparación previa')", []).unwrap();
            conn.execute("INSERT INTO therapy_tasks (id, patient_id, description) VALUES ('t1', 'p1', 'Tarea previa')", []).unwrap();

            // Paciente sin ninguna actividad clínica relevante — no debe
            // recibir proceso legacy.
            conn.execute("INSERT INTO patients (id, full_name, status) VALUES ('p2', 'Paciente Sin Actividad', 'activo')", []).unwrap();

            // Paciente ya dado de alta — su proceso legacy debe nacer 'cerrado'.
            conn.execute("INSERT INTO patients (id, full_name, status) VALUES ('p3', 'Paciente De Alta', 'alta')", []).unwrap();
            conn.execute("INSERT INTO sessions (id, patient_id, session_date) VALUES ('s3', 'p3', '2024-06-01')", []).unwrap();
        }

        // 2. Reabrir y aplicar el mecanismo normal de migraciones
        //    (V1+V2+V3+V4) — exactamente lo que hace la app en cada arranque.
        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).expect("V4 debe aplicarse limpiamente sobre un vault V1+V2+V3 con datos reales");

        // 3. Todos los datos previos siguen intactos, sin excepción.
        let name: String = conn.query_row("SELECT full_name FROM patients WHERE id = 'p1'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "Paciente Ficticio Uno");
        let goal_title: String = conn.query_row("SELECT title FROM therapeutic_goals WHERE id = 'g1'", [], |r| r.get(0)).unwrap();
        assert_eq!(goal_title, "Objetivo previo a Fase 9");
        let prep_content: String = conn.query_row("SELECT content FROM patient_prep_notes WHERE id = 'pn1'", [], |r| r.get(0)).unwrap();
        assert_eq!(prep_content, "Preparación previa");
        let task_desc: String = conn.query_row("SELECT description FROM therapy_tasks WHERE id = 't1'", [], |r| r.get(0)).unwrap();
        assert_eq!(task_desc, "Tarea previa");

        // 4. patient_clinical_profile NO se tocó: los cinco campos originales
        //    siguen exactamente iguales, incluidos los tres que también se
        //    copiaron al episodio legacy.
        let (presenting, diag_code, diag_notes): (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT presenting_problem, primary_diagnosis_code, diagnosis_notes FROM patient_clinical_profile WHERE patient_id = 'p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let (risk_flags, medical_notes): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT risk_flags, relevant_medical_notes FROM patient_clinical_profile WHERE patient_id = 'p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(presenting.as_deref(), Some("Duelo"));
        assert_eq!(diag_code.as_deref(), Some("F43.2"));
        assert_eq!(diag_notes.as_deref(), Some("Notas diagnósticas previas"));
        assert_eq!(risk_flags.as_deref(), Some(r#"["riesgo histórico"]"#));
        assert_eq!(medical_notes.as_deref(), Some("Antecedente médico longitudinal"));

        // 5. Proceso legacy creado para p1: started_at = fecha de la sesión
        //    más antigua (2025-03-10, no la más reciente ni intake_date),
        //    status = 'activo' (patients.status = 'activo').
        let (p1_started, p1_status): (String, String) = conn
            .query_row("SELECT started_at, status FROM treatment_episodes WHERE id = 'legacy-p1'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(p1_started, "2025-03-10");
        assert_eq!(p1_status, "activo");

        // 6. Ambas sesiones y el objetivo de p1 quedan asociados a su único
        //    proceso legacy.
        let s1_episode: String = conn.query_row("SELECT episode_id FROM sessions WHERE id = 's1'", [], |r| r.get(0)).unwrap();
        let s2_episode: String = conn.query_row("SELECT episode_id FROM sessions WHERE id = 's2'", [], |r| r.get(0)).unwrap();
        let g1_episode: String = conn.query_row("SELECT episode_id FROM therapeutic_goals WHERE id = 'g1'", [], |r| r.get(0)).unwrap();
        assert_eq!(s1_episode, "legacy-p1");
        assert_eq!(s2_episode, "legacy-p1");
        assert_eq!(g1_episode, "legacy-p1");

        // 7. episode_clinical_profile del proceso legacy de p1 contiene
        //    exactamente los tres campos específicos de proceso — nunca
        //    risk_flags ni relevant_medical_notes (no existen esas columnas
        //    en esta tabla).
        let (e_presenting, e_diag_code, e_diag_notes): (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT presenting_problem, primary_diagnosis_code, diagnosis_notes FROM episode_clinical_profile WHERE episode_id = 'legacy-p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(e_presenting.as_deref(), Some("Duelo"));
        assert_eq!(e_diag_code.as_deref(), Some("F43.2"));
        assert_eq!(e_diag_notes.as_deref(), Some("Notas diagnósticas previas"));

        // 8. p2 (sin sesiones/objetivos/antecedentes) NO recibe proceso
        //    legacy — no se crean episodios basura sin datos que agrupar.
        let p2_episode_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM treatment_episodes WHERE patient_id = 'p2'", [], |r| r.get(0)).unwrap();
        assert_eq!(p2_episode_count, 0);

        // 9. p3 (patients.status = 'alta') recibe un proceso legacy YA
        //    'cerrado' — no 'activo'.
        let p3_status: String = conn.query_row("SELECT status FROM treatment_episodes WHERE id = 'legacy-p3'", [], |r| r.get(0)).unwrap();
        assert_eq!(p3_status, "cerrado");
    }

    #[test]
    fn v4_legacy_episode_started_at_falls_back_to_intake_date_then_created_at() {
        let path = temp_db_path("v4-legacy-started-at-fallback");
        let k = key(0xE7);
        {
            let mut conn = open_vault(&path, &k).unwrap();
            conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
            Migrations::new(vec![
                M::up(SCHEMA_V1).foreign_key_check(),
                M::up(SCHEMA_V2).foreign_key_check(),
                M::up(SCHEMA_V3).foreign_key_check(),
            ])
            .to_latest(&mut conn)
            .unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();

            // Paciente con objetivo (sin sesiones) e intake_date: started_at
            // debe usar intake_date, no created_at.
            conn.execute("INSERT INTO patients (id, full_name, intake_date) VALUES ('p1', 'X', '2024-05-20')", []).unwrap();
            conn.execute("INSERT INTO therapeutic_goals (id, patient_id, title) VALUES ('g1', 'p1', 'Objetivo')", []).unwrap();

            // Paciente con solo antecedentes clínicos, sin intake_date: debe
            // caer a la fecha (sin hora) de created_at.
            conn.execute("INSERT INTO patients (id, full_name) VALUES ('p2', 'Y')", []).unwrap();
            conn.execute("INSERT INTO patient_clinical_profile (patient_id) VALUES ('p2')", []).unwrap();
        }

        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).unwrap();

        let p1_started: String = conn.query_row("SELECT started_at FROM treatment_episodes WHERE id = 'legacy-p1'", [], |r| r.get(0)).unwrap();
        assert_eq!(p1_started, "2024-05-20");

        let (p2_started, p2_created_at): (String, String) = conn
            .query_row("SELECT te.started_at, p.created_at FROM treatment_episodes te JOIN patients p ON p.id = te.patient_id WHERE te.id = 'legacy-p2'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(p2_started, &p2_created_at[0..10]);
    }

    #[test]
    fn v5_migration_creates_episode_closures_and_preserves_v4_data() {
        let path = temp_db_path("v5-preserves-v4-data");
        let k = key(0xE8);
        {
            let mut conn = open_vault(&path, &k).unwrap();
            conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
            Migrations::new(vec![
                M::up(SCHEMA_V1).foreign_key_check(),
                M::up(SCHEMA_V2).foreign_key_check(),
                M::up(SCHEMA_V3).foreign_key_check(),
                M::up(SCHEMA_V4).foreign_key_check(),
            ])
            .to_latest(&mut conn)
            .unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();

            conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'Paciente Previo A V5')", []).unwrap();
            conn.execute(
                "INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES ('ep1', 'p1', '2025-01-01', 'activo')",
                [],
            )
            .unwrap();
        }

        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).expect("V5 debe aplicarse limpiamente sobre un vault V1-V4 con datos reales");

        let patient_name: String = conn.query_row("SELECT full_name FROM patients WHERE id = 'p1'", [], |r| r.get(0)).unwrap();
        assert_eq!(patient_name, "Paciente Previo A V5");
        let episode_status: String = conn.query_row("SELECT status FROM treatment_episodes WHERE id = 'ep1'", [], |r| r.get(0)).unwrap();
        assert_eq!(episode_status, "activo", "V5 no debe modificar el estado de procesos existentes");

        let closures_count: i64 = conn.query_row("SELECT COUNT(*) FROM episode_closures", [], |r| r.get(0)).unwrap();
        assert_eq!(closures_count, 0, "V5 no crea ningún cierre retroactivo — es puramente aditiva");
    }

    #[test]
    fn v5_migration_is_idempotent() {
        let path = temp_db_path("v5-idempotent");
        let k = key(0xE9);
        let mut conn = open_vault(&path, &k).unwrap();
        run_migrations(&mut conn).unwrap();
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        conn.execute("INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES ('ep1', 'p1', '2026-01-01', 'cerrado')", []).unwrap();
        conn.execute(
            "INSERT INTO episode_closures (id, episode_id, closed_at, reason, outcome) VALUES ('c1', 'ep1', '2026-02-01', 'alta', 'objetivos_logrados')",
            [],
        )
        .unwrap();

        run_migrations(&mut conn).expect("reaplicar V1-V5 ya vigentes no debería fallar");

        let reason: String = conn.query_row("SELECT reason FROM episode_closures WHERE id = 'c1'", [], |r| r.get(0)).unwrap();
        assert_eq!(reason, "alta");
    }

    #[test]
    fn episode_closure_rejects_invalid_reason() {
        let (conn, _path, _key) = migrated_vault("v5-invalid-reason");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        conn.execute("INSERT INTO treatment_episodes (id, patient_id, started_at) VALUES ('ep1', 'p1', '2026-01-01')", []).unwrap();
        let err = conn
            .execute(
                "INSERT INTO episode_closures (id, episode_id, closed_at, reason, outcome) VALUES ('c1', 'ep1', '2026-01-05', 'inventado', 'objetivos_logrados')",
                [],
            )
            .expect_err("un motivo fuera de la taxonomía debe rechazarse a nivel de base de datos");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn episode_closure_rejects_invalid_outcome() {
        let (conn, _path, _key) = migrated_vault("v5-invalid-outcome");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        conn.execute("INSERT INTO treatment_episodes (id, patient_id, started_at) VALUES ('ep1', 'p1', '2026-01-01')", []).unwrap();
        let err = conn
            .execute(
                "INSERT INTO episode_closures (id, episode_id, closed_at, reason, outcome) VALUES ('c1', 'ep1', '2026-01-05', 'alta', 'inventado')",
                [],
            )
            .expect_err("un resultado fuera de la taxonomía debe rechazarse a nivel de base de datos");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
    }

    #[test]
    fn a_second_active_closure_for_the_same_episode_is_rejected_at_database_level() {
        let (conn, _path, _key) = migrated_vault("v5-one-active-closure");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        conn.execute("INSERT INTO treatment_episodes (id, patient_id, started_at, status) VALUES ('ep1', 'p1', '2026-01-01', 'cerrado')", []).unwrap();
        conn.execute(
            "INSERT INTO episode_closures (id, episode_id, closed_at, reason, outcome) VALUES ('c1', 'ep1', '2026-02-01', 'alta', 'objetivos_logrados')",
            [],
        )
        .unwrap();

        let err = conn
            .execute(
                "INSERT INTO episode_closures (id, episode_id, closed_at, reason, outcome) VALUES ('c2', 'ep1', '2026-02-02', 'alta', 'objetivos_logrados')",
                [],
            )
            .expect_err("un segundo cierre vigente para el mismo proceso debe rechazarse a nivel de base de datos");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));

        // Pero un segundo cierre ANULADO sí es aceptado — el índice único
        // parcial solo restringe reverted_at IS NULL. Simula la historia de
        // un cierre corregido: c1 se anula y c2 pasa a ser el vigente.
        conn.execute("UPDATE episode_closures SET reverted_at = '2026-02-03T00:00:00.000Z', reverted_reason = 'Motivo incorrecto' WHERE id = 'c1'", [])
            .unwrap();
        conn.execute(
            "INSERT INTO episode_closures (id, episode_id, closed_at, reason, outcome) VALUES ('c2', 'ep1', '2026-02-02', 'derivacion', 'parcialmente_logrados')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn episode_closure_requires_both_or_neither_reverted_fields() {
        let (conn, _path, _key) = migrated_vault("v5-reverted-pair");
        conn.execute("INSERT INTO patients (id, full_name) VALUES ('p1', 'X')", []).unwrap();
        conn.execute("INSERT INTO treatment_episodes (id, patient_id, started_at) VALUES ('ep1', 'p1', '2026-01-01')", []).unwrap();
        let err = conn
            .execute(
                "INSERT INTO episode_closures (id, episode_id, closed_at, reason, outcome, reverted_at) \
                 VALUES ('c1', 'ep1', '2026-01-05', 'alta', 'objetivos_logrados', '2026-01-06T00:00:00.000Z')",
                [],
            )
            .expect_err("reverted_at sin reverted_reason debe rechazarse a nivel de base de datos");
        assert!(matches!(err, SqliteError::SqliteFailure(_, _)));
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
