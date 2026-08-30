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
   `zeroize`/`keyring` en 1.4, `reqwest` en Fase 3, etc.) se valida que compila y funciona junto
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

### C. Ficha de paciente como centro del sistema

La ficha de un paciente es el punto de acceso rápido a: resumen, antecedentes, sesiones, notas,
formulación, objetivos, evaluaciones, documentos, pagos y línea temporal. Se diseña (Fase 2) como
un layout con navegación lateral/tabs dentro de la ficha, no como pantallas aisladas sin relación
entre sí.

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

### F. Sin IA generativa por ahora

Ninguna nota clínica, documento, evaluación ni información de paciente se envía a un modelo de
IA externo ni local en esta versión. Si en el futuro se evalúa IA local, será un proyecto
separado y explícitamente autorizado — no se incorpora por defecto ni de forma incremental sin
esa autorización.

---

## 14. Estado de avance

| Fase | Estado | Verificado |
|---|---|---|
| **1.1** — Scaffold Tauri + React + TS + Tailwind | ✅ Completada | `cargo check`, `cargo test`, `cargo clippy`, `npm run build`, `npm run lint` en verde; build release (`tauri build --no-bundle`) enlaza correctamente; app ejecutada bajo Xvfb con captura de pantalla confirmando que la ventana renderiza y el comando Rust `app_info` responde por IPC |
| **1.2** — SQLite + SQLCipher | ✅ Completada | `rusqlite` 0.40.2 + `libsqlite3-sys` 0.38.2 (SQLCipher 4.14.0, SQLite 3.51.3) con `bundled-sqlcipher-vendored-openssl` (OpenSSL 3.6.3 vendorizado); 11/11 tests en verde cubriendo creación, cierre/reapertura, rechazo de clave incorrecta, rechazo de archivo corrupto, verificación de `PRAGMA cipher_version`, e inspección de bytes en disco confirmando ausencia del encabezado plano de SQLite. Detalle completo en `docs/sqlcipher.md` |
| 1.3 — Migraciones y esquema | Pendiente | — |
| 1.4 — Seguridad (Argon2id, envelope encryption) | Pendiente | — |
| 1.5 — Vertical Pacientes (repositories/services/commands) | Pendiente | — |
| 1.6 — Cliente IPC + Zod + Zustand | Pendiente | — |
| 1.7 — Shell de UI y pantalla de bloqueo | Pendiente | — |
| 1.8 — Suite de pruebas y validación cruzada | Pendiente | — |
