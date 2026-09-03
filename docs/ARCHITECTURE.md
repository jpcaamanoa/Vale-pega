# Cuaderno Clínico — Arquitectura Fase 1

Documento de arquitectura técnica para "Cuaderno Clínico", aplicación de escritorio local-first
para uso personal de una psicóloga clínica independiente. Este documento se revisa y aprueba
antes de comenzar la implementación.

---

## 1. Resumen ejecutivo de arquitectura

```
┌─────────────────────────────────────────────────────────────┐
│  React + TypeScript (Vista)                                  │
│  - Componentes, routing, formularios, gráficos, canvas       │
│  - Zustand: SOLO estado de UI efímero (nunca dato clínico)   │
│  - Zod: validación de UX (no autoritativa)                   │
└───────────────────────────┬───────────────────────────────────┘
                            │  IPC tipada (comandos Tauri, uno por operación)
┌───────────────────────────▼───────────────────────────────────┐
│  Rust — capa "commands" (borde de Tauri)                      │
│  - Deserializa DTO, valida sesión/lock, delega a services      │
│  - Cero SQL aquí. Cero lógica de negocio aquí.                 │
├───────────────────────────┬───────────────────────────────────┤
│  Rust — capa "services" (lógica clínica y de negocio)          │
│  - Reglas: "no borrar paciente con sesiones futuras", etc.     │
│  - Orquesta repos + seguridad + archivos + Google Calendar     │
├───────────────────────────┬───────────────────────────────────┤
│  Rust — capa "repositories" (acceso a datos, SOLO SQL)         │
├───────────────────────────┬───────────────────────────────────┤
│  SQLite + SQLCipher (rusqlite)   │   Vault de archivos cifrado │
│  Datos estructurados              │   Documentos individuales   │
└───────────────────────────────────┴─────────────────────────────┘
        Módulos transversales: security (KDF/keychain/lock),
        calendar_sync (Google), backup, search (FTS5)
```

**Reglas de frontera que se imponen estructuralmente (no por convención):**
- React **nunca** ve SQL. Los comandos Tauri son verbos de negocio (`create_patient`,
  `list_sessions_for_patient`), nunca un `run_sql(query)` genérico — eso reintroduciría
  exactamente el acoplamiento que se quiere evitar.
- Rust no importa nada de UI; su única "vista" del mundo son DTOs serializables (`serde`).
- Zustand no persiste nada clínico (nada de `persist` middleware sobre esos slices). Solo
  cachea en memoria lo que la usuaria está mirando ahora mismo — que de todas formas vive en
  RAM del proceso mientras la app está desbloqueada, igual que en cualquier app de escritorio.
- Ningún dato clínico sale del proceso local salvo Google Calendar, y ahí solo un identificador
  y un texto genérico (ver sección 6).

---

## 2. Decisiones técnicas

| Decisión | Elegido | Por qué | Riesgo | Alternativa descartada |
|---|---|---|---|---|
| Framework de escritorio | **Tauri** | Rust en el core, sin Node.js expuesto al renderer, IPC con allowlist explícito, binarios mucho más chicos, mejor postura de seguridad por defecto | WebView del SO (WebView2 en Windows, WKWebView en macOS) puede tener pequeñas inconsistencias de render/CSS entre plataformas; ecosistema de plugins más joven que Electron | Electron: motor completo de Chromium+Node embebido, superficie de ataque mayor, y el patrón "Node accesible desde el renderer" es justo lo que se quiere evitar |
| Base de datos | **SQLite + SQLCipher** | Relacional, madura, soporta FTS5, cifrado transparente a nivel de página de archivo completo, cero infraestructura | SQLCipher añade complejidad de build (ver abajo) | Postgres embebido / DuckDB: pensados para otros casos de uso, sin cifrado transparente equivalente; IndexedDB: no aplica fuera del navegador |
| Binding Rust↔SQLCipher | **`rusqlite` con feature `bundled-sqlcipher-vendored-openssl`** | Vendoriza OpenSSL, evita depender de que la máquina de la usuaria (o el pipeline de build en Windows) tenga OpenSSL instalado | Tiempos de compilación más largos por vendoring; hay que fijar bien la versión | `sqlx` con sqlcipher: soporte menos maduro para esta combinación específica |
| Migraciones | **`rusqlite_migration`** | Trabaja sobre la misma conexión ya "keyed" (con `PRAGMA key` aplicado), a diferencia de herramientas que asumen SQLite plano | Ninguna relevante | Migraciones a mano: reinventar la rueda sin necesidad |
| Estado UI | **Zustand** | Ligero, sin boilerplate, encaja con "solo estado de interfaz" | Riesgo real: usar `persist` middleware "por comodidad" y terminar guardando datos clínicos en `localStorage` del WebView, sin cifrar. **Prohibido explícitamente como regla de arquitectura**, no solo de estilo | Redux: más ceremonia de la que este proyecto necesita |
| Formulación clínica | **React Flow** | Encaja 1:1 con el modelo nodo/conexión/posición requerido | Ninguno relevante para este alcance | — |
| Búsqueda | **FTS5 con external content tables** | Ver sección 8 — evita duplicar el texto clínico en una segunda tabla | — | Buscador externo (Elasticsearch, etc.): fuera de alcance para app local de una sola usuaria |
| Archivos | **Sistema de archivos, no BLOB en SQLite** | Un documento clínico o PDF adjunto en SQLite infla el archivo cifrado, hace los backups pesados y lentos de restaurar | Requiere cifrado propio por archivo (ver sección 7) | BLOB en SQLite: técnicamente posible, pero mala práctica operacional a esta escala |

### Cuestionamientos importantes al diseño inicial

**a) Derivación de clave: no confiar en el KDF interno de SQLCipher.**
Si se usa `PRAGMA key = 'frase-de-paso'` directamente, SQLCipher deriva la clave con
PBKDF2-HMAC-SHA512 (256.000 iteraciones por defecto). Es razonable, pero **Argon2id**
(memory-hard) resiste mucho mejor ataques con GPU/ASIC. Se propone: derivar la clave con
Argon2id (crate `argon2`, implementación de RustCrypto, no propia) y entregarla a SQLCipher en
**modo raw key** (`PRAGMA key = "x'<hex de 64 caracteres>'"`), evitando el KDF débil por
defecto. Patrón documentado oficialmente por SQLCipher, no una improvisación.

**b) "Contraseña maestra" como único punto de fallo, sin plan B.**
Si solo hay una contraseña y se olvida, los datos clínicos de años de trabajo se pierden para
siempre. Se propone **cifrado por sobres (envelope encryption)**, ver sección 5 — el mismo
patrón que usan FileVault, BitLocker y 1Password.

**c) Separación administrativa/clínica "solo conceptual" — se cuestiona como insuficiente, pero
también se cuestiona dividir en dos bases de datos.**
Se recomienda **una sola base de datos SQLCipher** (más simple de respaldar, mantiene
integridad referencial nativa) pero con **tablas físicamente separadas** y una frontera dura en
el código Rust (un servicio de "ficha administrativa" que ni siquiera importa el módulo
clínico). Dos archivos SQLCipher separados sí es técnicamente posible (con
`ATTACH DATABASE ... KEY`), pero SQLite **no valida foreign keys entre bases adjuntas**, así que
se perdería integridad referencial automática entre paciente y sus notas — un costo real por un
beneficio que hoy no existe (no hay ningún dato administrativo que se planee sincronizar a la
nube). Si en el futuro se quiere sincronizar solo la facturación a un sistema contable externo,
ahí sí se justificaría partir en dos bases.

**d) Biometría (Touch ID / Windows Hello) — no es tan simple como "guardar en el keychain".**
Guardar un secreto en el Keychain de macOS o en Windows Credential Manager es directo (crate
`keyring`). Pero **exigir que ese secreto solo se libere tras un prompt biométrico real**
requiere código nativo adicional específico por plataforma (en macOS, `SecAccessControl` con
flag `.biometryCurrentSet` vía el crate `security-framework`; en Windows, integrar Windows Hello
vía `UserConsentVerifier`, no trivial desde una app de escritorio Win32). Se recomienda
**diferir biometría a Fase 7** y arrancar con contraseña + código de recuperación.

**e) Google OAuth "Desktop app" — las credenciales no pueden generarse por la usuaria.**
En Fase 3 la usuaria deberá crear un proyecto en Google Cloud Console, habilitar la Calendar
API, configurar la pantalla de consentimiento OAuth (puede quedar en modo "Testing", solo para
su cuenta) y generar un Client ID tipo "Desktop app". Toda la arquitectura queda lista (PKCE,
loopback redirect, almacenamiento seguro de tokens) pero esas credenciales no pueden generarse
ni simularse externamente.

**f) Elección de Tauri y compatibilidad con el objetivo futuro de multiplataforma (agregado el 31
de agosto de 2026).** La elección de Tauri en esta sección no fue solo por postura de seguridad:
Tauri también soporta objetivos móviles (iOS/iPadOS vía `tauri-mobile`) sobre el mismo core en
Rust, lo que hace que el objetivo futuro de extender la app a iPhone/iPad (ver sección 15) sea
compatible con la arquitectura ya elegida, sin requerir un cambio de framework más adelante. Esto
no implica trabajo alguno de Fase 1 ni adelanta esa fase — es simplemente la constatación de que
la decisión tomada en 1.1 no bloquea esa dirección futura.

---

## 3. Estructura de carpetas

```
cuaderno-clinico/
├── src/                              # React + TypeScript (frontend)
│   ├── app/                          # bootstrap, router, providers globales
│   ├── components/                   # componentes de UI genéricos y reutilizables
│   │   └── ui/                       # primitivos (Button, Input, Modal, DataTable...)
│   ├── features/                     # un módulo por dominio de negocio
│   │   ├── patients/
│   │   │   ├── components/
│   │   │   ├── hooks/
│   │   │   ├── api.ts                # wrappers tipados sobre invoke() de Tauri
│   │   │   └── store.ts              # slice de Zustand (solo UI state de este feature)
│   │   ├── sessions/
│   │   ├── formulation/              # canvas React Flow
│   │   ├── goals/
│   │   ├── assessments/
│   │   ├── documents/
│   │   ├── agenda/
│   │   ├── payments/
│   │   ├── library/
│   │   ├── toolkit/
│   │   ├── reminders/
│   │   └── settings/
│   ├── search/                       # command palette (Cmd/Ctrl+K)
│   ├── shared/
│   │   ├── types/                    # tipos TS espejo de los DTOs de Rust (serde)
│   │   ├── schemas/                  # esquemas Zod (validación de formularios, UX)
│   │   └── ipc/                      # cliente tipado genérico sobre invoke()
│   └── styles/
│
├── src-tauri/                        # Rust backend
│   ├── src/
│   │   ├── commands/                 # capa fina expuesta a Tauri (1 archivo por dominio)
│   │   ├── services/                 # lógica de negocio/clínica
│   │   ├── repositories/             # SQL puro, un repo por tabla/agregado
│   │   ├── db/
│   │   │   ├── connection.rs         # apertura, keying, verificación de contraseña
│   │   │   └── migrations/           # migraciones versionadas (rusqlite_migration)
│   │   ├── security/
│   │   │   ├── kdf.rs                # Argon2id
│   │   │   ├── envelope.rs           # wrap/unwrap del DEK (KEK contraseña / recuperación)
│   │   │   ├── keychain.rs           # integración OS (crate keyring)
│   │   │   └── lock.rs               # temporizador de bloqueo automático
│   │   ├── files/                    # vault de documentos cifrados (AES-256-GCM)
│   │   ├── calendar/                 # OAuth PKCE + sync Google Calendar
│   │   ├── search/                   # orquestación de consultas FTS5
│   │   ├── backup/                   # crear/verificar/restaurar backups, exportación
│   │   └── shared/                   # tipos DTO, errores, utilidades
│   ├── tests/                        # tests de integración Rust
│   └── Cargo.toml
│
├── docs/
│   └── ARCHITECTURE.md               # este documento
└── package.json
```

Cada carpeta de `features/` en React es un vertical delgado: UI + hooks + llamado a comandos
Tauri. Ninguna lógica clínica vive ahí — solo presentación y estado de interacción.

---

## 4. Modelo de datos

Convenciones globales: `id TEXT PRIMARY KEY` (UUIDv4), `created_at`/`updated_at` en TEXT
ISO-8601, `deleted_at TEXT NULL` para soft delete en entidades reales (no en tablas puente),
`PRAGMA foreign_keys = ON`, `ON DELETE RESTRICT` por defecto salvo donde se indique.

### Pacientes

```sql
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
  status TEXT NOT NULL CHECK (status IN ('activo','inactivo','alta','archivado')) DEFAULT 'activo',
  referred_by TEXT,
  intake_date TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_patients_status ON patients(status) WHERE deleted_at IS NULL;
```

> **Fase 6.1 (3 de septiembre de 2026):** migración `V2`, exclusivamente aditiva, agregó
> `region TEXT` y `commune TEXT` a `patients` (ambas nullable, sin `DEFAULT`, sin backfill). El
> bloque de arriba sigue mostrando `SCHEMA_V1` tal como se aplicó en la Fase 1.3, sin editar — así
> se documenta cada versión del esquema por separado en vez de reescribir una ya publicada. Región
> y comuna se validan contra un catálogo cerrado de Chile (16 regiones, 346 comunas, más el valor
> reservado `"Extranjero"`); nunca texto libre. Detalle completo en `docs/geographic-stats.md`.

```sql
-- Separación física de lo clínico-sensible, aunque viva en el mismo archivo cifrado
CREATE TABLE patient_clinical_profile (
  patient_id TEXT PRIMARY KEY REFERENCES patients(id) ON DELETE RESTRICT,
  presenting_problem TEXT,
  primary_diagnosis_code TEXT,
  diagnosis_notes TEXT,
  risk_flags TEXT,          -- JSON: p.ej. ["riesgo_suicida_historico"]
  relevant_medical_notes TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
```

### Sesiones

```sql
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  appointment_id TEXT REFERENCES appointments(id) ON DELETE SET NULL,
  session_date TEXT NOT NULL,
  start_time TEXT,
  duration_minutes INTEGER,
  modality TEXT CHECK (modality IN ('presencial','online','telefonico')),
  status TEXT NOT NULL CHECK (status IN ('programada','realizada','cancelada','no_asistio')) DEFAULT 'programada',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_sessions_patient_date ON sessions(patient_id, session_date);

CREATE TABLE session_notes (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE RESTRICT,
  content TEXT,
  interventions TEXT,
  homework_tasks TEXT,
  next_focus TEXT,
  version INTEGER NOT NULL DEFAULT 1,
  is_locked INTEGER NOT NULL DEFAULT 0,    -- 0 = borrador editable (autoguardado), 1 = cerrada (inmutable)
  is_current INTEGER NOT NULL DEFAULT 1,   -- 1 = versión vigente de la nota de esta sesión
  closed_at TEXT,
  superseded_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_session_notes_session ON session_notes(session_id);
-- Garantiza a nivel de base de datos que solo puede existir una versión vigente por sesión.
CREATE UNIQUE INDEX idx_session_notes_current ON session_notes(session_id) WHERE is_current = 1;
```

> **Confirmado (Fase 1 MVP): versionado activo desde el inicio.** Ciclo de vida de una nota:
> **Borrador → Cerrada**.
> - En borrador (`is_locked = 0`): se edita en el mismo registro, con autoguardado.
> - Al cerrar (`is_locked = 1`, `closed_at` = ahora): la fila queda protegida en la capa de
>   servicio contra escritura — no se sobrescribe silenciosamente.
> - Al modificar una nota cerrada: el servicio Rust **inserta una fila nueva**
>   (`version = version_anterior + 1`, `is_locked = 0`, `is_current = 1`, precargada con el
>   contenido de la versión anterior como punto de partida) y marca la fila anterior
>   `is_current = 0`, `superseded_at = ahora`. Ninguna versión se pierde; el historial completo
>   se consulta con `SELECT * FROM session_notes WHERE session_id = ? ORDER BY version`.
> - El índice único parcial `idx_session_notes_current` hace que sea la base de datos, no solo
>   la aplicación, la que impide tener dos versiones "vigentes" simultáneas por sesión.

### Formulación

```sql
CREATE TABLE case_formulations (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  title TEXT NOT NULL,
  model_type TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);

CREATE TABLE formulation_versions (
  id TEXT PRIMARY KEY,
  formulation_id TEXT NOT NULL REFERENCES case_formulations(id) ON DELETE CASCADE,
  version_number INTEGER NOT NULL,
  summary_text TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE formulation_nodes (
  id TEXT PRIMARY KEY,
  formulation_version_id TEXT NOT NULL REFERENCES formulation_versions(id) ON DELETE CASCADE,
  node_type TEXT NOT NULL,   -- problema, hipotesis, factor_predisponente, etc.
  label TEXT NOT NULL,
  description TEXT,
  position_x REAL NOT NULL,
  position_y REAL NOT NULL
);

CREATE TABLE formulation_edges (
  id TEXT PRIMARY KEY,
  formulation_version_id TEXT NOT NULL REFERENCES formulation_versions(id) ON DELETE CASCADE,
  source_node_id TEXT NOT NULL REFERENCES formulation_nodes(id) ON DELETE CASCADE,
  target_node_id TEXT NOT NULL REFERENCES formulation_nodes(id) ON DELETE CASCADE,
  relation_label TEXT
);
```

`position_x`/`position_y` mapean directo a las coordenadas que React Flow necesita — sin
transformación intermedia.

### Objetivos

```sql
CREATE TABLE therapeutic_goals (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  formulation_id TEXT REFERENCES case_formulations(id) ON DELETE SET NULL,
  title TEXT NOT NULL,
  description TEXT,
  status TEXT NOT NULL CHECK (status IN ('activo','logrado','pausado','descartado')) DEFAULT 'activo',
  target_date TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);

CREATE TABLE goal_indicators (
  id TEXT PRIMARY KEY,
  goal_id TEXT NOT NULL REFERENCES therapeutic_goals(id) ON DELETE CASCADE,
  description TEXT NOT NULL,
  baseline_value TEXT,
  target_value TEXT
);

CREATE TABLE goal_interventions (
  id TEXT PRIMARY KEY,
  goal_id TEXT NOT NULL REFERENCES therapeutic_goals(id) ON DELETE CASCADE,
  technique_id TEXT REFERENCES clinical_techniques(id) ON DELETE SET NULL,
  description TEXT NOT NULL
);

CREATE TABLE session_goals (        -- N:M sesión<->objetivo
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  goal_id TEXT NOT NULL REFERENCES therapeutic_goals(id) ON DELETE CASCADE,
  progress_note TEXT,
  PRIMARY KEY (session_id, goal_id)
);
```

### Evaluaciones

```sql
CREATE TABLE assessment_instruments (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,     -- ej. "BDI-II", "PHQ-9"
  description TEXT,
  is_custom INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE assessment_administrations (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  instrument_id TEXT NOT NULL REFERENCES assessment_instruments(id) ON DELETE RESTRICT,
  administered_at TEXT NOT NULL,
  context TEXT CHECK (context IN ('ingreso','seguimiento','alta')),
  raw_responses TEXT,        -- JSON opcional, ítem por ítem
  total_score REAL,
  subscale_scores TEXT,      -- JSON
  interpretation_text TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_assessments_patient_instrument ON assessment_administrations(patient_id, instrument_id, administered_at);
```

Esta tabla, ordenada por fecha, **es** la evolución cuantitativa del paciente — Recharts
consulta directamente sobre ella.

### Documentos

```sql
CREATE TABLE documents (
  id TEXT PRIMARY KEY,
  patient_id TEXT REFERENCES patients(id) ON DELETE SET NULL,
  session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  category TEXT CHECK (category IN ('informe','consentimiento','evaluacion_adjunta','receta','correspondencia','otro')),
  original_filename TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  sha256_plaintext TEXT NOT NULL,
  storage_path TEXT NOT NULL UNIQUE,   -- ruta relativa, basada en UUID, sin nombre clínico
  is_clinical INTEGER NOT NULL DEFAULT 1,
  description TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_documents_patient ON documents(patient_id);
```

### Agenda

```sql
CREATE TABLE appointments (
  id TEXT PRIMARY KEY,
  patient_id TEXT REFERENCES patients(id) ON DELETE SET NULL,  -- NULL: bloqueo personal, no-paciente
  title TEXT NOT NULL,               -- texto local rico, NUNCA lo que se envía a Google
  starts_at TEXT NOT NULL,
  ends_at TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('programada','confirmada','cancelada','completada','no_asistio')) DEFAULT 'programada',
  modality TEXT,
  google_event_id TEXT,
  google_calendar_id TEXT,
  last_synced_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE UNIQUE INDEX idx_appointments_google_event ON appointments(google_event_id) WHERE google_event_id IS NOT NULL;
CREATE INDEX idx_appointments_starts_at ON appointments(starts_at) WHERE deleted_at IS NULL;
```

### Pagos

```sql
CREATE TABLE payments (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  amount REAL NOT NULL,
  currency TEXT NOT NULL DEFAULT 'CLP',
  method TEXT CHECK (method IN ('efectivo','transferencia','tarjeta','otro')),
  status TEXT NOT NULL CHECK (status IN ('pendiente','pagado','atrasado','condonado')) DEFAULT 'pendiente',
  due_date TEXT,
  paid_at TEXT,
  notes TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_payments_status_due ON payments(status, due_date);
```

### Biblioteca

```sql
CREATE TABLE library_resources (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  resource_type TEXT CHECK (resource_type IN ('articulo','libro','protocolo','escala','video','enlace')),
  author TEXT,
  source_url TEXT,
  file_document_id TEXT REFERENCES documents(id) ON DELETE SET NULL,
  summary TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE TABLE library_tags (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE);
CREATE TABLE library_resource_tags (
  resource_id TEXT NOT NULL REFERENCES library_resources(id) ON DELETE CASCADE,
  tag_id TEXT NOT NULL REFERENCES library_tags(id) ON DELETE CASCADE,
  PRIMARY KEY (resource_id, tag_id)
);
```

### Herramientas

```sql
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
CREATE TABLE technique_materials (
  id TEXT PRIMARY KEY,
  technique_id TEXT NOT NULL REFERENCES clinical_techniques(id) ON DELETE CASCADE,
  document_id TEXT REFERENCES documents(id) ON DELETE SET NULL,
  title TEXT NOT NULL,
  notes TEXT
);
```

### Recordatorios

```sql
CREATE TABLE reminders (
  id TEXT PRIMARY KEY,
  patient_id TEXT REFERENCES patients(id) ON DELETE SET NULL,
  related_entity_type TEXT,   -- 'session' | 'payment' | 'document' | ...
  related_entity_id TEXT,
  title TEXT NOT NULL,
  description TEXT,
  due_at TEXT,
  status TEXT NOT NULL CHECK (status IN ('pendiente','completado','descartado')) DEFAULT 'pendiente',
  priority TEXT CHECK (priority IN ('baja','media','alta')) DEFAULT 'media',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  completed_at TEXT
);
CREATE INDEX idx_reminders_due_status ON reminders(due_at, status);
```

### Configuración

```sql
CREATE TABLE app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,   -- JSON
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
```

Clave-valor deliberadamente: las preferencias cambian de forma que no vale la pena migrar el
esquema cada vez (tema, minutos de auto-bloqueo, carpeta de backup, duración de sesión por
defecto, moneda, etc.).

---

## 5. Seguridad y cifrado

Diseño de **cifrado por sobres (envelope encryption)** — patrón estándar (FileVault/BitLocker/
1Password), no criptografía propia:

- **DEK** (Data Encryption Key): 256 bits aleatorios, generado una sola vez al crear el "vault".
  Es la clave real que cifra la base SQLCipher y los archivos del vault de documentos. Nunca se
  deriva de la contraseña directamente — así, cambiar la contraseña no obliga a re-cifrar toda
  la base.
- **KEK de contraseña**: derivada con **Argon2id** (crate `argon2`, parámetros memory-hard
  configurables) a partir de la contraseña maestra + una sal aleatoria.
- **KEK de recuperación**: un código aleatorio de alta entropía (formato tipo
  `XXXX-XXXX-XXXX-XXXX-XXXX-XXXX`) que se muestra **una sola vez** al crear el vault, con
  instrucción explícita de guardarlo fuera de la app (papel, gestor de contraseñas).
- El DEK se guarda **envuelto dos veces** (una con cada KEK) en un archivo pequeño y no cifrado
  junto a la base: `vault.meta.json` (contiene sales, parámetros de Argon2id y los dos DEK
  envueltos — nada de esto es secreto por sí solo sin la contraseña o el código).
- Envoltura/desenvoltura con AES-Key-Wrap o AES-256-GCM (crate `aes-gcm`/`aes-kw`, RustCrypto).

**Respondiendo cada punto:**

1. **Derivación de clave**: Argon2id sobre la contraseña, no el PBKDF2 interno de SQLCipher.
2. **Desbloqueo**: la usuaria ingresa contraseña → deriva KEK → desenvuelve DEK →
   `PRAGMA key = "x'<DEK en hex>'"` sobre la conexión SQLite → se valida con una consulta
   trivial (`SELECT count(*) FROM sqlite_master`) para distinguir "contraseña incorrecta" de
   "archivo corrupto".
3. **Contraseña maestra**: nunca se almacena, en ningún formato, en ningún lugar. Solo existe en
   memoria durante el cálculo de Argon2id y se descarta inmediatamente (zeroizada con el crate
   `zeroize`).
4. **Keychain de macOS**: guarda el DEK (o una KEK de conveniencia) protegido con
   `SecAccessControl` + flag `.biometryCurrentSet`, de modo que si se agrega una huella nueva al
   Mac, el ítem se invalida automáticamente. *(La parte biométrica requiere trabajo nativo
   adicional; para el MVP el keychain puede usarse solo como "recordar sesión mientras el
   usuario del SO esté logueado", sin biometría todavía).*
5. **Windows**: Windows Credential Manager vía DPAPI (crate `keyring`), atado a la cuenta de
   Windows de la usuaria. Windows Hello (biometría) requeriría integración adicional vía
   `UserConsentVerifier` — ítem de Fase 7 separado.
6. **Bloqueo automático**: temporizador en Rust (no en el frontend) — a los N minutos de
   inactividad, o al detectar suspensión/bloqueo del SO, se cierra la conexión SQLCipher y se
   **zeroiza el DEK en memoria**, exigiendo desbloqueo completo. La UI solo muestra el
   countdown; la aplicación real del bloqueo vive en el backend.
7. **Biometría**: diferida a Fase 7. Hasta entonces, contraseña + keychain "atado al login del
   SO" como conveniencia intermedia.
8. **Contraseña olvidada**: si además se pierde el código de recuperación y no hay una sesión de
   keychain activa en esa máquina, **los datos son irrecuperables**. Es intencional — es el
   precio de que el cifrado sea real y no tenga puerta trasera.
9. **Robo de la base de datos**: el atacante tiene un archivo SQLCipher cifrado con AES-256; sin
   la contraseña o el código de recuperación, la única vía es fuerza bruta offline contra
   Argon2id, cuyo costo depende de la fortaleza de la contraseña.
10. **Backups**: se protegen exactamente igual, porque contienen los mismos artefactos ya
    cifrados con el DEK (ver sección 9).

**Límite explícito no resuelto por completo:** mientras la app está desbloqueada y corriendo, el
DEK vive en RAM del proceso. Un volcado de memoria (malware con privilegios, herramienta
forense con la máquina encendida) podría exponerlo. Esto es una limitación inherente a
cualquier app de escritorio sin hardware seguro dedicado (enclave/TPM), no específica de esta
arquitectura.

---

## 6. Google Calendar

- **Flujo**: OAuth 2.0 Authorization Code + PKCE, tipo de cliente "Desktop app" en Google Cloud
  Console (debe crearlo la usuaria: proyecto, habilitar Calendar API, configurar pantalla de
  consentimiento en modo "Testing" con su cuenta como test user, generar Client ID). Esas
  credenciales no pueden generarse ni simularse externamente.
- **Autorización**: la app levanta un listener HTTP temporal en `127.0.0.1:<puerto aleatorio>`,
  abre el navegador del sistema hacia la URL de consentimiento de Google, captura el `code` en
  el redirect de loopback, e intercambia código + verifier PKCE por tokens directamente contra
  el endpoint de Google desde Rust (`reqwest`). Sin servidor externo propio.
- **Almacenamiento de tokens**: `access_token`/`refresh_token` van al keychain del SO (crate
  `keyring`), nunca a la base SQLite ni a archivo plano.
- **Qué se envía a Google**: únicamente un título genérico fijo ("Sesión clínica"), hora de
  inicio/fin. La sanitización ocurre en el servicio `calendar_sync` de Rust en el momento del
  push; el título rico y real vive solo en `appointments.title` localmente. Nunca se envían
  nombre, RUT, diagnóstico, motivo de consulta, notas ni evaluaciones.
- **Sincronización**: recomendada **unidireccional (app → Google)** para el MVP de Fase 3 —
  Google Calendar actúa como espejo de solo lectura de la agenda profesional, visible desde el
  celular. Sincronización bidireccional es una decisión de alcance mayor pendiente de confirmar.
- **Creación/modificación/eliminación**: cada operación local sobre `appointments` dispara la
  llamada equivalente a la API de Google; el mapeo vive en `appointments.google_event_id`.
- **Token expirado/revocado**: si el refresh falla, la app marca la integración como
  "desconectada" en Configuración y pide reautorización — nunca bloquea el uso normal de la
  agenda local, que sigue siendo la fuente de verdad.
- **Conflictos**: si el evento espejo fue editado/borrado manualmente desde Google, la próxima
  operación recibirá 404/410; la app marca esa cita como "sin vínculo con Google" para revisión
  manual, sin sobrescribir la agenda local en base a Google.
- **Revocación**: botón "Desconectar Google Calendar" → llama al endpoint de revocación de
  Google, borra tokens del keychain, marca los `google_event_id` existentes como inactivos.

> **Nota (Fase 3, implementado):** esta sección describe el diseño aprobado antes de construirlo;
> `docs/google-calendar.md` es la referencia autoritativa de lo efectivamente implementado, con
> dos precisiones sobre lo escrito arriba: (1) solo el `refresh_token` va al keychain — el
> `access_token` deliberadamente no se cachea en ningún lado, se pide uno nuevo antes de cada
> llamada; (2) desconectar borra el `refresh_token` del keychain y el calendario seleccionado,
> pero no recorre `appointments` marcando sus `google_event_id` como inactivos — un vínculo
> obsoleto se limpia solo, la próxima vez que se intenta usar, por el mismo mecanismo de detección
> de 404/410 que ya cubre un evento borrado manualmente en Google (ver "Conflictos" arriba). Se
> documenta la diferencia en vez de ajustar el diseño original para que coincida.

---

## 7. Archivos y documentos

- **Ubicación**: directorio de datos de la app resuelto vía la API `path` de Tauri
  (`app_data_dir`), ej. `.../CuadernoClinico/vault/documents/`.
- **Nombres internos**: cada archivo se guarda como `<uuid>.enc` — nunca el nombre original. El
  nombre real, tipo MIME y demás metadatos viven solo en la tabla `documents`, dentro del
  archivo SQLCipher cifrado.
- **Cifrado por archivo**: AES-256-GCM (crate `aes-gcm`) con nonce aleatorio por archivo,
  usando el mismo DEK del vault.
- **Relación con pacientes/sesiones**: FK opcionales en `documents.patient_id` /
  `documents.session_id`.
- **Eliminación**: soft delete (`deleted_at`) + movimiento a una carpeta de papelera; el borrado
  físico real ("vaciar papelera") es best-effort. En discos SSD modernos, por wear-leveling, no
  se puede garantizar borrado forense irreversible.
- **Backup/restauración**: se copian los blobs ya cifrados tal cual (rápido, sin re-cifrar).
- **Exportación**: al exportar, se descifra en memoria, se genera el entregable y se escribe en
  texto plano en el destino elegido — con advertencia explícita en la UI.

---

## 8. Búsqueda (Cmd/Ctrl + K)

- Una tabla **FTS5 por dominio buscable**, todas en **modo external content**
  (`content='<tabla_origen>'`): `fts_patients`, `fts_session_notes`, `fts_goals`,
  `fts_assessments`, `fts_documents`, `fts_library`, `fts_techniques`.
- External content significa que FTS5 no duplica el texto, solo guarda el índice invertido
  apuntando a la fila original.
- Triggers `AFTER INSERT/UPDATE/DELETE` en cada tabla origen mantienen el índice sincronizado
  automáticamente.
- El índice FTS5 vive dentro del mismo archivo SQLCipher, así que está tan protegido en reposo
  como el resto de la base.
- Un único comando Tauri `global_search(query)` consulta todas las tablas FTS5, rankea con
  `bm25()`, fusiona resultados y devuelve solo lo necesario para navegar (tipo, id, título,
  fragmento) — nunca el contenido completo de una nota en la lista de resultados.
- Si el vault está bloqueado, no existe conexión a la base, así que la búsqueda global queda
  deshabilitada.

---

## 9. Backups y exportación

Dos mecanismos distintos, porque cumplen objetivos distintos:

**a) Backup cifrado** (para restaurar dentro de la misma app):
- Copia consistente del archivo SQLCipher (checkpoint de WAL primero) + carpeta `documents/`
  cifrada + `vault.meta.json` (DEK envuelto) + `manifest.json` con versión de app/esquema,
  timestamp y SHA-256 de cada archivo incluido.
- Verificación de integridad: al restaurar, se recalculan los SHA-256 contra el manifest antes
  de tocar el vault activo; el vault actual se mueve a un lugar seguro (no se borra) hasta
  confirmar que la restauración abrió correctamente.
- Automático: opcional, configurable (frecuencia, carpeta destino).
- Limitación: si se cambia la contraseña maestra y luego se restaura un backup muy antiguo, ese
  backup fue envuelto con la contraseña vigente en ese momento, no la actual.

**b) Exportación abierta** (para migrar fuera de la app):
- Descifra todo y produce una carpeta con nombres reales: fichas en PDF/Markdown, pagos en CSV,
  documentos con su nombre original, y un JSON completo.
- Exportación individual por paciente: PDF (resumen clínico), Markdown, CSV donde aplique.
- Advertencia explícita en la UI: esto es texto plano en disco, ya no protegido por esta
  arquitectura.

---

## 10. Threat model

| # | Amenaza | Impacto | Mitigación | Limitación residual |
|---|---|---|---|---|
| 1 | Robo del computador | Acceso potencial a toda la información clínica | Vault cifrado con SQLCipher (DEK protegido por contraseña/recuperación), bloqueo automático | Si estaba desbloqueada al momento del robo y sin auto-lock activado, hay ventana de exposición |
| 2 | Acceso físico de otra persona (no robo) | Lectura de información sin autorización | Bloqueo automático por inactividad, requerir contraseña al reabrir | Nada impide que alguien mire por sobre el hombro mientras está desbloqueada |
| 3 | Copia de la base de datos | El atacante tiene el archivo pero cifrado | AES-256 vía SQLCipher, clave derivada con Argon2id | Fuerza bruta offline sigue siendo teóricamente posible si la contraseña es débil |
| 4 | Robo de backups | Igual que el punto 3 | Backups contienen los mismos artefactos cifrados, nunca en plano | Si además existe una "exportación abierta" en el mismo medio robado, esa sí está en plano |
| 5 | Exposición accidental de archivos | Un documento clínico visible sin querer | Documentos individuales cifrados en disco, nombres de archivo anónimos (UUID) | Si se exporta y se guarda en una carpeta insegura, la app no puede protegerlo después |
| 6 | Logs con información clínica | Un log de errores podría filtrar contenido de notas | Logs solo registran eventos técnicos, nunca contenido clínico; sin telemetría externa | Requiere disciplina de code review continua |
| 7 | Tokens de Google comprometidos | Acceso a la agenda de Google Calendar (no a datos clínicos) | Tokens en keychain del SO, alcance mínimo de scopes | Si el propio SO/cuenta está comprometido, el keychain también lo está |
| 8 | Exportaciones | Una "exportación abierta" queda en texto plano en el destino elegido | Advertencia explícita en UI al exportar | No hay forma de proteger un archivo después de que sale del control de la app |
| 9 | Malware local (keylogger, RAT) | Podría capturar la contraseña o leer memoria del proceso desbloqueado | Fuera del alcance que una app de escritorio puede controlar por sí sola | No se puede garantizar protección contra malware con privilegios en la misma máquina |
| 10 | Error humano | Pérdida de datos o de acceso | Soft delete en toda entidad clínica, papelera recuperable, código de recuperación | Si además se pierde el código de recuperación, la pérdida es definitiva |

No se promete seguridad absoluta en ningún punto.

---

## 11. Plan de implementación (Fase 1, técnico)

| Paso | Qué se construye | Depende de | Cómo se prueba | Criterio de "terminado" |
|---|---|---|---|---|
| 1.1 | Scaffold Tauri + React + TS + Tailwind, build local en macOS (y Windows si es posible probarlo) | — | `npm run tauri dev` levanta ventana vacía | App compila y abre en ambas plataformas objetivo disponibles |
| 1.2 | Integración `rusqlite` + SQLCipher (bundled-sqlcipher-vendored-openssl) | 1.1 | Test Rust: abrir archivo con clave, verificar `PRAGMA cipher_version` | Se puede crear/abrir/cerrar un archivo cifrado desde Rust |
| 1.3 | Esquema inicial completo + migraciones (`rusqlite_migration`) | 1.2 | Test de integración: correr migraciones sobre DB nueva, verificar tablas | Todas las tablas de la sección 4 existen tras migrar |
| 1.4 | Módulo de seguridad: Argon2id, envelope encryption, `vault.meta.json`, flujo "crear vault" y "desbloquear" | 1.3 | Test: crear vault con contraseña, cerrar app, reabrir y desbloquear; probar contraseña incorrecta; probar código de recuperación | Ciclo completo crear→bloquear→desbloquear funciona con datos reales |
| 1.5 | Capas repositories/services/commands en Rust, piloto vertical completo con Pacientes | 1.4 | Tests unitarios de repositorio y servicio; test de comando Tauri end-to-end | CRUD (con soft delete) de paciente persiste realmente, cifrado, y sobrevive un reinicio de la app |
| 1.6 | Cliente IPC tipado en TS + Zod espejo + Zustand store base | 1.5 | Prueba manual: crear paciente desde la UI, verlo listado tras recargar | La UI puede crear y listar un paciente real sin datos de mentira |
| 1.7 | Shell de UI: routing, layout, tema, pantalla de bloqueo/desbloqueo | 1.6 | Prueba manual de navegación | Se navega entre secciones vacías, pantalla de lock funcional |
| 1.8 | Suite de pruebas y validación cruzada mac/Windows | 1.1–1.7 | `cargo test` + smoke test manual en ambas plataformas | Todo lo anterior verde en al menos macOS; Windows si el entorno lo permite en esta fase |

> **Nota (31 de agosto de 2026) — el orden real de ejecución divergió de esta tabla a partir de
> 1.5, y eso es intencional, no un error.** Esta tabla es el plan *original*, escrito antes de
> empezar a implementar, y se conserva tal cual por valor histórico. En la práctica:
>
> - El "Paso 1.6" de esta tabla (cliente IPC tipado + Zod espejo) **se implementó dentro de la
>   Fase 1.5**, no como paso separado — el vertical de Pacientes se construyó de punta a punta
>   (SQLCipher → Repository → Service → Command → IPC tipado → React → UI) en un solo commit, en
>   vez de partir el cliente IPC de su primer caso de uso real. Zustand, en cambio, sigue sin
>   instalarse: no ha aparecido todavía un estado de UI efímero que lo justifique (ver sección 17,
>   fila de la Fase 1.6).
> - Lo que efectivamente se ejecutó y se aprobó como **"Fase 1.6" (31 de agosto de 2026)** fue
>   distinto: cerrar la única brecha que quedó pendiente de la Fase 1.5 (la papelera de pacientes
>   archivados, con restauración real desde la interfaz). Ver el detalle completo en
>   `docs/patients-vertical.md`, sección "Fase 1.6", y el resumen en la sección 17 de este
>   documento.
> - La Fase 1.7 real (sistema visual, design tokens y consolidación de la UI, ver sección 17) sí
>   corresponde al "Paso 1.7" de esta tabla ("tema" incluido), así que a partir de aquí el plan y
>   la ejecución vuelven a coincidir en el nombre, aunque el contenido de 1.7 es más amplio de lo
>   que esta tabla original anticipaba (identidad visual completa, no solo el mecanismo de tema).
> - El "Paso 1.8" de esta tabla dice "Suite de pruebas y validación cruzada mac/Windows". La Fase
>   1.8 real (31 de agosto de 2026, ver sección 17 y `docs/fase-1-cierre.md`) coincide en el
>   espíritu (regresión completa, cierre de Fase 1) pero **no incluyó validación cruzada física en
>   macOS ni Windows**, porque no hay una máquina de esas plataformas disponible en este entorno de
>   desarrollo — sería deshonesto declarar "probado en Windows" sin haberlo hecho. En su lugar, 1.8
>   fue una auditoría técnica completa de arquitectura/seguridad/regresión sobre Linux, dejando
>   constancia explícita de que la validación física en Mac/Windows/iOS/iPadOS sigue pendiente para
>   cuando exista acceso a esas máquinas.
>
> En resumen: el número de fase en la sección 17 ("Estado de avance") es siempre la fuente de
> verdad sobre qué se implementó y cuándo — esta tabla documenta únicamente la intención original
> de agosto de 2026, no el historial real de ejecución.

**Definición de "Fase 1 terminada":** se puede abrir la app, crear un vault con contraseña real,
cerrarla, reabrirla, desbloquearla, crear/editar/eliminar (soft) un paciente real que persiste
cifrado en disco, y toda la suite de tests automatizados pasa. Sin datos de mentira sustituyendo
funcionalidad.

Las Fases 2–8 mantienen el alcance definido originalmente; no se replanifican aquí.

---

## 12. Decisiones confirmadas (30 de agosto de 2026)

Revisión completa de la Fase 1 aprobada. Estado de cada punto:

1. **Separación de base de datos — confirmado.** Una sola base SQLCipher. Separación lógica y
   física mediante tablas dedicadas (`patient_clinical_profile` separada de `patients`, etc.) y
   frontera dura en el código Rust (servicios administrativos que no importan los servicios
   clínicos). No se crean dos bases de datos.
2. **Biometría — diferida a Fase 7, confirmado.** MVP con contraseña maestra + código de
   recuperación + bloqueo automático real. El keychain/Credential Manager del SO se usa
   **solo como conveniencia cuando sea apropiado y seguro** (ver regla explícita abajo) —
   nunca implica dejar la base de datos permanentemente desbloqueada. Bloquear la app siempre
   cierra la conexión cifrada y exige autenticación real para reabrirla; "recordar sesión" no
   puede degradar esa garantía.
3. **Sincronización Google Calendar — unidireccional confirmada.** Cuaderno Clínico → Google
   Calendar únicamente, en ambos sentidos de la Fase 3. La aplicación local es siempre la fuente
   de verdad; un cambio o eliminación hecho directamente en Google Calendar **nunca** modifica o
   elimina una cita o información clínica local — a lo sumo, la próxima sincronización marca esa
   cita como "sin vínculo con Google" para revisión manual (ver sección 6).
4. **Google OAuth — confirmado.** La usuaria creará las credenciales (proyecto, Calendar API,
   Client ID) en Google Cloud Console al llegar a la Fase 3. No se inventan ni simulan Client
   ID, Client Secret ni tokens en ningún momento.
5. **Notas de sesión — versionado activo desde el MVP, confirmado.** Ciclo
   **Borrador → Cerrada** con autoguardado en borrador, inmutabilidad real al cerrar, nueva
   versión (no sobrescritura) al modificar una nota cerrada, e historial de versiones
   consultable. Esquema y comportamiento detallados en la sección 4 (tabla `session_notes`).
6. **Backup automático — confirmado, configurable.** Activado por defecto, frecuencia diaria,
   ejecutado en un momento seguro (al cerrar la app) sin interferir con el uso. Requisitos de
   producto para la Fase 7 (backup): botón de backup manual; selector de carpeta de destino;
   fecha del último backup exitoso visible; advertencia si ha pasado demasiado tiempo sin
   backup exitoso; verificación de integridad (sección 9); nunca sobrescribir silenciosamente el
   único backup existente; **rotación de backups automáticos, conservando los últimos 7**.
7. **FileVault/BitLocker — confirmado, se agrega a Fase 7.** Chequeo del estado de cifrado de
   disco del sistema operativo, mostrado como recomendación si está desactivado. Es
   exclusivamente informativo: **nunca bloquea el uso de la aplicación**.
8. **Librerías Rust — dirección aceptada, con verificación de compatibilidad antes de fijar
   versiones.** Antes de agregar cada dependencia a `Cargo.toml` en la fase que la necesite
   (`rusqlite`+SQLCipher en 1.2, `rusqlite_migration` en 1.3, `argon2`/`aes-gcm`/`aes-kw`/
   `zeroize` en 1.4, `reqwest`/`keyring` en Fase 3, etc.) se valida que compila y funciona junto
   al resto del árbol de dependencias ya presente, no solo que "debería funcionar" en teoría. Se
   mantiene el número de dependencias al mínimo necesario; si alguna presenta problemas de
   mantenimiento, compatibilidad o seguridad al momento de agregarla, se reporta y se propone
   una alternativa antes de incorporarla — nunca se agrega "porque podría servir".
9. **Pérdida de contraseña y código de recuperación — aceptado explícitamente.** Sin ningún otro
   mecanismo legítimo de recuperación disponible, los datos son irrecuperables por diseño. No
   habrá puerta trasera.

---

## 13. Reglas adicionales de producto (incorporadas el 30 de agosto de 2026)

### A. Minimización de exposición de datos dentro de la propia aplicación

Aunque todo esté cifrado en reposo, se aplica mínima exposición también en tiempo de uso:

- No mostrar el RUT completo en listados cuando no sea necesario (ej. mostrar solo los últimos
  dígitos u ocultarlo tras un clic).
- No mostrar información clínica sensible (diagnóstico, contenido de notas) en el dashboard.
- Los títulos de ventana del sistema operativo nunca incluyen diagnóstico ni datos clínicos
  (el título de la ventana Tauri es fijo: "Cuaderno Clínico").
- Ninguna notificación del sistema operativo incluye información clínica ni nombre de paciente
  en el cuerpo visible (a lo sumo, un texto genérico tipo "Tienes una sesión en 15 minutos").
- Los logs de la aplicación nunca incluyen nombres de pacientes ni contenido clínico —
  únicamente identificadores técnicos (UUID) y eventos (ver threat model, punto 6).
- Los nombres de archivo en el sistema operativo nunca incluyen nombres de pacientes (ver
  sección 7 — nombres `<uuid>.enc`).
- Google Calendar nunca recibe información identificable (ver sección 6).
- La búsqueda global (sección 8) devuelve solo el contexto mínimo para identificar un resultado
  (tipo + título + fragmento corto), nunca el contenido completo de una nota o evaluación.

Esta regla se aplica de forma transversal a cada feature que se construya desde la Fase 2 en
adelante; se revisa explícitamente en cada nueva pantalla que muestre datos de un paciente.

### B. Dashboard

Al abrir la aplicación (Fase 2), pantalla de inicio con tres bloques, sin sobrecargarla:

- **Hoy**: sesiones del día con hora, paciente y estado.
- **Pendientes**: sesiones sin nota cerrada, pagos pendientes, tareas clínicas registradas,
  documentos pendientes cuando corresponda.
- **Resumen**: pacientes activos, sesiones del mes, ingresos del mes.

> **Estado de implementación (actualizado en Fase 3, 31 de agosto de 2026):** los tres bloques
> existen visualmente. "Pacientes activos" (dentro de Resumen) y, desde la Fase 3, "Hoy" muestran
> datos reales — "Hoy" lista las citas activas del día vía `agendaApi.list()` con el rango de la
> fecha local, sin distinguir todavía sesión de bloqueo personal más que por el texto mostrado
> ("Bloqueo personal" cuando no hay paciente). "Pendientes", "Sesiones del mes" e "Ingresos del
> mes" siguen dependiendo de funcionalidades (Sesiones, Pagos) que todavía no existen como
> backend — se muestran como "Próximamente" de forma explícita, nunca con un número o dato
> inventado. Detalle de la Fase 2 en `docs/dashboard.md`; detalle de la Agenda y Google Calendar
> de la Fase 3 en `docs/google-calendar.md`.

### C. Ficha de paciente como centro del sistema

La ficha de un paciente es el punto de acceso rápido a: resumen, antecedentes, sesiones, notas,
formulación, objetivos, evaluaciones, documentos, pagos y línea temporal. Se diseña (Fase 2) como
un layout con navegación lateral/tabs dentro de la ficha, no como pantallas aisladas sin relación
entre sí.

> **Estado de implementación:** este layout ya existía desde la Fase 1.5 (`PatientDetailScreen`,
> con las 9 secciones como tabs reales, "Resumen" con contenido y el resto marcado
> "Próximamente"). La Fase 2 no lo modificó — se reutilizó tal cual, sin reconstruirlo.

### D. Creación de sesión — rápida, sin generación automática de contenido clínico

Flujo objetivo: **Nueva sesión → seleccionar paciente → fecha/hora → abrir nota**, con el mínimo
de clics. La aplicación puede recordar y sugerir automáticamente: la estructura de nota usada
anteriormente con ese paciente, los objetivos terapéuticos activos, e información relevante para
continuar la sesión — todo como **ayuda de organización**, nunca generando contenido clínico por
sí misma (esto también es coherente con la regla F: no IA generativa).

### E. Modo privacidad (arquitectura preparada, implementación no necesariamente en el MVP)

Un modo de interfaz que, al activarse, minimiza la información identificable visible en pantalla
(útil al compartir pantalla): oculta RUT, oculta datos de contacto, y opcionalmente muestra
iniciales en vez de nombres completos en listados. Se deja preparado como una preferencia global
de UI (`app_settings`, ej. `privacy_mode: boolean`) que los componentes de listado consultan al
renderizar — así activarlo/desactivarlo es una bandera transversal y no requiere tocar cada
pantalla si se implementa después del MVP.

Esta regla es un caso particular de un principio más amplio de **privacidad visual**: la
aplicación debe minimizar por diseño lo que es visible de reojo o en una captura de pantalla, no
solo lo que queda en reposo cifrado. La sección 14 (identidad visual y sistema de diseño) recoge
este principio como parte de las reglas de UX transversales, y la sección 16 lo consolida junto
con el resto de los principios de privacidad no negociables.

### F. Sin IA generativa por ahora

Ninguna nota clínica, documento, evaluación ni información de paciente se envía a un modelo de
IA externo ni local en esta versión. Si en el futuro se evalúa IA local, será un proyecto
separado y explícitamente autorizado — no se incorpora por defecto ni de forma incremental sin
esa autorización.

---

## 14. Identidad visual y sistema de diseño (incorporado el 31 de agosto de 2026)

### A. Principio: fuente única de verdad para lo visual

Todo valor visual (colores, espaciado, tipografía, radios, sombras) se define **una sola vez**
como *design tokens* (variables — CSS custom properties o el `theme` de Tailwind extendido) y los
componentes consumen esos tokens por nombre semántico (`bg-surface`, `text-accent`,
`border-default`), nunca un color de la paleta por defecto de Tailwind escrito directamente
(`bg-slate-900`, `text-red-600`, `bg-emerald-500`, etc.) ni un valor hexadecimal suelto dentro de
un componente. La razón no es estética: si el color de marca, el contraste de accesibilidad o un
futuro tema oscuro cambian, el cambio debe hacerse en un solo lugar y propagarse solo, en vez de
perseguir cada clase escrita a mano en cada componente.

- **Color de acento**: `#2D5128` (verde oscuro). Se define como token (`--color-accent` /
  `accent` en el tema de Tailwind) y se usa para elementos de marca, estados activos/seleccionados
  y llamados a la acción primarios — no como color de fondo masivo.
- **Categorías de tokens necesarias** (a definir en detalle en la Fase 1.7, "tema", según el plan
  de la sección 11): color (fondo, superficie, borde, texto primario/secundario, acento, estados
  semánticos éxito/advertencia/error), espaciado, radios, escala tipográfica, sombras.
- **Preparación para modo oscuro**: los tokens se estructuran para que un futuro tema oscuro sea
  un *reemplazo de valores de los mismos tokens*, no una reescritura de componentes. No implica
  implementar modo oscuro ahora, solo no cerrar esa puerta con nombres de clase hardcodeados.

### B. Principios de UX

Consistentes con las reglas de producto ya aprobadas (sección 13, especialmente 13.C y 13.D):

- **Claridad sobre decoración**: sin animaciones, transiciones o elementos visuales que no
  aporten a entender o hacer algo más rápido.
- **Comodidad en uso prolongado**: la aplicación se usa varias horas seguidas en un día clínico
  (sección tras sección); tipografía legible, espaciado generoso, sin fatiga visual por contraste
  excesivo o densidad innecesaria.
- **Profesional y sobrio**: paleta reducida, sin colores saturados fuera del acento y los estados
  semánticos.
- **Consistencia entre pantallas**: el mismo componente se ve y se comporta igual en toda la app
  — otra razón para que los tokens sean centrales y no por-componente.

### C. Privacidad visual

Ver sección 13.E (Modo privacidad) y la ampliación de ese principio en la sección 16. La
identidad visual y la privacidad visual comparten mecanismo: ambas dependen de que el estado
"qué se muestra en pantalla" sea gobernable de forma centralizada (tokens de diseño +
`privacy_mode` en `app_settings`), no decisiones sueltas repetidas en cada pantalla.

### D. Afecta la arquitectura actual — contradicción detectada y resuelta en la Fase 1.7

Esto **sí afecta el frontend ya implementado**, aunque no toca Rust, la base de datos ni el
módulo de seguridad.

> **Contradicción detectada el 31 de agosto de 2026, resuelta en la Fase 1.7 (mismo día).** Los
> componentes de React construidos en las Fases 1.1–1.5 (`src/components/ui/*`,
> `src/features/auth/*`, `src/features/patients/*`, `src/app/Layout.tsx`, `src/App.tsx`) usaban
> clases de la paleta por defecto de Tailwind escritas directamente componente por componente
> (`bg-slate-900`, `bg-slate-50`, `text-red-600`, `bg-emerald-500`, etc.), sin una capa de tokens
> centralizada ni el color de acento `#2D5128` — contradiciendo el principio A de esta sección.
> Cuando se detectó, se dejó constancia explícita sin corregirla de inmediato, reservando la
> migración para la Fase 1.7 ("Shell de UI: routing, layout, **tema**"), que era el momento
> natural para hacerlo de una vez y no ad hoc. Esa fase ya se ejecutó: los tokens se definieron en
> `src/index.css` (bloque `@theme` de Tailwind v4) y **todos** los componentes listados arriba se
> migraron a consumirlos — ya no queda ninguna clase de paleta por defecto de Tailwind en `src/`.
> Detalle completo (tabla de tokens, contraste WCAG verificado, capturas de la aplicación real) en
> `docs/design-tokens.md`.

---

## 15. Objetivo de multiplataforma y arquitectura de sincronización futura (incorporado el 31 de agosto de 2026 — fuera de alcance de Fase 1)

Todo lo descrito en esta sección es un **objetivo de producto a futuro**, documentado ahora para
que las decisiones de Fase 1 no lo bloqueen accidentalmente. **Nada de esta sección se
implementa en Fase 1, ni siquiera parcialmente.** No hay backend, no hay nube, no hay sync, no
hay build de iOS/iPadOS, no hay E2EE multi-dispositivo — todo eso queda para cuando se apruebe
explícitamente una fase futura dedicada.

### A. Multiplataforma

- **Hoy**: macOS y Windows, ambos de primera clase (ver sección 2, fila "Framework de
  escritorio", y el ítem f agregado a "Cuestionamientos importantes"). **Windows no es un
  soporte secundario de un producto pensado para Apple** — cualquier decisión de diseño o de
  almacenamiento debe funcionar igual de bien en ambos sistemas operativos.
- **Futuro**: extender a iPhone/iPad. Tauri ya soporta objetivos móviles sobre el mismo core en
  Rust, por lo que esta meta es compatible con la arquitectura elegida en 1.1 sin requerir
  reescribirla.
- **Un dispositivo Apple no implica iCloud.** Tener build en iOS/iPadOS no significa que la
  sincronización (ver punto B) use iCloud por defecto, ni que se acople a servicios exclusivos de
  Apple de forma que Windows quede en desventaja. Cualquier mecanismo de sincronización futuro
  debe diseñarse para funcionar igual entre macOS, Windows, iOS/iPadOS — nunca como "Apple
  primero, Windows si alcanza".

### B. Sincronización entre dispositivos (futura, no diseñada en detalle todavía)

Mandato de diseño, no diseño cerrado: **cualquier sincronización futura debe ser local-first y
end-to-end encrypted (E2EE)**, de forma que ningún servidor, relay o proveedor de nube
intermedio — sea propio o de terceros (iCloud, Google Drive, un relay propio, etc.) — pueda leer
el contenido clínico en tránsito ni en reposo del lado del servidor. El servidor, si existe, es
en el mejor de los casos un intermediario ciego de bytes cifrados.

Antes de implementar sincronización en cualquier forma, deben responderse explícitamente (no se
responden en este documento, quedan como lista de preguntas abiertas para cuando se apruebe esa
fase):

1. ¿Qué transporte se usa? (relay propio mínimo, servicio de terceros ya cifrado en tránsito, o
   sincronización de archivo crudo vía un proveedor de almacenamiento que la usuaria ya controle).
2. ¿Cómo se emparejan dispositivos nuevos sin exponer el DEK ni las contraseñas en el proceso?
3. ¿Cómo se comparte o deriva la clave entre dispositivos autorizados sin debilitar la seguridad
   de un solo dispositivo (ver sección 5)?
4. ¿Qué pasa con el rendimiento y el costo de almacenamiento si se sincroniza también el vault de
   documentos, no solo la base estructurada?
5. ¿Cómo se comporta la app sin conexión (local-first implica que la ausencia de red nunca
   bloquea el trabajo clínico normal)?

### C. Dispositivos autorizados y revocación (concepto futuro, no diseñado en detalle)

Se reserva conceptualmente la idea de una lista de "dispositivos autorizados" por vault, con la
capacidad de **revocar** el acceso de un dispositivo específico (por ejemplo, si se pierde o se
vende). Consistente con el patrón de envelope encryption ya implementado en la sección 5 (cada
KEK envuelve el mismo DEK independientemente): un diseño futuro razonable es que cada dispositivo
autorizado tenga su propia envoltura del DEK, de modo que revocar un dispositivo sea invalidar
solo su envoltura — sin tener que rotar contraseñas de otros dispositivos. Esto es una dirección
de diseño compatible con lo ya construido, no una decisión tomada; el diseño concreto se define
cuando se aborde esa fase.

### D. Resolución de conflictos — prohibición explícita de "last-write-wins" silencioso

Ninguna sincronización futura puede resolver un conflicto sobre una nota clínica sobrescribiendo
silenciosamente una versión con otra ("el último que sincronizó gana"). Esto ya es compatible con
lo implementado: las notas de sesión (sección 4, tabla `session_notes`) son append-only por
versión desde la Fase 1.4/1.5 (`version`, `is_current`, `superseded_at`) precisamente porque
nunca se sobrescribe una versión — se crea una nueva y se preserva el historial completo. Un
futuro mecanismo de sincronización debe extender ese mismo principio entre dispositivos: ante un
conflicto real, ambas versiones se preservan y la usuaria decide, o el conflicto se expone
explícitamente en la UI para revisión manual — nunca una resolución automática invisible.

### E. Backup, Sync y Export — tres sistemas conceptualmente distintos

La sección 9 ya distingue Backup y Exportación con propósitos distintos; se agrega Sync como un
tercer sistema, y se deja explícito que **los tres nunca se combinan ni se confunden entre sí**,
ni en el diseño ni en la interfaz:

| Sistema | Propósito | Alcance | Estado |
|---|---|---|---|
| **Backup** | Recuperar el propio vault ante pérdida/corrupción/error | Un solo vault, restaurado en el mismo dispositivo o uno de reemplazo | Implementado conceptualmente en la sección 9, pendiente de construir en Fase 7 |
| **Sync** | Mantener el mismo vault consistente entre varios dispositivos autorizados de la misma usuaria | Múltiples dispositivos, mismo vault lógico, E2EE | Futuro, fuera de alcance de Fase 1, ver puntos B–D |
| **Export** | Extraer datos deliberadamente en texto plano para uso fuera de la app (otro sistema, otro profesional, un trámite) | Un subconjunto de datos, ya no protegido por esta arquitectura | Implementado conceptualmente en la sección 9 |

### F. Explícitamente fuera de alcance de esta actualización y de la Fase 1 actual

No se escribe código nuevo, no se implementa sincronización, no se implementa backend/nube, no se
implementa build de iOS/iPadOS, no se implementa E2EE multi-dispositivo, y no se agrega ninguna
dependencia nueva a `Cargo.toml` ni a `package.json` en función de nada de esta sección. Nada de
lo anterior avanza la Fase 1.6.

**Verificación de compatibilidad con lo ya implementado (Fases 1.1–1.5):** no se encontró
contradicción entre este objetivo futuro y la arquitectura actual. Tauri (1.1), el vault único
SQLCipher con envelope encryption (1.2/1.4) y el versionado append-only de notas (1.3) son, de
hecho, una base de partida favorable para B, C y D respectivamente, sin haber sido diseñados
pensando en sync — es una coincidencia útil, no una casualidad forzada.

---

## 16. Principios de privacidad no negociables (consolidado el 31 de agosto de 2026)

Esta sección consolida en un solo lugar los principios de privacidad que ya estaban dispersos
(secciones 5, 6, 9, 13.A, 13.E) junto con los agregados en esta actualización, para que existan
como una lista única citable en vez de un criterio implícito repetido de memoria en cada
decisión futura:

1. Ningún dato clínico sale del dispositivo sin cifrado de extremo a extremo bajo control de la
   propia aplicación (ver sección 5; extendido a cualquier sincronización futura en la sección
   15.B).
2. Ningún proveedor de sincronización, backup en la nube o infraestructura intermedia puede leer
   contenido clínico — ni siquiera el proveedor de la infraestructura (iCloud, Google, un relay
   propio, etc.).
3. No existe puerta trasera, ni maestra ni de soporte técnico, para acceder a un vault sin la
   contraseña o el código de recuperación de la usuaria (ver sección 5, punto 8).
4. Los dispositivos Apple no implican sincronización vía iCloud por defecto — es una decisión
   explícita, nunca una consecuencia automática de la plataforma (ver sección 15.A).
5. Windows es un ciudadano de primera clase de esta aplicación, no un soporte secundario respecto
   de macOS/iOS (ver sección 2 y 15.A).
6. Ninguna notificación del sistema operativo expone contenido clínico ni un nombre de paciente
   identificable en el cuerpo visible, en ninguna plataforma. Ejemplo permitido: *"Tienes una
   sesión en 15 minutos."* Ejemplo prohibido: *"Sesión con Juan Pérez sobre ideación suicida en 15
   minutos."* (Regla ya establecida en 13.A; se reafirma aquí como no negociable y válida también
   para cualquier notificación futura relacionada con sincronización, ej. "Sync completado" sí,
   "3 notas de Juan Pérez sincronizadas" no).
7. Ninguna resolución de conflicto sobre datos clínicos es automática o silenciosa
   ("last-write-wins"); la usuaria siempre puede ver y decidir (ver sección 15.D).
8. Backup, Sync y Export son sistemas distintos con propósitos distintos y nunca se combinan ni
   se presentan como intercambiables en la implementación ni en la interfaz (ver sección 15.E).
9. Cualquier tecnología, dependencia o cambio de arquitectura que toque estos nueve principios
   requiere detenerse y pedir aprobación explícita antes de implementarse — coherente con la regla
   de proceso ya vigente desde el inicio del proyecto para cualquier decisión de arquitectura.

Estos principios no reemplazan el threat model de la sección 10; lo complementan desde el ángulo
de producto/privacidad en vez del ángulo de amenaza técnica.

---

## 17. Estado de avance

> **Nota (31 de agosto de 2026):** las reglas permanentes de proceso para todo el proyecto quedaron
> fijadas en `CLAUDE.md` (estabilidad y no retroceso, migraciones no destructivas, datos de
> desarrollo siempre ficticios, principios de seguridad no negociables, Backup≠Sync≠Export,
> multiplataforma futura, identidad visual, regresión obligatoria, informe de cierre de fase, y
> regla de detenerse). La Fase 1.6 se ejecutó bajo esas reglas — ver detalle en
> `docs/patients-vertical.md`, sección "Fase 1.6".

| Fase | Estado | Verificado |
|---|---|---|
| **1.1** — Scaffold Tauri + React + TS + Tailwind | ✅ Completada | `cargo check`, `cargo test`, `cargo clippy`, `npm run build`, `npm run lint` en verde; build release (`tauri build --no-bundle`) enlaza correctamente; app ejecutada bajo Xvfb con captura de pantalla confirmando que la ventana renderiza y el comando Rust `app_info` responde por IPC |
| **1.2** — SQLite + SQLCipher | ✅ Completada | `rusqlite` 0.40.2 + `libsqlite3-sys` 0.38.2 (SQLCipher 4.14.0, SQLite 3.51.3) con `bundled-sqlcipher-vendored-openssl` (OpenSSL 3.6.3 vendorizado); 11/11 tests en verde cubriendo creación, cierre/reapertura, rechazo de clave incorrecta, rechazo de archivo corrupto, verificación de `PRAGMA cipher_version`, e inspección de bytes en disco confirmando ausencia del encabezado plano de SQLite. Detalle completo en `docs/sqlcipher.md` |
| **1.3** — Migraciones y esquema completo | ✅ Completada | `rusqlite_migration` 2.6.0 (una sola migración V1 con las 25 tablas); 29/29 tests en verde (11 de la 1.2 + 18 nuevos) cubriendo creación desde cero, foreign keys, índices/CHECK, un caso de datos relacionados de punta a punta en todos los dominios, rechazo de estados inválidos, verificación de que el esquema funciona sobre SQLCipher y no sobre SQLite plano, y que reaplicar o agregar migraciones no destruye datos existentes. Detalle completo, diferencias respecto al esquema original y decisiones pendientes en `docs/db-schema.md` |
| **1.4** — Seguridad (Argon2id, envelope encryption, sesión) | ✅ Completada | DEK de 256 bits + Argon2id (RFC 9106) + AES-256-GCM (RustCrypto) para envolver/desenvolver, código de recuperación de 120 bits, cambio de contraseña y recuperación sin re-cifrar la base, bloqueo manual y automático por inactividad con imposibilidad estructural de leer datos bloqueada. 87/87 tests en verde (11+18 de fases previas sin cambios + 58 nuevos) más verificación manual de extremo a extremo sobre la aplicación real compilada (Xvfb + interacción real de mouse/teclado). Primera UI funcional: crear/confirmar código/desbloquear/recuperar/cambiar contraseña/bloquear. Detalle completo en `docs/security.md` |
| **1.5** — Vertical Pacientes (repositories/services/commands) | ✅ Completada | Capa completa SQLCipher → Repository → Service → Tauri Command → IPC tipado → React, con validación autoritativa en Rust (RUT chileno con dígito verificador, estado, fechas) y minimización de exposición estructural (el listado no puede llevar RUT porque el tipo no lo tiene). Primeras pantallas de datos clínicos reales: listado con buscador contra la base, crear/editar con formulario dividido en secciones, ficha con navegación a las 9 secciones futuras. Router real (react-router-dom) y atajo ⌘/Ctrl+N ya funcionando. 111/111 tests en verde (87 previos + 24 nuevos) más verificación manual de extremo a extremo (crear → cerrar la app → reabrir → desbloquear → paciente persistido → editar → archivar). Detalle completo en `docs/patients-vertical.md` |
| **1.6** — Conexión real frontend↔backend para Pacientes (papelera de archivados) | ✅ Completada | El cliente IPC tipado y los esquemas Zod ya existían desde la Fase 1.5 (Zustand sigue sin usarse por diseño: no hay todavía estado de UI efímero que lo justifique). Esta fase cerró la única brecha pendiente de 1.5: vista de pacientes archivados con restauración real desde la interfaz, sobre capacidades de backend (`restore_patient`) que ya existían y ya tenían tests desde la Fase 1.5. 114/114 tests en verde (111 previos sin cambios + 3 nuevos) más verificación manual de extremo a extremo (crear → archivar → ver en "Archivados" → restaurar → cerrar la app → reabrir → desbloquear → persistencia confirmada). Sin cambios de esquema ni dependencias nuevas. Detalle completo en `docs/patients-vertical.md`, sección "Fase 1.6" |
| **1.7** — Sistema visual, design tokens y consolidación de la UI | ✅ Completada | Tokens definidos en `src/index.css` (`@theme` de Tailwind v4): background/surface/surface-elevated/foreground/muted-foreground/border/accent(+hover/active/soft)/success/warning/danger/focus/disabled, con `#2D5128` como acento único. Los 17 archivos de `src/` que tenían clases de la paleta por defecto de Tailwind (`slate-*`, `emerald-*`, `red-*`, `amber-*`) se migraron a los tokens — cero clases de paleta por defecto restantes en todo `src/`. Contraste WCAG verificado matemáticamente para cada combinación texto/fondo (mínimo 4.5:1, la mayoría AAA). Anillo de foco visible globalmente vía `:focus-visible`. Sin cambios en Rust, sin cambios de esquema, sin dependencias nuevas. 114/114 tests Rust sin cambios, build/lint frontend limpios, `cargo clippy` sin advertencias, más verificación manual completa sobre la aplicación real compilada (bloqueo/desbloqueo, creación de vault, código de recuperación, pacientes activos/archivados, ficha de paciente, ciclo archivar→restaurar) con capturas de pantalla. Detalle completo en `docs/design-tokens.md` |
| **1.8** — Cierre técnico de Fase 1, regresión y auditoría | ✅ Completada | Auditoría completa de arquitectura/seguridad/regresión sin implementar funcionalidad nueva: separación React/Rust, envelope encryption, zeroización (verificada leyendo las 3 implementaciones que redactan `Debug`/zeroizan en `Drop`), soft delete, migraciones (confirmado por `git log` que `SCHEMA_V1` no cambió un carácter desde la Fase 1.3), design tokens, CSP, ausencia de `localStorage`/SQL-en-React/comandos genéricos, y una inspección directa en disco de las carpetas de caché del WebView real confirmando que ningún dato clínico quedó persistido ahí. 114/114 tests Rust sin cambios, `cargo clippy` sin advertencias, build/lint frontend limpios, y verificación manual completa (crear→editar→archivar→restaurar→cerrar proceso→reabrir) sobre un **vault de prueba separado** (no se tocó ningún vault existente). Modo WAL investigado y confirmado como diferido a Fase 7, con justificación (ver `docs/db-schema.md`). **Validación física en macOS/Windows/iOS/iPadOS sigue sin realizarse** — no hay esas máquinas disponibles en este entorno; esto se declara explícitamente, no se asume resuelto. Ninguna decisión arquitectónica aprobada se modificó. Detalle completo en `docs/fase-1-cierre.md` |

**Fase 1 (1.1–1.8) cerrada.** La aplicación crea/desbloquea/bloquea un vault real cifrado con
SQLCipher + envelope encryption, gestiona pacientes de punta a punta (crear/editar/archivar/
restaurar) con persistencia real verificada a través de reinicios completos del proceso, y tiene
una identidad visual consistente en toda la interfaz. Sin deuda técnica crítica ni importante
pendiente; los pendientes menores (modo WAL, validación física multiplataforma) están
documentados explícitamente, no ocultos.

| **2** — Dashboard como pantalla de inicio | ✅ Completada | `/` pasa a ser el Dashboard (sección 13.B); `/patients` recibe el listado que antes vivía en `/`. Del Dashboard, solo "Pacientes activos" muestra un dato real (vía `patientsApi.list()`, sin backend nuevo); "Hoy", "Pendientes", "Sesiones del mes" e "Ingresos del mes" se muestran explícitamente como "Próximamente" — ningún número inventado. Nav "Inicio"/"Pacientes" agregado al header. Ficha de paciente (sección 13.C) reutilizada sin modificar, ya existía desde la Fase 1.5. Cero cambios en Rust, cero migraciones, cero dependencias nuevas. 114/114 tests Rust sin cambios, build/lint frontend limpios, `cargo clippy` sin advertencias, más verificación manual completa (Dashboard sin pacientes → con paciente ficticio → click a `/patients` → archivar → conteo actualizado → restaurar → cierre completo del proceso → reapertura → desbloquear → aterriza en Dashboard con el conteo persistido) sobre un **vault de prueba separado**. Detalle completo en `docs/dashboard.md` |
| **3** — Agenda local y Google Calendar (sync unidireccional) | ✅ Completada | Agenda local completa (CRUD de citas, cancelar/archivar/restaurar, advertencia de solapamiento sin revelar el paciente en conflicto, citas sin paciente como "bloqueo personal") sobre la tabla `appointments` ya existente desde la Fase 1.3 — sin migraciones nuevas. Integración OAuth (PKCE) con Google Calendar, minimizada por construcción: el evento espejo solo lleva un resumen genérico ("Sesión clínica"/"Bloqueo personal") y las dos marcas de tiempo — nunca nombre, RUT, diagnóstico, modalidad ni notas — verificado con `event_payload_never_contains_anything_beyond_the_generic_summary_and_the_two_timestamps`. La cita local siempre se guarda primero e independiente; la sincronización es *best-effort* y nunca puede revertir ni alterar el estado local (tabla de contrato completa en `docs/google-calendar.md`). Refresh token en el keychain del SO vía el crate `keyring`. Pantalla `/settings` para configurar credenciales OAuth y conectar/desconectar. 157/157 tests Rust en verde (114 previos sin cambios + 43 nuevos), `cargo clippy` sin advertencias, build/lint frontend limpios, más verificación manual sobre la aplicación real. Backend de keychain real y login OAuth completo contra Google no pudieron ejercitarse de punta a punta en este entorno headless sin D-Bus/Secret Service ni navegador interactivo — declarado explícitamente, no asumido resuelto. Detalle completo en `docs/google-calendar.md` |
| **4** — Sesiones clínicas y notas versionadas | ✅ Completada | Segundo vertical funcional completo (repository → service → command → IPC → React) sobre las tablas `sessions`/`session_notes`, ya existentes desde la Fase 1.3 — sin migraciones nuevas. Notas clínicas con versionado *append-only* real: una nota cerrada nunca se modifica con un UPDATE de contenido — editarla siempre crea una versión nueva, garantizado en tres niveles independientes (SQL estructural con `WHERE is_locked = 0`, índice único parcial de SQLite que impide dos versiones vigentes simultáneas, y orden de operaciones en el servicio dentro de una transacción atómica). Autoguardado de borradores, cierre con reglas de contenido no vacío, historial completo de versiones de solo lectura. Dos flujos de creación: desde la ficha del paciente y desde una cita de Agenda (con herencia de fecha/hora/modalidad y bloqueo de una segunda sesión por cita, incluso si la primera está archivada). Pestaña "Sesiones" de la ficha del paciente deja de mostrar "Próximamente". 200/200 tests Rust en verde (157 previos sin cambios + 43 nuevos), `cargo clippy` sin advertencias, build/lint frontend limpios, más verificación manual completa sobre la aplicación real compilada (creación de tres versiones consecutivas de una nota con verificación visual de que las versiones reemplazadas conservan su contenido original intacto, archivar/restaurar, ambos flujos de creación, persistencia a través de bloqueo/desbloqueo y de un cierre/reapertura completos del proceso, y auditoría de privacidad con un marcador ficticio confirmando cero fugas fuera del vault cifrado) sobre un **vault de prueba desechable**. Detalle completo en `docs/sessions.md` |
| **5** — Objetivos terapéuticos y vínculo con sesiones | ✅ Completada | Tercer vertical funcional completo sobre las tablas `therapeutic_goals`/`goal_indicators`/`session_goals`, ya existentes desde la Fase 1.3 — sin migraciones nuevas. A diferencia de `session_notes` (Fase 4), los objetivos son **registros mutables sin versionado** — decisión de producto deliberada. Estados `activo`/`logrado`/`pausado`/`descartado` validados en backend, con `logrado` explícitamente no terminal (cualquier transición es aceptada). Indicadores en texto libre (descripción, valor de partida, valor a alcanzar), sin cálculos ni métricas automáticas, opcionales al crear el objetivo. Vínculo N:M objetivo↔sesión (`session_goals`, con `progress_note` editable) con la regla de integridad crítica de que `session.patient_id == goal.patient_id` se verifica explícitamente en el servicio antes de crear cualquier vínculo — la FK no lo garantiza por sí sola — y con el selector de la UI ("Agregar objetivo" en `SessionDetailScreen`) estructuralmente limitado al paciente de esa sesión. Pestaña "Objetivos" de la ficha del paciente deja de mostrar "Próximamente". 247/247 tests Rust en verde (200 previos sin cambios + 47 nuevos), `cargo clippy` sin advertencias, build/lint frontend limpios, más verificación manual completa sobre la aplicación real compilada (ciclo completo de estados incluyendo `logrado → activo`, archivar/restaurar con indicadores y vínculos intactos, un objetivo en múltiples sesiones y una sesión con múltiples objetivos, persistencia a través de bloqueo/desbloqueo y de un cierre/reapertura completos del proceso, y auditoría de privacidad con un marcador ficticio confirmando cero fugas fuera del vault cifrado) sobre un **vault de prueba desechable**. Detalle completo en `docs/goals.md` |
| **6** — Antecedentes clínicos | ✅ Completada | Cuarto vertical funcional completo sobre la tabla `patient_clinical_profile`, ya existente desde la Fase 1.3 — sin migraciones nuevas. Registro **mutable simple, sin versionado ni historial** — misma decisión de producto deliberada que Objetivos (Fase 5), reforzada aquí explícitamente para no introducir una arquitectura de versionado no definida por `SCHEMA_V1`. Un único registro por paciente, garantizado por la propia `PRIMARY KEY` de la tabla (`patient_id`); creación y edición son operaciones separadas (no un upsert), reflejando directamente los estados de la UI. `risk_flags` se trata únicamente como JSON de sintaxis válida, sin catálogo ni taxonomía clínica de factores de riesgo — decisión explícita para no inventar semántica clínica no definida por el proyecto. Creación bloqueada para un paciente archivado (backend y UI); edición de un perfil ya existente permitida. Pestaña "Antecedentes" de la ficha del paciente deja de mostrar "Próximamente". 271/271 tests Rust en verde (247 previos sin cambios + 24 nuevos), `cargo clippy` sin advertencias, build/lint frontend limpios (14 warnings: 13 preexistentes de Fase 5, verificados empíricamente contra un worktree del commit de cierre de Fase 5, más 1 nuevo de la misma categoría preexistente — sin categoría nueva), más verificación manual completa sobre la aplicación real compilada (crear/editar/consultar antecedentes con los cinco campos, validación de JSON inválido en `risk_flags`, perfil completamente vacío, aislamiento entre dos pacientes de prueba, creación bloqueada en la UI para un paciente archivado, persistencia a través de bloqueo/desbloqueo y de un cierre/reapertura completos del proceso, y una segunda pasada de regresión funcional de Sesiones/Notas versionadas/Objetivos sobre un vault de prueba independiente) sobre **dos vaults de prueba desechables**, y auditoría de privacidad con un marcador ficticio confirmando cero fugas fuera del vault cifrado. Detalle completo en `docs/clinical-profile.md` |
| **6.1** — Ubicación geográfica y estadísticas de pacientes | ✅ Completada | Extensión pequeña y aislada de la vertical Pacientes (Fase 1.5): migración `V2` exclusivamente aditiva (`ALTER TABLE patients ADD COLUMN region TEXT` / `ADD COLUMN commune TEXT`, ambas nullable, sin `DEFAULT`, sin backfill; `SCHEMA_V1` intacto). Región/comuna se validan contra un catálogo cerrado de Chile (16 regiones, 346 comunas, fuente DEIS vía GitHub, con la Región de Ñuble re-derivada estructuralmente de sus provincias post-2018) más el valor reservado `"Extranjero"` (sin comuna); catálogo compartido byte a byte entre Rust (`geo.rs`, `include_str!`) y TypeScript (`features/patients/geo.ts`, import directo del mismo `src/data/chile-geo.json`) — una única fuente de verdad, no dos copias sincronizadas a mano. Nueva pantalla independiente "Estadísticas" (no dentro del Dashboard) con conteo con/sin ubicación y distribución por región (donut) y por comuna (barras horizontales), ambas en SVG/CSS nativo sin librería de gráficos, agregación siempre vía `GROUP BY` en el backend, filtro Activos(por defecto)/Todos, y categorías con menos de 3 pacientes agrupadas en "Otras" — nunca hay click-through desde un gráfico hacia un paciente individual. `PatientListItem` no se tocó (sigue sin RUT ni ubicación) — la minimización de exposición se extiende al nuevo campo. 302/302 tests Rust en verde (271 previos sin cambios + 31 nuevos), `cargo clippy` sin advertencias, build/lint frontend limpios (16 warnings: 15 preexistentes de fases anteriores + 1 nuevo de la misma categoría ya aceptada en el resto del código — sin categoría nueva), cero dependencias nuevas (`Cargo.toml`/`Cargo.lock`/`package.json`/`package-lock.json` sin diff), más verificación manual completa sobre la aplicación real compilada (creación/edición con región+comuna válidas e inválidas, valor "Extranjero", umbral de agrupación "Otras" en vivo con 2/3/5 pacientes por categoría, filtro Activos/Todos reaccionando a archivar/restaurar, persistencia a través de bloqueo/desbloqueo, regresión de Antecedentes/Dashboard/Agenda) sobre un **vault de prueba desechable**, y auditoría de privacidad confirmando cero datos geográficos en logs, Google Calendar (no tocado), o `localStorage`/`sessionStorage`. **Excepción mecánica de compatibilidad de tests, expresamente aprobada:** agregar los dos campos a `NewPatientRow`/`PatientInput` rompió la compilación de 11 helpers `#[cfg(test)]` en archivos de otras verticales (Sesiones, Objetivos, Antecedentes, Agenda, `security::session`) que construyen un paciente ficticio para sus propios tests — se corrigieron con exactamente `region: None, commune: None,` (24 líneas en total, cero lógica productiva tocada, ver detalle en `docs/geographic-stats.md`). Detalle completo en `docs/geographic-stats.md` |
| **7** — Pagos / Cobros internos | ✅ Completada | Quinto vertical funcional completo sobre la tabla `payments`, ya existente desde la Fase 1.3 — sin migraciones nuevas. **"Atrasado" es derivado, nunca se escribe automáticamente**: `status` permanece en `pendiente` hasta una acción manual; el vencimiento se calcula en el repositorio en tiempo de lectura (`(status='pendiente' AND due_date IS NOT NULL AND due_date < date('now')) AS is_overdue`, nunca persistido), con la limitación de huso horario UTC de `date('now')` documentada explícitamente en vez de ocultada. `amount == 0` es válido únicamente junto a `status = 'condonado'`; montos negativos siempre inválidos; sin tabla ni estado de "reembolso" propio. Monto en CLP forzado a entero por regla de **servicio** (no de esquema — `amount` sigue siendo `REAL`), sin librería monetaria nueva. Método de pago obligatorio solo al marcar `pagado`. Relación opcional con `sessions` (`ON DELETE SET NULL`, sin relación con `appointments`), con la regla de integridad `session.patient_id == payment.patient_id` verificada explícitamente en el servicio tanto al crear como al reasociar. Punto de entrada "Registrar pago" en `SessionDetailScreen` reutiliza el mismo formulario y las mismas reglas que la pestaña "Pagos" del paciente, sin selector de sesión ni segunda implementación de CRUD — solo precarga `sessionId` vía `location.state`. Dashboard con dos agregados reales calculados en SQL: "Ingresos del mes" (reemplaza el placeholder de la Fase 2) y la nueva fila "Pagos pendientes"; "Sesiones del mes" (vertical Sesiones) no se tocó. Paciente archivado: creación de pagos nuevos bloqueada en el backend (UI la oculta solo como refuerzo); pagos históricos siguen visibles y editables. Pestaña "Pagos" de la ficha del paciente deja de mostrar "Próximamente". 355/355 tests Rust en verde (302 previos sin cambios + 53 nuevos: 14 en `repositories::payments`, 39 en `services::payments`), `cargo clippy` sin advertencias, build/lint frontend limpios (19 warnings: 16 preexistentes de fases anteriores + 3 nuevos de las mismas dos categorías ya presentes en el resto del código — sin categoría nueva), cero dependencias nuevas, más verificación manual completa sobre la aplicación real compilada (condonado con monto > 0 y con monto = 0, rechazo de monto = 0 no condonado, pago vinculado a una sesión desde ambos puntos de entrada, edición con fecha de vencimiento pasada confirmando "Atrasado" en la vista sin alterar el `status` almacenado, archivar/restaurar un pago, paciente archivado bloqueando solo pagos nuevos, Dashboard con "Ingresos del mes" y "Pagos pendientes" verificados contra los pagos reales creados, persistencia a través de un ciclo real de recuperación de acceso con el código de recuperación genuino, y de un cierre/reapertura completos del proceso, y una segunda pasada de regresión funcional de Pacientes/Objetivos/Antecedentes/Estadísticas/Agenda) sobre un **vault de prueba desechable** (conservado bajo nombre de respaldo, nunca eliminado; vault real restaurado exactamente como estaba), y auditoría de privacidad con un marcador ficticio (`XYZFASE7PAGOS`) confirmando cero fugas fuera del vault cifrado. Detalle completo en `docs/payments.md` |
