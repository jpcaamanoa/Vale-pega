# Plan Fase 16 — Documentos y adjuntos clínicos cifrados

**ESTADO: SOLO AUDITORÍA Y PLANIFICACIÓN. NADA DE ESTO ESTÁ IMPLEMENTADO.**

Documento autocontenido: incluye baseline, auditoría completa del código real (esquema,
filesystem, criptografía), threat model, alternativas evaluadas, arquitectura recomendada,
y las preguntas concretas que requieren aprobación antes de escribir una sola línea de código de
esta fase. Pensado para poder copiarse/descargarse íntegro y entregarse a un tercero (incluida
otra IA) para revisión, sin depender de ningún otro mensaje de esta conversación.

---

## 1. Baseline

Verificado antes de cualquier análisis, sin corregir nada:

```
git rev-parse HEAD        → 0845654d7947c155a77b36e1c534c9c5d1645fb0
git branch --show-current → claude/cuaderno-clinico-desktop-udijjq
git status --porcelain=2  → (vacío — working tree limpio)
```

`git log --oneline -10`:

```
0845654 docs: agregar informe de cierre de Fase 15
8524f31 Fase 15: formulación clínica textual versionada
b42d356 docs: agregar auditoría de transición post-Fase 13 e informe de cierre de Fase 14
c4fb227 Fase 14: historial de cierres y hardening longitudinal
5427aee docs: agregar informe de cierre de Fase 13
bd6d9b1 Fase 13: evaluaciones clínicas y seguimiento psicométrico
684f048 fix: reforzar versionado y archivado del plan de seguridad
7f7bfbd Fase 12: plan de seguridad clínico versionado
d66686b fix: unificar semántica de archivado de pacientes
7e95dc9 Fase 11: cierre estructurado de procesos terapéuticos
```

`HEAD` coincide exactamente con el commit del informe de cierre de Fase 15. Sin discrepancia —
no fue necesario detenerse.

Regresión completa:

| Comando | Resultado |
|---|---|
| `cargo test --release` | **695/695 en verde** |
| `cargo clippy --release --all-targets` | 0 warnings |
| `cargo build --release` | limpio |
| `npm run build` | limpio, sin errores TS |
| `npm run lint` | 24 warnings (categoría preexistente), 0 errors |
| `git diff --check` | limpio |

Coincide exactamente con el estado declarado al cierre de Fase 15. La prueba GUI manual de
Fase 15 sigue explícitamente como **pendiente de validación manual** (bloqueada por un problema
de renderizado de WebKitGTK/Xvfb en este entorno) — no se reinterpreta como verificada en ningún
punto de este documento.

---

## 2. Esquema `documents` real (auditoría directa de `migrations.rs`)

Ubicación: `src-tauri/src/db/migrations.rs`, líneas 166-189, dentro de `SCHEMA_V1` (Fase 1.3).
**Nunca modificada por ninguna migración posterior** (`V2`–`V8`) — verificado con
`grep -n "ALTER TABLE documents" migrations.rs` → sin resultados.

```sql
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
CREATE TRIGGER trg_documents_touch_updated_at ...  -- updated_at automático, patrón estándar del proyecto
```

**Lo que SÍ tiene ya:** `patient_id` (opcional, `ON DELETE SET NULL`), `session_id` (opcional,
`ON DELETE SET NULL`), taxonomía de categoría cerrada (`CHECK`, 6 valores), nombre original,
MIME, tamaño (con `CHECK >= 0`), hash SHA-256 del **plaintext** (con `CHECK length = 64`), una
ruta de almacenamiento (`UNIQUE`, sin ninguna restricción de formato hoy), un flag
`is_clinical`, descripción libre, soft delete (`deleted_at`), timestamps automáticos.

**Lo que NO tiene:** `episode_id`, ningún campo de cifrado (`encrypted`, `nonce`, `format_version`,
`dek_wrapped`, `key_id`), ningún hash del ciphertext, ninguna restricción de formato sobre
`storage_path` (podría contener cualquier string, incluida una ruta absoluta si algo la insertara
así — ver Bloque 5/19).

**Referencias cruzadas al esquema, también sin código real detrás (ver sección 3):**

```sql
-- library_resources.file_document_id TEXT REFERENCES documents(id) ON DELETE SET NULL
-- technique_materials.document_id  TEXT REFERENCES documents(id) ON DELETE SET NULL
```

### Clasificación (A/B/C/D pedida en el encargo)

**A. Esquema existente sin uso.** No es B (infraestructura parcial) ni C (funcional): es un
esquema completo y razonablemente bien pensado (ya anticipa hash, tamaño, MIME, categoría,
`storage_path` separado del nombre real) pero **sin una sola línea de repositorio, servicio,
comando o componente de frontend detrás**. Confirmado por búsqueda exhaustiva (sección 3). No hay
código muerto/legacy (D) tampoco — simplemente nunca se construyó el vertical, igual que
`case_formulations` antes de la Fase 15 y `assessment_instruments` antes de la Fase 13.

**¿Reutilizable o requiere migración?** El esquema es reutilizable en su mayor parte, pero
**probablemente insuficiente** sin un cambio de esquema — ver sección 26 (migraciones necesarias)
y las preguntas de la sección 31. No hay ninguna fila real en ningún entorno (documented
`grep` de código productivo = cero), así que, igual que en Fase 13/15, es el momento más seguro
posible para migrar si hiciera falta.

---

## 3. Infraestructura de filesystem y de código real (Bloque 1 y 2)

Búsqueda exhaustiva de `documents`/`document`/`attachment`/`attachments`/`file`/`files`/`vault`/
`encrypted_file`/`file_vault` en `repositories/`, `services/`, `commands/`, `src/features/`:

```
$ grep -rli "documents\|document_id\|attachment" src-tauri/src/repositories src-tauri/src/services src-tauri/src/commands
(sin resultados)

$ grep -rli "library_resources\|technique_materials\|clinical_techniques" src-tauri/src/repositories src-tauri/src/services src-tauri/src/commands
(sin resultados)

$ grep -rli "document" src/features
PatientDetailScreen.tsx   → solo la pestaña "Documentos" mostrando "Próximamente"
SafetyPlanTab.tsx         → falso positivo, no relacionado
types.ts (safety-plan)    → falso positivo, no relacionado
DashboardScreen.tsx       → falso positivo, no relacionado
```

**Cero código real en cualquier capa.** La pestaña "Documentos" existe en
`PatientDetailScreen.tsx` (línea de `SECTIONS`) pero **no** está en `SECTIONS_WITH_REAL_CONTENT`
— muestra "Próximamente", igual que "Línea temporal".

### Filesystem: qué usa realmente la aplicación

```
$ grep -rn "app_data_dir\|path_resolver\|BaseDirectory" src-tauri/src/*.rs src-tauri/src/commands/*.rs
src/lib.rs:36:            let vault_dir = app.path().app_data_dir()?.join("vault");
src/commands/backup.rs:21: .app_data_dir()
```

Un único punto de entrada real (`lib.rs` línea 36): `vault_dir = app_data_dir()/vault`, creado con
`std::fs::create_dir_all` al arrancar, y pasado a `VaultSession::new(&vault_dir)`
(`security::session`). `commands/backup.rs` recalcula el mismo directorio de forma independiente
(`app.path().app_data_dir().join("vault")`) porque no recibe el `PathBuf` de `lib.rs` — pequeña
duplicación ya existente, no introducida por esta fase.

Ningún path hardcodeado de ningún SO: `grep -rn '"/Users/\|"/home/\|C:\\\\' src-tauri/src` →
sin resultados. Todo path pasa por la API portable `tauri::path::PathResolver` (`app_data_dir()`)
o por `PathBuf`/`Path` genéricos construidos a partir de ese directorio raíz.

**Layout real dentro de `vault_dir` hoy** (confirmado en `security::vault_manager::VaultPaths` y
`backup::manifest`):

```
<app_data_dir>/vault/
    vault.db            (SQLCipher, cifrado con el DEK)
    vault.meta.json      (DEK envuelto dos veces — contraseña y código de recuperación — en claro,
                          pero ninguno de esos campos es secreto sin la KEK correspondiente)
```

**Propuesta portable evaluada contra el código real** (Bloque 2): agregar un tercer elemento
hermano, `<app_data_dir>/vault/files/`, exactamente en el mismo directorio raíz ya resuelto por
`app.path().app_data_dir()` — no hace falta ninguna API nueva, ninguna resolución de ruta nueva,
y es coherente con `docs/ARCHITECTURE.md` sección 7 (que ya proponía esto conceptualmente, nunca
implementado). Dentro de `files/`, la sección 5/en adelante evalúa si conviene un único nivel
plano o "sharding" en subcarpetas (`files/ab/<uuid>.enc`) para no acumular decenas de miles de
archivos en un solo directorio (algunos sistemas de archivos degradan con directorios muy
poblados). Esto se decide en el Bloque de diseño (sección 8), no aquí — aquí solo se confirma
que la ubicación raíz es viable y no requiere ninguna dependencia ni permiso nuevo.

**Operaciones de filesystem reales ya existentes en el proyecto** (para no reinventar patrones):

- `security::vault_meta::VaultMetaFile::save`: escritura atómica real —
  `fs::write(&tmp_path, &json)` seguido de `fs::rename(&tmp_path, path)`. **Este es el patrón
  atómico ya establecido en el proyecto** para "nunca dejar un archivo a medio escribir": se
  reutilizaría igual para escribir un archivo cifrado nuevo (cifrar a un temporal, `rename`
  atómico al destino final).
- `backup::archive`: lectura/escritura de un contenedor ZIP real (`File::open`/`File::create_new`,
  sin recompresión — `CompressionMethod::Stored`), `sha256_file` ya implementado ahí mismo con
  `sha2::Sha256` en streaming por buffer (nunca `read_to_end` completo en memoria) — precedente
  directo y reutilizable para hashear archivos grandes sin cargarlos enteros a RAM.
- `backup::service`: el patrón completo de "escribir todo en un directorio de staging desechable,
  validar todo, y solo entonces mover con un `rename` atómico al destino final, conservando el
  estado anterior en una carpeta de rescate hasta confirmar éxito" — este es exactamente el
  patrón que se recomienda reutilizar conceptualmente para la importación de documentos
  (sección 11).

---

## 4. Infraestructura criptográfica real (Bloque 3 — el bloque más importante)

Auditados directamente: `security/*` (9 archivos), `backup/*`, `db/connection.rs`, `Cargo.toml`,
`Cargo.lock`.

### 4.1 KDF (contraseña → KEK)

`security/kdf.rs`: **Argon2id real** (crate `argon2` 0.6.0, con feature `zeroize`), parámetros
explícitos según RFC 9106 §4 ("second recommended option"): `m_cost = 65536 KiB` (64 MiB),
`t_cost = 3`, `p_cost = 4`, produce una KEK de 32 bytes. Sal aleatoria de 16 bytes por derivación
(`getrandom`). Mismos parámetros para la KEK de contraseña y la KEK del código de recuperación.
Cubierto por 5 tests (roundtrip, secretos distintos → KEKs distintas, sales distintas → KEKs
distintas, `Debug` nunca imprime bytes, sal serializa/deserializa en base64). **Esto es real, no
aspiracional.**

### 4.2 Envelope encryption real (KEK → DEK)

`security/envelope.rs`: el DEK (`db::VaultKey`, 32 bytes aleatorios — la clave real que cifra
`vault.db` vía `PRAGMA key`) se envuelve con **AES-256-GCM** (crate `aes-gcm` 0.11.1, feature
`zeroize`), nonce de 12 bytes aleatorio **nuevo en cada envoltura** (nunca reutilizado — verificado
con el test `two_wraps_of_the_same_dek_use_different_nonces`). `wrap_dek(dek, kek) ->
WrappedKey{nonce, ciphertext}` / `unwrap_dek(wrapped, kek) -> VaultKey`. Fallo de autenticación
(KEK incorrecta o ciphertext manipulado) es indistinguible por diseño (`EnvelopeError::
UnwrapFailed` para ambos casos) — decisión de diseño ya documentada explícitamente en el propio
código. Cubierto por 4 tests, incluido `tampered_ciphertext_is_rejected` (un solo bit alterado en
el ciphertext hace fallar la autenticación).

**Esto es exactamente envelope encryption real, ya en producción — no hay que construirlo desde
cero para Documentos, solo decidir cómo extenderlo (sección 8).**

### 4.3 Dónde vive el DEK durante una sesión desbloqueada

`security/session.rs`, struct `UnlockedSession`:

```rust
struct UnlockedSession {
    conn: Connection,
    #[allow(dead_code)] // se usará para abrir conexiones adicionales en fases futuras
    dek: VaultKey,
    tracker: AutoLockTracker,
}
```

**Hallazgo central de esta auditoría:** el DEK (no la KEK) se retiene en memoria durante toda la
sesión desbloqueada — la KEK se deriva transitoriamente solo durante `unlock`/`recover_access`/
`change_password`/`begin_creation` y se descarta de inmediato. El propio comentario del código
(`"se usará para abrir conexiones adicionales en fases futuras"`) ya anticipaba reutilizar este
campo — aunque específicamente para conexiones SQL adicionales, no para cifrado de archivos, el
principio es el mismo: **es el único secreto de "sesión" disponible sin volver a pedir la
contraseña**.

`VaultSession` (la fachada pública) **no expone ningún método que entregue el DEK** — el único
acceso a la base es `with_connection(f: impl FnOnce(&Connection) -> T)`. No existe hoy ningún
`with_dek(...)` ni equivalente. Esto es relevante para la sección 8/38: cualquier diseño que use
el DEK del vault para envolver una DEK de archivo **requiere agregar un método nuevo a
`security::session::VaultSession`** — es decir, modificar `security/*`, un archivo prohibido por
defecto.

### 4.4 Qué va al Keychain/Credential Manager del SO

```
$ grep -rn "keyring::" src-tauri/src --include=*.rs
src/calendar/tokens.rs   (único uso real)
```

El crate `keyring` (v3, features `apple-native`/`windows-native`/`sync-secret-service`) **solo se
usa para el `refresh_token` de Google Calendar** (Fase 3). La contraseña maestra, el DEK, la KEK y
cualquier otro secreto del vault **nunca tocan el keychain del SO** — confirmado por ausencia total
de otro uso. Esto coincide exactamente con la regla permanente de `CLAUDE.md` ("la contraseña
maestra nunca se almacena en ningún formato ni lugar").

### 4.5 Recovery

`security/recovery_code.rs` (no reproducido aquí en detalle por no ser el foco de esta fase):
un código de alta entropía funciona como una segunda "contraseña" que deriva su propia KEK
(mismos parámetros Argon2id) y envuelve una segunda copia del mismo DEK
(`vault.meta.json` guarda ambas envolturas). No hay puerta trasera: perder ambos (contraseña y
código) hace los datos irrecuperables por diseño.

### 4.6 AEAD disponibles realmente (no solo documentados)

`Cargo.toml`/`Cargo.lock` reales:

| Crate | Versión | Uso real hoy | Relevante para Fase 16 |
|---|---|---|---|
| `aes-gcm` | 0.11.1 (feature `zeroize`) | Envoltura del DEK (`envelope.rs`) | **Sí — AEAD ya disponible y en producción** |
| `aead` | 0.6.1 | Dependencia transitiva de `aes-gcm`, features por defecto únicamente | La versión soporta un feature `stream` (streaming AEAD, sección 4.7) pero **no está habilitado hoy** |
| `chacha20` | (presente en `Cargo.lock`) | Dependencia transitiva de `rand`, **no relacionado con AEAD** | Ninguno — no es el crate `chacha20poly1305` |
| `chacha20poly1305` | **ausente** | — | Si se quisiera XChaCha20-Poly1305, sería una dependencia nueva |
| `sha2` | 0.10 | `sha256_plaintext` en `documents` (nunca escrito hoy), `sha256_file` en `backup::archive` | Reutilizable directamente para hashear documentos |
| `zeroize` | 1.8.2 | DEK, KEK, buffers intermedios | Reutilizable para cualquier clave/DEK de archivo nueva |
| `getrandom` | 0.4.3 | Única fuente de aleatoriedad (`security::random`) | Reutilizable para DEKs/nonces de archivo |
| `base64ct` | 1.8.3 | Codificar bytes binarios como texto en `vault.meta.json` | Reutilizable si se decide guardar una DEK de archivo envuelta como texto en `documents` |
| `zip` | 8.6.0 | Contenedor `.cclinbackup` (Fase 10) | Ya soporta múltiples entradas (ver sección 17) |
| `tauri-plugin-dialog` / `@tauri-apps/plugin-dialog` | 2.7.3 | Diálogos nativos abrir/guardar (Backup/Restore, Fase 10) | **Ya cubre el selector de archivos de importación/exportación — sección 25** |
| `open` | 5 | Abrir el navegador del SO para OAuth (`calendar/oauth.rs`) | `open::that(path)` también abre cualquier archivo con la app por defecto del SO — **reutilizable para "abrir documento" (Bloque 10, opción A)** |
| `keyring` | 3 | Refresh token de Google | No relevante para archivos (sección 4.4) |

**No hay ningún crate criptográfico "de sobra" sin usar** — cada uno presente en `Cargo.toml`
tiene un consumidor real. Esto es consistente con la disciplina de dependencias ya demostrada en
Fases 6.1/13 (rechazar Recharts dos veces).

### 4.7 Streaming AEAD — lo que existiría vs. lo que hay que agregar

El crate `aead` 0.6.1 (ya en el árbol de dependencias, transitivamente) define el módulo
`aead::stream` (construcción STREAM de Rogaway — `EncryptorBE32`/`DecryptorBE32`) **solo si su
propio feature `stream` está habilitado**. Hoy `Cargo.toml` declara `aes-gcm = { version =
"0.11.1", features = ["zeroize"] }` — no reexporta ni activa el feature `stream` de `aead`. Para
usarlo haría falta **una de dos cosas**: (a) agregar `aead` como dependencia directa con
`features = ["stream"]` (formalmente "una dependencia nueva" aunque el crate ya esté en el árbol
transitivamente — el `Cargo.toml` de la app no lo declara hoy), o (b) implementar un formato
chunked propio por encima de `aes-gcm` tal cual está (ver sección 9, alternativa B). Ambas se
presentan sin decidir en la sección 8/9 — **ninguna se adopta en este documento**.

### 4.8 SQLCipher — qué cifra realmente y qué no

`db/connection.rs`: la clave se aplica en **modo raw key** (`PRAGMA key = "x'<64 hex>'"`,
comentario explícito en el propio código: "nunca pasar la contraseña directamente a SQLCipher").
**Importante para no confundir dos cosas distintas, como pide el encargo:** SQLCipher cifra
únicamamente `vault.db` (y todo su contenido: tablas, índices, `sqlite_master`). **Hoy no existe
ningún archivo externo cifrado en el disco** — la tabla `documents` está vacía de código, así que
no hay ningún archivo `.enc` en ningún lugar todavía. Se afirma explícitamente, sin ambigüedad:
**solo `vault.db` está cifrado hoy.**

---

## 5. Threat model de Documentos (Bloque 4)

| # | Amenaza | ¿Cubierta por el diseño recomendado (sección 8)? | Cómo |
|---|---|---|---|
| 1 | Copiar directamente la carpeta de datos de la app | Sí | Cada archivo cifrado individualmente con AEAD; sin la DEK del vault (que a su vez requiere contraseña/recuperación) el ciphertext es inútil — igual garantía que `vault.db` hoy |
| 2 | Obtener el archivo físico del adjunto sin desbloquear la app | Sí | Mismo mecanismo — el archivo en disco es ciphertext puro |
| 3 | Nombre de archivo clínicamente sensible visible en filesystem | Sí | Nombre físico opaco (UUID), nombre real solo dentro de SQLCipher (sección 6) |
| 4 | Metadata sensible visible fuera de SQLCipher | Sí | Toda metadata (nombre, MIME, descripción, asociaciones) vive en `documents`, nunca en el nombre/atributos del archivo físico |
| 5 | Archivo temporal en plaintext | Parcialmente — depende de la decisión del Bloque 10 (visualización) | Un temporal es inevitable si se abre con la app del sistema (opción A); se puede minimizar (carpeta específica, borrado tras cierre) pero no eliminar sin una opción B/C completa |
| 6 | Crash durante importación | Sí | Patrón de staging + rename atómico (igual que Backup/Restore), sección 11 |
| 7 | Crash durante exportación | Parcial | El archivo exportado es una copia deliberada del usuario, fuera del perímetro cifrado por definición (sección 15/Bloque 11) — un crash a mitad de copia deja un archivo incompleto en el destino del usuario, no un problema de seguridad del vault |
| 8 | Copia incompleta | Sí (importación) | Verificación de tamaño/hash antes de confirmar en DB (sección 11) |
| 9 | Manipulación/tampering del ciphertext | Sí | AEAD autentica todo el contenido — cualquier alteración de un solo bit hace fallar el descifrado, mismo comportamiento ya probado en `envelope.rs` |
| 10 | Sustitución de archivos | Sí, parcialmente | El AEAD detecta que el contenido cambió; asociar metadata autenticada (AAD) con el `document_id` esperado (sección 7) detecta además que un ciphertext válido de OTRO documento fue puesto en su lugar |
| 11 | Reutilización de nonce | Sí | Nonce aleatorio de 12 bytes por archivo (o por chunk si hay streaming), nunca reutilizado — mismo criterio ya aplicado en `envelope.rs` |
| 12 | Path traversal | Sí | Nombre físico generado internamente (UUID), nunca derivado del nombre original del usuario — sección 19 |
| 13 | Archivos enormes | Parcial | Límite explícito de tamaño (sección 20) + diseño de streaming/chunked si se aprueba (sección 9); sin límite, un archivo de varios GB cargado con `read_to_end()` agotaría RAM — riesgo real, no hipotético |
| 14 | MIME spoofing / extensión falsa | Parcial | El MIME se guarda como lo reporta el sistema de selección de archivos (no hay verificación de "magic bytes" real hoy en el proyecto) — se documenta como limitación, no se resuelve con una librería de detección nueva sin aprobación |
| 15 | Archivo eliminado de DB pero presente físicamente (huérfano) | Sí | Reconciliador de arranque (sección 34), nunca borra automáticamente sin criterio explícito |
| 16 | DB restaurada desde backup pero archivos no correspondientes | Sí | Backup/Restore de Documentos debe ser atómico como conjunto (sección 17) — nunca restaurar la DB sin los archivos referenciados, o viceversa |
| 17 | Vault bloqueado mientras existe un temporal descifrado | Parcial — ver Bloque 23 | Política a definir explícitamente (sección 23); no hay hoy ningún mecanismo de "purgar temporales al bloquear" en el proyecto porque nunca existió un temporal descifrado antes de esta fase |

**Amenazas que este modelo explícitamente NO intenta resolver** (declarado, no ocultado — mismo
criterio que la sección 10 "Threat model" de `docs/ARCHITECTURE.md`, que ya dice "no se promete
seguridad absoluta en ningún punto"):

- Malware/RAT con privilegios en la misma máquina mientras el vault está desbloqueado (ítem 9 de
  la tabla general de `ARCHITECTURE.md` — se hereda tal cual, no es específico de Documentos).
- Forensia de bajo nivel sobre SSD con wear-leveling (borrado físico irreversible garantizado) —
  ya reconocido como limitación general en `docs/ARCHITECTURE.md` sección 7 para el mismo caso.
- Cualquier antivirus/indexador del SO que decida copiar/cachear el contenido de un temporal
  descifrado antes de que la app lo borre — fuera del control de la aplicación (se documenta como
  riesgo residual, no se intenta mitigar con lógica adicional en el Bloque 10).
- Cualquier cosa que ocurra **después** de una exportación consciente del usuario — por diseño
  (sección 15).

---

## 6. Principio de metadata privada (Bloque 5) — compatible con el esquema existente

La regla propuesta (`filesystem` nunca revela nombre/RUT/diagnóstico/sesión/tipo clínico) **es
compatible con el esquema `documents` actual sin cambios**: `storage_path` ya es una columna
separada de `original_filename` — el esquema, desde su diseño original (Fase 1.3, nunca
implementado), ya anticipaba exactamente esta separación. Lo único que falta es la disciplina de
implementación: generar `storage_path` como un UUID interno, nunca derivado (ni siquiera
parcialmente, ni siquiera con hash) del nombre real o de cualquier identificador del paciente.

Layout propuesto (evaluado, no implementado):

```
<app_data_dir>/vault/files/
  ab/
    ab12cd34-....enc
  7f/
    7f9e0a1b-....enc
```

Sharding de dos caracteres hex del propio UUID como subcarpeta (mismo patrón usado por Git y por
muchos almacenes de blobs de contenido-direccionado) — evita miles de archivos en un único
directorio plano sin necesitar ninguna dependencia nueva; el propio UUID ya aporta esos dos
caracteres. **Decisión abierta en la sección 31**: ¿vale la pena el sharding para el volumen
esperado (una psicóloga independiente, no una clínica con miles de pacientes), o es complejidad
prematura? Ambas opciones son técnicamente triviales; se presenta como pregunta, no como decisión
tomada.

---

## 7. Cifrado por archivo — alternativas evaluadas (Bloque 6)

### Opción A — Clave global de archivos derivada/protegida por el vault

Una única clave (derivada del DEK del vault, p. ej. vía HKDF con un contexto fijo
`"cuaderno-clinico:documents:v1"`) cifra **todos** los archivos.

- **Confidencialidad**: buena mientras el vault esté protegido — pero un único punto de fallo:
  comprometer esa clave compromete **todos** los documentos a la vez.
- **Integridad**: AEAD por archivo sigue detectando tampering individual.
- **Rotación**: rotar la clave global obliga a re-cifrar **todos** los archivos existentes — caro
  y arriesgado (una operación que toca N archivos a la vez tiene N puntos de fallo).
- **Backup/Restore**: simple — un solo material de clave que copiar (ya vive en `vault.meta.json`
  o se deriva de él).
- **Pérdida/corrupción**: si la clave derivada cambia de fórmula alguna vez (bug, cambio de
  parámetros), **todos** los archivos quedan ilegibles a la vez.
- **Complejidad de implementación**: la más baja de las tres.
- **Archivos grandes**: sin impacto adicional respecto a las otras opciones.

### Opción B — DEK aleatoria por archivo, envuelta por una clave del vault (envelope encryption, dos niveles)

Cada archivo genera su propia DEK de archivo (256 bits aleatorios) al importarse. Esa DEK cifra
el contenido (AEAD). La DEK de archivo se envuelve (AES-256-GCM, igual que `envelope.rs`) con una
clave derivada del DEK del vault, y el resultado envuelto (nonce + ciphertext, ~44-60 bytes) se
guarda como una columna nueva en `documents` (texto base64, igual criterio que
`vault.meta.json`).

- **Confidencialidad**: comprometer una DEK de archivo (im)posible solo si se compromete el DEK
  del vault primero — mismo nivel de protección que hoy, sin nuevo punto de fallo único adicional
  a lo que ya existe.
- **Integridad**: AEAD por archivo + la propia envoltura es también AEAD (doble autenticación).
- **Rotación**: rotar la clave del vault (cambio de contraseña) **no requiere re-cifrar ningún
  archivo** — solo re-envolver las DEKs de archivo (operación barata, ya análoga a lo que
  `change_password` hace hoy con el DEK del vault mismo).
- **Backup/Restore**: cada archivo es autocontenido (ciphertext + su envoltura viajan juntos,
  la envoltura vive en la fila de `documents`, que viaja con `vault.db`) — coherente con la
  garantía de que un backup completo (DB + `files/`) es suficiente para restaurar todo.
- **Pérdida/corrupción**: perder o corromper la DEK de UN archivo (fila de DB dañada) afecta solo
  a ESE archivo, nunca a los demás — mucho mejor aislamiento de fallos que la Opción A.
- **Complejidad de implementación**: moderada — reutiliza directamente el mecanismo de
  `envelope.rs` (mismo `wrap`/`unwrap` conceptual, ya probado con 4 tests), solo con una clave de
  entrada distinta (el DEK del vault en vez de una KEK derivada de contraseña).
- **Archivos grandes**: sin impacto — la DEK de archivo es del mismo tamaño fijo (32 bytes)
  independiente del tamaño del archivo que protege.
- **Migraciones futuras**: un `format_version` por archivo (sección 33) permite migrar el
  algoritmo de un archivo a la vez, sin bloquear el resto — más flexible que la Opción A.
- **Multiplataforma**: sin diferencia — es software puro (RustCrypto), sin dependencia de
  hardware específico de ninguna plataforma.

### Opción C — Alternativas descartadas sin más análisis

- **Clave derivada por archivo vía KDF costoso (Argon2id) por archivo**: rechazada de inmediato —
  Argon2id cuesta cientos de milisegundos por diseño (sección 4.1); aplicarlo por archivo haría
  la importación de N archivos perceptiblemente lenta sin ningún beneficio de seguridad adicional
  frente a la Opción B (que ya logra "una clave distinta por archivo" sin ese costo).
- **BLOB en SQLite en vez de archivos en filesystem**: ya rechazada explícitamente en
  `docs/ARCHITECTURE.md` sección 2 desde el diseño original del proyecto ("infla el archivo
  cifrado, hace los backups pesados y lentos de restaurar") — no se reabre esa decisión aquí.
- **Cifrado a nivel de sistema de archivos (LUKS/FileVault/BitLocker) en vez de por archivo**:
  fuera del control de la aplicación — no portable, no auditable desde el código del proyecto,
  y no ofrece ninguna de las garantías (nombre opaco, metadata separada) que exige el Bloque 5.

### Recomendación de esta auditoría

**La Opción B encaja mejor con la arquitectura real auditada en la sección 4** — no es una
preferencia importada sin verificar: es literalmente la extensión natural de un mecanismo que ya
existe, ya está probado, y cuyo propio comentario de código ("el DEK... es la clave real que
cifra la base SQLCipher y los archivos del vault de documentos", `docs/ARCHITECTURE.md` sección 5,
escrito en la Fase 1) ya anticipaba este exacto diseño. Coincide además con la preferencia
histórica indicada en el encargo. **No se adopta aquí como decisión final** — queda sujeta a tu
aprobación explícita en la sección 31, junto con el hallazgo de que requiere tocar
`security/session.rs` (sección 4.3, y STOP formal en sección 38).

---

## 8. Formato criptográfico propuesto (Bloque 7)

**AEAD recomendado: AES-256-GCM**, no XChaCha20-Poly1305 — por una razón puramente de
arquitectura real, no de preferencia: **ya es una dependencia en producción** (`aes-gcm` 0.11.1,
usada en `envelope.rs`), mientras que XChaCha20-Poly1305 requeriría el crate `chacha20poly1305`,
ausente hoy (sección 4.6) — una dependencia nueva sin ningún beneficio de seguridad adicional
claro para este caso de uso (ambos son AEAD modernos, seguros, con implementaciones RustCrypto
auditadas razonablemente equivalentes; la ventaja tradicional de ChaCha20 — mejor rendimiento sin
aceleración AES-NI de hardware — no es un factor decisivo para archivos clínicos de tamaño
moderado en hardware de escritorio moderno, que sí tiene AES-NI).

- **Nonce**: 12 bytes aleatorios (`getrandom`, mismo mecanismo que `security::random`), uno nuevo
  por archivo (o por chunk, si se aprueba streaming — sección 9). Nunca derivado de un contador
  simple ni de timestamp — aleatoriedad real, mismo criterio que `envelope.rs`.
- **Autenticación**: nativa del AEAD (tag de 16 bytes). Se recomienda además autenticar como
  *additional authenticated data* (AAD) el `document_id` (UUID de la fila en `documents`) — así,
  si alguien intercambia dos archivos `.enc` válidos entre sí en el filesystem (ítem 10 del threat
  model), el descifrado falla igual que si el ciphertext estuviera corrupto, porque el AAD
  esperado no coincide.
- **Streaming/archivos grandes**: ver sección 9 — no resuelto por AES-256-GCM "tal cual" sin un
  formato adicional.
- **Memoria**: ver sección 9.
- **Compatibilidad Rust/macOS/Windows**: ninguna, `aes-gcm` es software puro (RustCrypto), sin
  ningún binding nativo por plataforma — ya corre en el mismo binario Tauri hoy.
- **iOS/iPadOS futuro**: mismo razonamiento — el crate es Rust puro, compilable para
  `aarch64-apple-ios` sin cambios; no depende de ninguna API de plataforma. No se implementa en
  esta fase (fuera de alcance, regla 7 de `CLAUDE.md`), pero no hay nada en esta elección que lo
  bloquee a futuro.

**Ninguna dependencia nueva se agrega en este bloque** — `aes-gcm` ya está presente y en uso.

---

## 9. Archivos grandes y streaming (Bloque 8)

**No se recomienda `read_to_end()` completo seguido de un único `encrypt()`/`write_all()`** para
archivos sin límite — sección 20 propone un límite explícito precisamente para acotar este
riesgo mientras no exista streaming real. Dos caminos evaluados, sin decidir cuál adoptar:

### Camino 1 — Límite de tamaño sin streaming (más simple)

Fijar un límite razonable (sección 20 propone un valor concreto a aprobar) por debajo del cual
cifrar el archivo completo en memoria es aceptable (unas pocas decenas de MB caben cómodamente en
RAM de cualquier equipo moderno sin riesgo). **Ventaja**: cero complejidad adicional, cero
dependencia nueva, reutiliza `aes-gcm` tal cual. **Desventaja**: un informe con muchas imágenes
incrustadas o un PDF escaneado de alta resolución podría exceder el límite y no poder importarse
sin que el usuario lo comprima antes — límite de producto, no de seguridad.

### Camino 2 — Formato chunked autenticado (streaming real)

Si se decide soportar archivos más grandes sin cargarlos enteros en RAM, se necesita un formato
propio por chunks, ya que **AES-256-GCM "plano" no es seguro de aplicar ingenuamente chunk por
chunk con nonces independientes sin una construcción formal** (repetir nonce entre chunks del
mismo archivo sería catastrófico; usar nonces aleatorios por chunk es seguro pero desperdicia
espacio de nonce sin necesidad frente a una construcción diseñada para esto). La opción estándar
es la construcción **STREAM** (Hoang/Reyhanitabar/Rogaway/Vizár, ya implementada en el crate
`aead::stream`, sección 4.7): nonce base de 7 bytes + contador de 4 bytes + 1 bit de "último
chunk", autenticado por chunk, con detección de truncamiento (el último chunk lleva un flag
autenticado que impide cortar el archivo silenciosamente).

Elementos de diseño a decidir explícitamente si se aprueba este camino (**no implementar sin
aprobación**, y **requiere declarar la dependencia `aead` con el feature `stream` como decisión
pendiente**, sección 4.7):

- **Tamaño de chunk**: un valor típico razonable es 64 KiB o 1 MiB — suficientemente grande para
  no generar overhead de tag por chunk excesivo, suficientemente pequeño para no perder la
  ventaja de streaming. Sin decidir aquí.
- **Nonce por chunk**: derivado determinísticamente del nonce base + índice de chunk (parte de la
  construcción STREAM), nunca aleatorio independiente por chunk (rompería la garantía de la
  construcción).
- **Orden**: los chunks deben descifrarse en orden estricto — la construcción STREAM ata cada
  chunk criptográficamente al anterior mediante el contador, así que un reordenamiento se detecta
  como fallo de autenticación.
- **Truncamiento**: el flag de "último chunk" autenticado impide que un atacante (o una copia
  incompleta, ítem 8 del threat model) corte el archivo y lo haga pasar por completo — un archivo
  truncado sin el flag final falla la verificación.
- **Metadata autenticada**: el mismo AAD por documento (sección 8) se aplicaría igual a cada
  chunk, no solo al primero.

**Recomendación de esta auditoría**: empezar con el Camino 1 (límite simple, sección 20) para el
V1 de esta fase, dejando el Camino 2 documentado y diseñado (este mismo bloque) para una fase de
"streaming de documentos grandes" posterior si la necesidad real aparece — mismo criterio
conservador ya aplicado repetidamente en el proyecto (Recharts en Fase 6.1/13, editor de texto
enriquecido en Fase 15). **No se decide aquí** — es una de las preguntas de la sección 31.

---

## 10. Importación segura (Bloque 9)

Flujo propuesto, modelado directamente sobre el patrón ya probado de `backup::service`
(staging + validación + rename atómico):

1. Usuario selecciona un archivo (diálogo nativo, `@tauri-apps/plugin-dialog`, ya disponible —
   sección 25).
2. **Validar** en el frontend/comando: tamaño ≤ límite (sección 20), no vacío, nombre no vacío.
3. Generar `document_id` (UUID v4, mismo crate `uuid` ya en uso) y la ruta física opaca
   (`files/<2-hex>/<uuid>.enc`).
4. Generar DEK de archivo aleatoria (sección 7, Opción B) + nonce aleatorio.
5. Cifrar el contenido (en memoria, dentro del límite del Camino 1 — sección 9) con AAD =
   `document_id`.
6. Escribir el ciphertext a un **archivo temporal** en el mismo volumen que el destino final
   (p. ej. `files/<2-hex>/<uuid>.enc.tmp`) — mismo directorio para que el `rename` posterior sea
   atómico dentro del mismo sistema de archivos (un `rename` entre volúmenes distintos no es
   atómico en la mayoría de los SO).
7. `rename` atómico del temporal al nombre final — solo entonces el archivo "existe" de verdad
   para el resto del sistema.
8. **Solo después** del `rename` exitoso: transacción SQL que inserta la fila en `documents` (DEK
   de archivo envuelta, hash, metadata, asociaciones).
9. Si cualquier paso 2-7 falla: limpiar el temporal (si llegó a crearse) y no tocar la DB —
   ninguna referencia DB a un archivo inexistente puede llegar a crearse porque el `INSERT` es
   el último paso, no el primero.
10. Si el paso 8 (transacción SQL) falla **después** de un `rename` exitoso (poco probable pero
    posible — disco lleno, etc.): queda un archivo cifrado huérfano en `files/` sin fila en DB —
    cubierto por el reconciliador de arranque (sección 34), nunca aparece como "documento" en la
    UI porque no hay fila que lo represente, y puede limpiarse de forma segura porque un
    ciphertext sin absolutamente ninguna metadata asociada no representa una pérdida de
    información recuperable de otra forma.

**Qué NO puede pasar con este orden:** una fila en `documents` apuntando a un `storage_path` que
nunca llegó a existir (el `INSERT` es el último paso), ni un archivo de texto plano creado por la
propia aplicación en ningún punto del proceso (el archivo temporal ya se escribe cifrado desde el
paso 6 — nunca se escribe el plaintext a disco en ningún punto de la importación).

---

## 11. Visualización / apertura (Bloque 10)

Tres opciones evaluadas:

**A. Descifrar temporalmente a disco y abrir con la aplicación del sistema.** Usa el crate `open`
(ya en `Cargo.toml`, ya usado para abrir el navegador en OAuth — sección 4.6) —
`open::that(temp_path)`. Es la única opción que soporta de forma realista PDFs, DOCX e "otros
formatos" sin construir un visor propio de cada formato dentro del WebView. **Costo**: existe una
ventana de tiempo real, aunque breve, en la que el plaintext vive en un archivo temporal en disco
— exactamente la amenaza #5 y #17 del threat model (sección 5). Mitigación parcial: usar el
directorio de temporales del propio SO (`std::env::temp_dir()`, ya usado extensivamente en los
tests del proyecto — nunca en producción hoy) en vez de dentro del vault, nombrar el temporal con
un UUID nuevo (nunca el nombre real), y borrarlo proactivamente cuando sea detectable hacerlo
(sección 23) — sin poder garantizar el borrado si la aplicación del sistema sigue el archivo
abierto (un lector de PDF puede mantenerlo abierto indefinidamente).

**B. Descifrar en memoria y mostrar dentro de Cuaderno Clínico.** Viable de forma nativa y
razonablemente simple para **imágenes** (un `<img src="data:image/...;base64,...">` en el
WebView, sin ningún archivo temporal en disco). **Mucho más difícil para PDF** sin una librería de
renderizado de PDF en el frontend (ninguna está instalada hoy; agregar una sería una dependencia
JS nueva, sujeta a aprobación) — el propio WebView de Tauri puede renderizar un PDF vía
`<embed>`/`<iframe>` con una URL `data:` en algunos casos, pero el soporte varía por WebView
(WebKitGTK en Linux, que ya mostró limitaciones de renderizado reales en esta misma sesión para
Fase 15 — sección de prueba manual pendiente) y no está verificado empíricamente en este entorno.
Imposible de forma directa para DOCX (requeriría convertir el formato, fuera de alcance).

**C. Estrategia híbrida por MIME (recomendada para evaluar, no decidida):** imágenes → Opción B
(sin ningún temporal); PDF/DOCX/"otros" → Opción A (temporal con las mitigaciones descritas).
Esto acota la ventana de exposición de plaintext en disco al subconjunto de formatos que
realmente lo necesitan, sin renunciar a soportar PDFs (el formato más probable para
consentimientos/informes/derivaciones, según el propio encargo).

**Limpieza de temporales**: al cerrar el visor externo (no siempre detectable — el SO no siempre
notifica cuándo el usuario cerró la aplicación externa), al bloquear el vault (sección 23), y como
red de seguridad, un barrido de temporales huérfanos al iniciar la aplicación (buscando por un
prefijo/patrón reconocible propio de esta app dentro de `temp_dir()`, nunca borrando archivos
ajenos). **Antivirus/indexadores**: Windows Search/Spotlight podrían indexar el contenido de un
temporal antes de que se borre — riesgo residual reconocido, no mitigable completamente desde la
aplicación (mismo tipo de limitación ya aceptada para malware en la sección 5). **Quick Look
(macOS) / Windows Shell**: ambos podrían generar una miniatura cacheada del temporal — mismo
riesgo residual. **Futuras plataformas móviles**: fuera de alcance de esta fase (regla 7 de
`CLAUDE.md`); se señala que la Opción B (sin temporal) es estructuralmente más segura para un
futuro iOS/iPadOS, donde el sandboxing de archivos es más estricto y compartir un archivo temporal
con "la aplicación por defecto" es un concepto distinto (share sheet, no un `open::that`
equivalente).

---

## 12. Exportar / "Guardar como" (Bloque 11)

Diferenciado explícitamente de "abrir/ver" (sección 11): exportar es una acción **explícita** del
usuario, con su propio botón ("Descargar copia" / "Exportar"), nunca una consecuencia automática
de abrir un documento. Usa el mismo diálogo nativo de guardar ya disponible
(`@tauri-apps/plugin-dialog`, `save()` — sección 25), igual que Backup ya hace hoy. El archivo
resultante en el destino elegido por el usuario **queda en texto plano deliberadamente** — es
exactamente lo que el usuario pidió al exportar, mismo principio ya aplicado en
`docs/ARCHITECTURE.md` sección 9.b ("Exportación abierta"). La UI debe advertir, en un tono
sobrio y no alarmista (mismo criterio de diseño ya establecido en el Plan de Seguridad, Fase 12,
para su advertencia clínica), que la copia exportada ya no está protegida por el cifrado del
vault — una sola línea de texto discreta cerca del botón, no un modal de confirmación agresivo.

---

## 13. Soft delete (Bloque 12)

`documents.deleted_at` ya existe en el esquema (sección 2) — mismo patrón que toda otra entidad
clínica del proyecto. Política recomendada, alineada con el resto del dominio:

1. **Soft delete lógico** (`deleted_at` con timestamp): el documento desaparece de los listados
   normales, pero el ciphertext físico **permanece intacto en `files/`** — nunca se borra al
   archivar. Restaurable (`deleted_at = NULL`) en cualquier momento, igual que pacientes/sesiones/
   tareas/pagos.
2. **Eliminación física del ciphertext**: **no** ocurre automáticamente al archivar. Se deja
   explícitamente para una acción futura separada y deliberada ("vaciar papelera" o equivalente),
   fuera de esta fase salvo que se apruebe lo contrario — mismo criterio ya aplicado en
   `docs/ARCHITECTURE.md` sección 7 ("el borrado físico real... es best-effort", y ya reconoce
   que en SSD modernos no hay garantía forense de borrado irreversible de todas formas).

**No hay hard-delete automático en ningún punto de este diseño** — coincide con la regla
permanente de `CLAUDE.md` ("nunca borrar... una base existente... sin advertencia explícita"),
extendida aquí por analogía a archivos individuales.

---

## 14. Asociación con paciente / proceso / sesión (Bloque 13)

Auditado el esquema real (sección 2): `documents.patient_id` (opcional, `ON DELETE SET NULL`) y
`documents.session_id` (opcional, `ON DELETE SET NULL`) — **sin `episode_id`**.

Aplicando explícitamente la lección de Formulación (Fase 15, `Plan-Fase-15-pendiente-de-
aprobacion.md`): un documento **puede** ser longitudinal del paciente (p. ej. una derivación de
otro profesional recibida antes de que exista ningún proceso formal), **puede** pertenecer a un
proceso concreto (p. ej. el consentimiento informado de un proceso terapéutico específico), y
**puede** pertenecer a una sesión concreta (ya cubierto hoy por `session_id`). A diferencia de
Formulación (donde el encargo pedía explícitamente una hipótesis clínica ligada siempre a UN
proceso), aquí **no hay una razón clínica obvia para forzar que todo documento pertenezca
obligatoriamente a un proceso** — un documento administrativo antiguo, o un documento recibido
antes de que exista ningún proceso formal, son casos de uso legítimos sin proceso asociado.

**Recomendación de esta auditoría**: si se decide agregar `episode_id`, debería ser **opcional**
(a diferencia del `episode_id` obligatorio a nivel de servicio que se decidió para Formulación),
exactamente con el mismo patrón ya usado tres veces en el proyecto para vínculos opcionales a
proceso (`sessions.episode_id`, `therapeutic_goals.episode_id`, `assessment_administrations.
episode_id` de la Fase 13) — nulable en el esquema, revalidado con `check_episode_assignable`
solo cuando se informa. **No se decide en este documento si agregar `episode_id` es necesario para
el V1** — se presenta como pregunta abierta (sección 31), porque a diferencia de Formulación no
hay un requisito explícito del encargo que lo exija (el encargo de esta fase no menciona
"reingreso" ni "proceso anterior" para documentos en ningún bloque). **No se inventa ninguna FK
sin esta aprobación explícita.**

---

## 15. Categorías de documentos (Bloque 14)

El esquema real (sección 2) **ya tiene** una taxonomía cerrada vía `CHECK`:
`'informe','consentimiento','evaluacion_adjunta','receta','correspondencia','otro'` — seis
categorías administrativas, ninguna diagnóstica, coincide exactamente con el espíritu pedido en
el encargo (evita sobre-modelar). Es más rígida que "TEXT flexible" pero más simple que un
catálogo separado (una tabla `document_categories` propia, que no existe hoy y que sería
sobre-ingeniería para seis valores fijos que ya cubren razonablemente los ejemplos del encargo).

**Recomendación**: mantener el `CHECK` existente tal cual, sin convertirlo en catálogo — mismo
criterio ya aplicado a `payments.status`/`therapeutic_goals.status`/`episode_closures.reason` en
todo el proyecto (taxonomías fijas pequeñas se modelan con `CHECK`, no con tablas catálogo,
reservando las tablas catálogo para conjuntos abiertos que la usuaria mantiene ella misma, como
`assessment_instruments`). **Pregunta abierta**: ¿las seis categorías actuales cubren
razonablemente los ejemplos del encargo ("consentimientos, informes, derivaciones, PDFs,
imágenes, documentos entregados/recibidos")? Nótese que "derivación" no tiene una categoría
propia hoy (encajaría en `'correspondencia'` u `'otro'`) — se señala como posible hueco menor a
decidir, no se corrige unilateralmente en este documento.

---

## 16. Consentimientos (Bloque 15)

Esta fase cubre **únicamente la opción A** del encargo: almacenar un consentimiento ya firmado
externamente (en papel y luego escaneado, o firmado digitalmente por un servicio externo) como un
documento cifrado más, con `category = 'consentimiento'`. **Explícitamente fuera de alcance**:
capturar una firma manuscrita dentro de la app (opción B — requeriría un componente de captura de
firma, una decisión de UX y posiblemente una dependencia nueva de canvas/dibujo, no evaluada
aquí) y cualquier forma de firma electrónica avanzada/legal con validez jurídica (opción C —
implica cumplimiento normativo específico, fuera del alcance de una auditoría técnica). Ninguna
de las dos se diseña ni se menciona más allá de este párrafo, salvo aprobación futura explícita.

---

## 17. Backup / Restore (Bloque 16 — requiere aprobación explícita antes de tocar `backup/*`)

Auditado directamente el código real de Fase 10 (`backup/manifest.rs`, `backup/archive.rs`,
`backup/service.rs`), no solo `docs/backup-restore.md`.

### Hallazgo importante: la afirmación de `docs/ARCHITECTURE.md` no es exacta

`docs/ARCHITECTURE.md` (sección 9, nota de corrección de Fase 10) dice: *"el formato del
contenedor de Fase 10 la reserva [la carpeta `documents/`] como entrada opcional"*. Verificado
contra el código real: **esto es parcialmente inexacto**. Lo que existe realmente:

- `BackupManifest.files: Vec<BackupFileEntry>` — el **modelo de datos** ya es genuinamente una
  lista de longitud variable (no un struct con campos fijos) — así que estructuralmente sí podría
  llevar N archivos.
- `archive::write_container(dest, entries: &[(&str, &Path)])` y `archive::extract_container(...)`
  — ambas funciones **ya son genéricas** sobre cualquier número de entradas con nombre arbitrario,
  sin ningún cambio de firma necesario.
- **Pero** `backup::service::create_backup`/`restore_backup` (la lógica de orquestación real,
  no el modelo de datos ni las primitivas de archivo) **hoy arman la lista de entradas de forma
  hardcodeada con exactamente tres elementos fijos**: `MANIFEST_ENTRY`, `VAULT_DB_ENTRY`,
  `VAULT_META_ENTRY`. No hay ningún bucle que enumere un directorio `documents/`, ni código que
  sepa qué hacer con una entrada `documents/<id>.enc` al extraer.

**Conclusión**: el contenedor **no** "ya soporta" documentos hoy en el sentido de que baste con
agregar archivos y ya funcione — sí lo soportaría con una extensión relativamente acotada y de
bajo riesgo (el modelo de datos y las primitivas de bajo nivel no cambian; solo el cuerpo de dos
funciones de orquestación en `service.rs` necesita extenderse para enumerar `files/` y agregar/
extraer esas entradas). Esto es, de todas formas, **una modificación real a `backup/service.rs`**
— archivo prohibido por defecto — así que se presenta aquí y se detiene, según pide el encargo,
sin tocarlo.

### Qué debería garantizar un backup completo con Documentos (si se aprueba)

- Incluir: `vault.db` (con las filas de `documents` actualizadas), **todos** los archivos
  cifrados en `files/` referenciados por filas no eliminadas físicamente, `vault.meta.json`.
- Nunca incluir plaintext (los archivos en `files/` ya están cifrados — copiarlos "tal cual" al
  contenedor no requiere descifrar-recifrar, igual que `vault.db` no se re-cifra en el backup
  actual).
- Nunca duplicar claves de forma insegura: la DEK de archivo envuelta (sección 7, Opción B) ya
  viaja dentro de la fila de `documents`, que a su vez viaja dentro de `vault.db` — **no hace
  falta ningún material de clave adicional en el manifest del backup**, evitando exactamente el
  riesgo que el encargo pide prevenir explícitamente.
- Nunca permitir que DB y archivos queden desincronizados: el backup debe capturar ambos como un
  conjunto atómico desde la perspectiva del usuario (mismo `VACUUM INTO` de `vault.db` ya usado
  hoy, más una copia de los archivos de `files/` existentes **en ese mismo momento** — un archivo
  importado a mitad de un backup en curso es un caso borde a resolver explícitamente si se
  aprueba esta integración, no cubierto por el mecanismo actual de un solo archivo).

**No se modifica `backup/*` en esta fase de auditoría — DETENIDO, como pide el encargo, a la
espera de aprobación explícita antes de tocarlo (sección 31).**

---

## 18. Restore y consistencia (Bloque 17)

Validaciones adicionales que un `restore_backup` con Documentos necesitaría (diseño, no
implementación), extendiendo el patrón de validación ya real en `backup::service::restore_backup`
(que hoy valida manifest, hashes SHA-256, credencial, `schema_version`, `foreign_key_check` —
todo antes de tocar el vault activo):

- Archivo listado en `documents` (fila no eliminada) pero su entrada `files/<id>.enc` ausente del
  backup → **rechazar el restore completo**, mismo criterio "todo o nada" que ya aplica hoy a
  `vault.db`/`vault.meta.json` faltantes (`RestoreError::MissingRequiredFile`).
- Entrada `files/<id>.enc` presente en el backup sin ninguna fila correspondiente en `documents`
  → no debería impedir el restore (es un huérfano ya en el momento del backup, cubierto por el
  mismo reconciliador de la sección 34), pero sí debería registrarse como advertencia, nunca
  ignorarse silenciosamente.
- Hash/tag inválido de un archivo individual → mismo mecanismo ya real de
  `RestoreError::FileHashMismatch`, extendido a cada entrada de `files/`, no solo a `vault.db`/
  `vault.meta.json`.
- Tamaño inesperado → mismo mecanismo ya real (`manifest_entry_for` calcula tamaño real al
  respaldar; el restore ya compara contra el manifest).
- `format_version` de cifrado de un archivo individual desconocido por esta versión de la app
  (sección 33) → rechazar ese archivo específico con un error claro, sin bloquear el resto del
  restore si los demás son de una versión conocida — decisión de diseño a confirmar (¿todo o
  nada, o por archivo?), no resuelta aquí.
- Backup incompleto (algunas entradas de `files/` truncadas a mitad de escritura del propio
  backup) → mismo mecanismo de verificación de hash ya existente, simplemente aplicado a más
  archivos.
- Restore de una versión futura del formato (`backup_format_version` mayor a lo soportado) → ya
  existe hoy (`RestoreError::SchemaTooNew` cubre `schema_version`; un mecanismo análogo aplicaría
  a `backup_format_version` si el formato del contenedor mismo cambiara, cosa que agregar
  Documentos **no necesariamente requiere** — ver sección 17, el modelo de datos ya es genérico).

**Ninguna corrupción se ignora silenciosamente en ningún punto de este diseño** — mismo principio
ya package en el código real de Fase 10.

---

## 19. Hashing (Bloque 18)

El esquema ya tiene `sha256_plaintext` (hash del contenido **descifrado**). Evaluado
explícitamente si esto es un riesgo: un hash del plaintext permite, en teoría, confirmar si un
documento conocido específico (p. ej. un formulario estándar público) está presente, **sin
necesidad de descifrarlo** — un atacante con el hash de un documento público de referencia podría
comparar. Para documentos clínicos genuinamente únicos (una derivación específica de un paciente
concreto) este riesgo es bajo en la práctica (el atacante necesitaría ya conocer el contenido
exacto de antemano, en cuyo caso el hash no le aporta información nueva relevante) pero no es
cero para documentos administrativos estandarizados (p. ej. una plantilla de consentimiento
informado idéntica para todos los pacientes).

**Recomendación**: mantener `sha256_plaintext` **solo si aporta valor real de verificación de
integridad de importación** (confirmar que el archivo importado no se corrompió al leerlo del
disco de origen, antes de cifrarlo) — y evaluar si conviene, en cambio, un **hash del ciphertext**
para verificación de integridad del backup/restore (que es exactamente lo que `backup::archive::
sha256_file` ya hace hoy sobre `vault.db`, sin ningún hash del plaintext de la base). Dado que el
AEAD ya autentica el contenido en cada descifrado (sección 8), **un hash adicional del plaintext
no es estrictamente necesario para detectar corrupción o tampering** — el propio AEAD ya lo hace,
mejor y sin el riesgo de inferencia descrito arriba. **No se agrega ningún hash "porque sí"**,
según pide explícitamente el encargo — se presenta como pregunta abierta (sección 31): ¿mantener
`sha256_plaintext` tal como está en el esquema (columna ya `NOT NULL` — quitarla sería un cambio
de esquema más invasivo que dejarla) pero dejar de escribirla con un valor real, o sí calcularla y
aceptar el riesgo residual descrito, documentándolo?

---

## 20. Nombres de archivo (Bloque 19)

El nombre original **debe vivir dentro de SQLCipher** (`documents.original_filename`, ya existe)
— nunca en el nombre físico (sección 6). Consideraciones adicionales identificadas:

- **Normalización Unicode**: `original_filename` debería normalizarse (NFC) al guardar, para
  evitar que dos nombres visualmente idénticos pero con distinta representación de bytes (p. ej.
  "é" precompuesto vs. "e" + acento combinante) se traten como diferentes en búsquedas futuras —
  Rust no normaliza Unicode por defecto; requeriría el crate `unicode-normalization` **o**
  simplemente no normalizar y aceptar la limitación (decisión de bajo riesgo, no crítica).
- **Path traversal**: como el nombre físico es siempre un UUID generado internamente (nunca
  derivado del nombre real ni de ningún input del usuario), no hay vector de path traversal
  posible en el nombre físico por diseño — el `original_filename` guardado en DB es texto puro,
  nunca se usa para construir una ruta de filesystem.
- **Caracteres inválidos / nombres extremadamente largos**: como `original_filename` es solo
  metadata de texto (nunca se usa como ruta real), no hay ninguna restricción técnica del
  filesystem que aplique — solo un límite razonable de longitud a nivel de validación de servicio
  (p. ej. unos pocos cientos de caracteres) para evitar abuso, a definir junto con los demás
  límites (sección 21).
- **Duplicados**: dos documentos con el mismo `original_filename` para el mismo paciente son
  válidos y esperables (p. ej. "consentimiento.pdf" de dos procesos distintos) — `storage_path`
  ya es `UNIQUE` (por ser un UUID), así que nunca hay colisión física; no se necesita ninguna
  lógica de desambiguación de nombres.

---

## 21. Límites (Bloque 20)

Propuestos para aprobación explícita, no impuestos unilateralmente:

- **Tamaño máximo por archivo**: sujeto a la decisión de streaming (sección 9) — con el Camino 1
  (sin streaming), un límite conservador razonable estaría en el rango de **20-50 MB** (cubre
  PDFs escaneados típicos e imágenes de alta resolución sin riesgo real de agotar RAM en
  hardware de escritorio moderno). Cifra exacta a confirmar contigo, no decidida aquí.
- **Extensiones/MIME permitidos**: se recomienda **no** restringir por lista blanca cerrada —
  coincide con el espíritu del encargo ("documentos clínicos razonables" es deliberadamente
  abierto) y con la ausencia de cualquier catálogo cerrado de MIME en el resto del proyecto. Se
  registra el MIME reportado por el selector de archivos del sistema, sin validarlo contra una
  lista.
  cerrada de "documento clínico razonable" arriesga bloquear casos legítimos no anticipados.
- **Cantidad máxima razonable**: no se propone ningún límite artificial al número de documentos
  por paciente/proceso — ninguna otra entidad del proyecto (sesiones, pagos, tareas) tiene un
  límite de cantidad; no hay razón para que Documentos sea la excepción.
- **Archivos vacíos**: rechazar (`size_bytes == 0`) — un documento vacío no aporta valor y ya
  violaría implícitamente cualquier CHECK futuro de tamaño mínimo si se agregara; se recomienda
  rechazar explícitamente en el servicio con un error claro, no solo confiar en el `CHECK (>= 0)`
  actual del esquema (que sí permite 0 hoy).
- **Nombres vacíos**: rechazar `original_filename` vacío o solo espacios en blanco — mismo
  criterio ya aplicado a `title`/`full_name` en el resto del proyecto (Formulación, Pacientes,
  Objetivos).

---

## 22. Privacidad (Bloque 21)

Confirmado por auditoría directa (no solo por documentación):

```
$ grep -rli "document\|attachment\|storage_path\|sha256" src-tauri/src/calendar/
(sin resultados reales — los 4 falsos positivos encontrados son la palabra "documenta" en
comentarios y el uso no relacionado de SHA-256 para PKCE en el flujo OAuth de Google Calendar)
```

Se documenta expresamente, para esta fase, que:

- Los archivos y sus nombres **nunca** se envían a Google Calendar — no hay ningún código en
  `calendar/*` que conozca la existencia de `documents`, y esta fase **no modifica
  `calendar/*`** (confirmado como no necesario por la propia auditoría).
- El contenido de un documento **nunca** sale a ningún servicio externo — no existe ningún
  cliente HTTP en el proyecto salvo `reqwest` en `calendar/*` (exclusivamente para la API de
  Google Calendar, con scope mínimo), y esta fase no introduce ningún cliente de red nuevo.
- **No hay OCR automático, no hay IA, no hay indexación en la nube, no hay analytics, no hay
  subida remota** — ninguno de estos conceptos existe en ningún punto del proyecto hoy (confirmado
  por la ausencia total de cualquier dependencia de red, ML o telemetría en `Cargo.toml`/
  `package.json` más allá de lo ya auditado), y esta fase no los introduce.

---

## 23. Logging (Bloque 22)

Auditado el uso real de logging en todo el proyecto (`grep -rn "log::\|println!\|eprintln!"
src-tauri/src`): el único uso productivo es `tauri_plugin_log` a nivel `Info`, **habilitado solo
en builds de debug** (`if cfg!(debug_assertions)`, `lib.rs` línea 28) — en producción no hay
ningún logging activo en absoluto. El único otro uso del crate `log` en todo el código es un
logger de prueba (`CapturingLogger`) dentro de los tests de `security::vault_manager`, usado
precisamente para **verificar que nada sensible se registra** durante el desbloqueo. **No existe
hoy ningún `log::info!`/`log::error!` con contenido de negocio en ninguna parte del código.**

Diseño de errores seguro para Documentos, coherente con este precedente y con el patrón de
errores ya usado en todo el proyecto (enums de error tipados, nunca `format!` con contenido
sensible interpolado):

- Ningún mensaje de error de este dominio debe interpolar `original_filename`, ninguna ruta
  completa del filesystem, contenido, `patient_id` (salvo en el sentido ya aceptado en todo el
  proyecto de un ID técnico opaco, nunca un mensaje que combine el ID con contenido clínico),
  claves, nonces (sin razón técnica que lo justifique — un nonce no es secreto por sí solo, pero
  no aporta nada útil a un mensaje de error de cara al usuario), la DEK de archivo, ni ninguna
  metadata clínica.
- Mismo patrón ya usado en `EnvelopeError`/`RestoreError`/`FormulationError`: variantes de error
  específicas y opacas (`FileNotFound`, `DecryptionFailed`, `SizeLimitExceeded`), nunca un mensaje
  genérico que incluya el path o el contenido crudo del error subyacente del SO cuando ese error
  pudiera filtrar una ruta sensible.

---

## 24. Lock / Unlock (Bloque 23)

Analizado contra el mecanismo real de `security::session` (auto-lock por inactividad,
`tick_auto_lock`, zeroización del DEK al bloquear — `VaultSession::lock()`):

- **Usuario abre un documento (Opción A, temporal en disco) y luego el vault se bloquea por
  inactividad**: el temporal descifrado en disco **no desaparece automáticamente** solo porque el
  vault se bloqueó (bloquear el vault zeroiza el DEK en memoria y cierra la conexión SQL — no
  tiene ningún mecanismo hoy que sepa de archivos temporales, porque nunca existió ninguno antes
  de esta fase). **Esto es una brecha real a resolver explícitamente si se aprueba la Opción A**:
  se recomienda que `lock()` dispare también una limpieza de cualquier temporal de documentos
  activo — requiere que el mecanismo de temporales sea rastreable desde el mismo lugar que
  gestiona el bloqueo (posible necesidad adicional de tocar `security/session.rs`, más allá de lo
  ya identificado en la sección 4.3 — ver STOP en sección 38).
- **App pasa a segundo plano**: en escritorio (macOS/Windows) no hay un evento equivalente a
  "background" de móvil que la aplicación pueda interceptar de forma confiable a través de Tauri
  sin una investigación adicional no cubierta por esta auditoría — se documenta como no resuelto,
  no se inventa una solución.
- **App se cierra abruptamente (crash, kill -9)**: el temporal descifrado, si existía, queda en
  disco indefinidamente hasta el próximo arranque — cubierto parcialmente por un barrido de
  temporales huérfanos al iniciar (sección 11), igual criterio que `run_startup_recovery` ya
  aplica hoy a un `restore_backup` interrumpido.
- **Futuro iOS/iPadOS**: el sistema operativo puede terminar la app en segundo plano sin previo
  aviso en cualquier momento — refuerza la recomendación de la sección 11 de preferir la Opción B
  (sin temporal) cuando sea viable, precisamente por este motivo, aunque la implementación de
  iOS/iPadOS en sí queda fuera de alcance de esta fase.

---

## 25. UI propuesta (Bloque 24)

Pestaña "Documentos" en `PatientDetailScreen.tsx` (ya reservada en `SECTIONS`, hoy muestra
"Próximamente" — mismo punto de integración ya usado por cada vertical anterior desde Fase 6).
Mínimo funcional evaluado:

- Listar (activos/archivados, mismo patrón de dos vistas ya usado en Pagos/Tareas/Evaluaciones).
- Agregar (selector de archivo nativo — sección 27).
- Abrir/ver (según la estrategia que se apruebe — sección 11).
- Descargar/exportar copia (sección 12).
- Editar metadata (categoría, descripción — nunca el archivo en sí, que es inmutable una vez
  importado; reemplazar el contenido de un documento sería, conceptualmente, archivar el viejo e
  importar uno nuevo, no una "edición" del ciphertext).
- Archivar/restaurar (sección 13).

Cada fila: nombre (el real, desde `documents.original_filename` — nunca la ruta física),
categoría, fecha, tamaño (formateado legible, p. ej. "2.4 MB" — nunca bytes crudos), asociación a
proceso/sesión cuando exista (mismo patrón de metadata ya usado en `FormulationCard`/
`AssessmentsTab`). **Nunca se muestra `storage_path` ni ninguna ruta interna del filesystem en
ningún punto de la UI** — coincide con el principio de metadata privada (sección 6), reforzado
aquí explícitamente como requisito de UI, no solo de backend.

---

## 26. Importación desde UI — dependencias reales (Bloque 25)

**Ya existe todo lo necesario, sin ninguna dependencia nueva:**

```json
// package.json — ya presente
"@tauri-apps/plugin-dialog": "^2.7.3"
```

```toml
# Cargo.toml — ya presente
tauri-plugin-dialog = "2.7.3"
```

```json
// capabilities/default.json — ya presente
"permissions": ["core:default", "dialog:allow-open", "dialog:allow-save"]
```

Ya usado en producción hoy: `src/features/backup/api.ts` importa `open`/`save` de
`@tauri-apps/plugin-dialog` para los flujos de Backup/Restore (Fase 10). Para Documentos, el
mismo `open()` con un filtro de extensiones (la API del plugin soporta `filters: [{name, extensions}]`)
cubriría el selector de importación sin instalar nada nuevo. **No se instala ninguna dependencia
en este bloque — no hace falta.**

---

## 27. Drag & Drop (Bloque 26)

Deseable pero no obligatorio, según el propio encargo. Tauri/WebKitGTK/WebView2 soportan eventos
de arrastre HTML5 nativos del navegador (`ondrop`, `DataTransfer.files`) sin ningún plugin
adicional — técnicamente viable sin dependencia nueva. **Prioridad confirmada explícitamente,
como pide el encargo: el selector de archivos nativo (ya disponible, sección 26) es el V1; drag &
drop, si se decide incluirlo, sería un incremento posterior no crítico para el mínimo funcional.**
No se diseña en más detalle aquí para no ampliar el alcance de esta auditoría más allá de lo
pedido.

---

## 28. Preview (Bloque 27)

MVP evaluado: PDF e imágenes (mismo alcance que la Opción C híbrida de la sección 11 — imágenes
sin temporal, PDF vía visor externo con temporal). DOCX y formatos complejos quedan
explícitamente para una fase posterior — no hay ninguna librería de renderizado de DOCX instalada
ni evaluada aquí. El impacto de temporales para PDF (abierto externamente) está documentado en
las secciones 11 y 24 — no se repite aquí, se referencia.

---

## 29. Documentos y Evaluaciones (Bloque 28)

Confirmado explícitamente: la categoría `'evaluacion_adjunta'` ya existe en el `CHECK` del
esquema real (sección 2) — es decir, el esquema **ya anticipa** que un documento pueda
categorizarse como relacionado a una evaluación, sin que eso implique ninguna relación
estructural (no hay ninguna FK entre `documents` y `assessment_administrations` en el esquema, y
no se propone agregar una). **No se crea ninguna integración automática** entre importar un
documento y modificar una `assessment_administration` — ambos verticales permanecen
completamente desacoplados, exactamente como pide el encargo.

---

## 30. Documentos y cierre de proceso (Bloque 29)

Cerrar un proceso (`treatment_episodes.status = 'cerrado'`, vía `episode_closures`, Fase 11)
**nunca borra documentos** — ninguna función de `services::episode_closures` toca `documents` en
ningún punto (confirmado por ausencia total de referencias cruzadas en el código real). Los
documentos históricos de un proceso cerrado permanecen disponibles en modo lectura, igual que
Formulación/Evaluaciones de un proceso cerrado.

**Pregunta explícitamente abierta, según pide el encargo** (no es obvia, se pregunta en vez de
decidir): ¿debe impedirse agregar **nuevos** documentos a un proceso cerrado (mismo criterio que
Formulación, que bloquea nueva versión), o debe permitirse documentación administrativa
posterior al cierre (p. ej. un informe de alta que se termina de redactar y adjunta días después
del cierre formal, o correspondencia recibida después)? A diferencia de Formulación, donde el
encargo de Fase 15 pedía explícitamente bloquear esto, el encargo de esta fase no lo especifica —
se presenta como pregunta concreta en la sección 31, no se asume ninguna de las dos respuestas.

---

## 31. Documentos y paciente archivado (Bloque 30)

Precedentes reales auditados en el proyecto:

- **Bloquea incluso contenido "de continuación"** (una nueva versión cuenta como contenido
  nuevo): `safety_plans` (Fase 12, `require_editable_draft`) y `case_formulations`
  (Fase 15, `create_new_version` también bloqueado).
- **Solo bloquea la primera creación, permite seguir editando/gestionando lo ya existente**:
  `therapy_tasks`/`patient_prep_notes` (Fase 8), `payments` (Fase 7), `assessment_administrations`
  (Fase 13) — todos permiten editar un registro histórico de un paciente archivado, solo bloquean
  crear uno **nuevo**.

**Documentos no encaja limpiamente en ninguno de los dos precedentes sin decidir explícitamente
cuál analogía aplica**, y esto es exactamente la ambigüedad que el encargo pide reportar en vez de
resolver unilateralmente:

- Importar un documento **nuevo** es, sin ambigüedad, "crear contenido nuevo" → debería bloquearse
  para un paciente archivado, siguiendo la regla general del proyecto (ítem no controvertido).
- Pero: ¿"editar metadata" de un documento ya existente (cambiar su categoría o descripción) es
  más parecido a "editar un pago/evaluación histórica" (permitido hoy para archivados) o debería
  tratarse con el mismo criterio estricto de Formulación/Plan de Seguridad? El encargo de esta
  fase no lo especifica.
- ¿"Archivar/restaurar" un documento (soft delete, sección 13) para un paciente ya archivado
  debería permitirse? Ningún precedente exacto existe — archivar/restaurar contenido siempre se
  ha evaluado hasta ahora sobre pacientes activos.

**Se reporta esta ambigüedad explícitamente, según pide el encargo, en vez de asumir una
respuesta — ver pregunta concreta en la sección 31 (preguntas).**

---

## 32. Portabilidad (Bloque 31)

Revisada la arquitectura multiplataforma real (`docs/ARCHITECTURE.md` secciones 2/5/15.A, y
verificación directa de código en esta auditoría, sección 3): todo el filesystem de la
aplicación pasa por `tauri::path::PathResolver::app_data_dir()`, ya confirmado portable (sin
ningún hardcode de `/Users/`, `/home/`, `C:\`). El diseño de Documentos propuesto (sección 3)
reutiliza exactamente ese mismo mecanismo — `<app_data_dir>/vault/files/` — sin introducir
ninguna dependencia ni API específica de una sola plataforma. `aes-gcm`/`sha2`/`uuid`/`getrandom`
(todos los crates involucrados en el diseño recomendado) son Rust puro sin bindings nativos por
plataforma, ya compilando hoy para macOS/Windows (y compilables para iOS/iPadOS sin cambios,
aunque esa build no exista todavía — fuera de alcance de esta fase). **No se usa ninguna API
exclusiva de macOS en el diseño propuesto** (el único uso de una capacidad "nativa" del SO en todo
el proyecto es `keyring` para el keychain de Google Calendar, que esta fase no toca).

---

## 33. Sync futuro (Bloque 32) — no diseñado, solo no bloqueado

Sync **no pertenece a esta fase** (regla 6 de `CLAUDE.md` — Backup ≠ Sync ≠ Export, cada uno su
propia fase futura dedicada). Se verifica únicamente que el diseño de almacenamiento de archivos
propuesto **no cierra la puerta** a un E2EE futuro:

- **IDs estables**: `document_id` (UUID, generado una sola vez, nunca reasignado) — estable por
  diseño, útil como identificador de sincronización futuro sin cambios.
- **Ciphertext autocontenido**: con la Opción B (sección 7), cada archivo cifrado es
  independiente (su propia DEK envuelta, su propio nonce) — no depende de un estado global que
  un futuro mecanismo de sync tendría que coordinar entre dispositivos.
- **Versionado criptográfico**: `format_version` por archivo (sección 34) permite que dispositivos
  con versiones de app ligeramente distintas coexistan durante una migración de sync futura, sin
  que todos deban actualizarse atómicamente a la vez.
- **Metadata necesaria para sync**: no se diseña aquí — se señala solo que la separación ya
  existente (metadata en SQLCipher, contenido en `files/`) es compatible con cualquier estrategia
  de sync futura que decida sincronizar ambos por separado o de formas distintas.
- **Conflictos**: explícitamente fuera de alcance — ninguna lógica de resolución de conflictos se
  diseña en este documento. Se recuerda la regla permanente ya aprobada: nunca last-write-wins
  silencioso para datos clínicos en cualquier diseño de sincronización futuro.

**No se diseña ningún servidor, no se crean cuentas, no se implementa ninguna sincronización en
esta fase ni en este documento.**

---

## 34. Formato criptográfico versionado (Bloque 33)

Se propone un campo `format_version` (entero, empezando en `1`) que viva **en ambos lugares**, no
uno solo, con justificación:

- **En la fila de `documents`** (columna SQL nueva, si se aprueba la migración de la sección 26):
  permite a la aplicación decidir, antes de siquiera tocar el archivo físico, qué algoritmo/
  formato usar para descifrarlo — útil para mostrar un error claro ("este documento fue cifrado
  con una versión de formato que esta versión de la app no reconoce") sin intentar primero un
  descifrado condenado a fallar.
- **En el propio header del ciphertext** (los primeros bytes del archivo `.enc`): garantiza que
  la información de versión viaja siempre junto con el archivo, incluso en escenarios donde la
  fila de DB y el archivo físico se separan accidentalmente (un archivo copiado manualmente fuera
  de contexto, o un escenario de recuperación de emergencia donde solo se tiene el archivo
  físico) — es la misma razón de diseño por la que `vault.meta.json` guarda sus propios
  parámetros de Argon2id junto a cada DEK envuelto, en vez de asumirlos fijos globalmente.

Redundancia intencional, no un error — mismo principio de "defensa en profundidad" ya aplicado en
el propio `vault.meta.json` (`FORMAT_VERSION` ahí también existe, ver `security::vault_meta`).

---

## 35. Atomicidad DB + filesystem (Bloque 34)

SQLite y el filesystem no comparten una transacción real — mismo problema de fondo ya resuelto
para Backup/Restore (`backup::service`, patrón de staging + rename atómico + rescue dir) y
aplicado aquí por analogía directa (sección 11, flujo de importación):

1. Cifrar a un temporal en el mismo directorio de destino final.
2. Verificar (tamaño esperado, o descifrado de prueba si se quiere ser extremadamente
   conservador — probablemente innecesario, el AEAD ya autentica al leer).
3. `rename` atómico al nombre final.
4. Transacción SQL que inserta/actualiza la fila de `documents` — **después**, nunca antes, del
   paso 3.
5. Cleanup/reconciliación: ver reconciliador de arranque, punto siguiente.

**Reconciliador seguro al inicio** (análogo a `run_startup_recovery` de Backup, sección 3):

- **Ciphertext huérfano** (`files/<uuid>.enc` sin fila correspondiente en `documents`): se
  detecta comparando el contenido real de `files/` contra `SELECT storage_path FROM documents`.
  **No se borra automáticamente sin criterio explícito**, según pide el encargo — se recomienda,
  como mínimo para el V1, solo **registrar** su existencia (sin acción automática), dejando una
  purga manual explícita como posible mejora futura, nunca un borrado silencioso al arrancar.
- **Fila sin ciphertext** (`documents` referencia un `storage_path` que no existe físicamente):
  caso más delicado — representa una posible pérdida de datos real (el documento existía y ya no
  está). Se recomienda que la UI lo muestre explícitamente como "archivo no disponible" en vez de
  fallar silenciosamente u ocultarlo, para que la usuaria lo note y pueda decidir (¿restaurar
  desde un backup anterior?, ¿fue un error de sincronización manual del usuario copiando la
  carpeta de datos?). **Nunca se borra automáticamente la fila de DB tampoco** — sería destruir
  el único rastro de que el documento existió.

---

## 36. Tests propuestos (Bloque 35)

Diseño de la matriz de tests completa, para cuando se apruebe la implementación (no se escribe
ningún test en esta fase de auditoría):

**Criptografía** (nivel de una función pura de cifrado/descifrado, análoga a
`security::envelope` — 4-6 tests esperables, mismo estilo que los 4 tests ya existentes de
`envelope.rs`): roundtrip (cifrar → descifrar → contenido idéntico), clave incorrecta rechazada,
ciphertext alterado rechazado (un bit volteado), nonce incorrecto/reutilizado rechazado, archivo
vacío (si se decide permitirlo o rechazarlo explícitamente — sección 21), archivo del tamaño
límite exacto (borde), dos archivos con contenido idéntico producen ciphertext distinto (nonces
distintos — mismo test ya existente en `envelope.rs` adaptado a este contexto). Si se aprueba
streaming (sección 9): chunk truncado rechazado, chunks reordenados rechazados.

**Filesystem**: path traversal (confirmar que ningún input de usuario puede alcanzar una ruta
fuera de `files/`), limpieza de temporales (uno creado y luego confirmado ausente tras el flujo
normal), rename atómico (simular una interrupción a mitad de camino y confirmar que no queda un
archivo a medias visible como válido), huérfano detectado por el reconciliador, ciphertext
faltante detectado por el reconciliador.

**Repository** (SQL puro, sin reglas de negocio — mismo estilo que `repositories::formulations`):
CRUD de metadata, soft delete/restore, asociaciones paciente/proceso/sesión (si se aprueba
`episode_id`, sección 14).

**Service** (reglas de negocio — mismo estilo que `services::formulations`/`services::
assessments`): paciente archivado (según la decisión que resulte de la sección 31), proceso
cerrado (según la decisión de la sección 30), sesión de otro paciente rechazada (mismo patrón ya
aplicado en Pagos/Tareas — verificar `session.patient_id == document.patient_id` explícitamente
en el servicio), proceso de otro paciente rechazado (si aplica `episode_id`), límites de tamaño
(sección 21), nombres vacíos rechazados, MIME sin restricción confirmada explícitamente por un
test (para dejar constancia de que es una decisión deliberada, no un descuido).

**Backup** (si se aprueba tocar `backup/*`, sección 17): backup + restore con documentos
incluidos, ciphertext corrupto detectado en restore, documento faltante detectado en restore.

**Privacidad**: marcador ficticio `XYZFASE16DOCUMENTOSMARKER` insertado como contenido de un
documento de prueba y como `original_filename`/`description` de prueba, verificado ausente en:
logs (capturados con el mismo `CapturingLogger` ya usado en `vault_manager.rs`), rutas físicas
reales en disco (confirmar que ningún nombre de archivo en `files/` contiene el marcador),
Google Calendar (grep sobre `calendar/*`, igual que esta auditoría ya hizo), `localStorage`/
`sessionStorage` (inspección del frontend, igual que cada fase anterior), título de ventana.

---

## 37. Prueba manual futura (Bloque 36)

Diseñada para cuando exista implementación — **no ejecutable hoy, nada que probar todavía**:

1. Crear paciente ficticio, importar un PDF ficticio (sin datos reales, según regla permanente
   del proyecto).
2. Importar una imagen ficticia.
3. Verificar que ambos aparecen en el listado de "Documentos" con nombre/categoría/tamaño
   correctos.
4. Reiniciar la aplicación completa (cierre y reapertura del proceso).
5. Abrir ambos documentos (verificar que el contenido se recupera correctamente).
6. Bloquear y desbloquear el vault; confirmar que los documentos siguen accesibles después.
7. Exportar una copia de uno de los dos a una carpeta elegida; confirmar la advertencia de la UI
   (sección 12) y que el archivo exportado es legible fuera de la app.
8. Archivar uno de los documentos; confirmar que desaparece del listado activo y reaparece al
   restaurar.
9. Crear un backup completo (Ajustes → Respaldo, si Documentos ya se integró ahí — sección 17).
10. Mover/renombrar el vault de prueba (simular pérdida del original).
11. Restaurar desde el backup.
12. Confirmar que ambos documentos se recuperan íntegros (mismo contenido, mismo hash si aplica).
13. **Inspeccionar directamente el filesystem** (fuera de la app, con un explorador de archivos
    normal) y confirmar que los nombres dentro de `files/` son opacos (UUIDs), nunca el nombre
    real ni ningún dato identificable.
14. Intentar abrir uno de los archivos `.enc` directamente con un editor de texto/hexadecimal
    fuera de la app, y confirmar que no revela ningún contenido legible (ni el nombre, ni el
    texto del documento).

**Ningún dato clínico real en ningún paso** — regla permanente del proyecto, sin excepción.

---

## 38. Archivos que probablemente habría que crear/modificar (Bloque 25 del entregable)

**Nuevos** (bajo riesgo — vertical nueva, mismo patrón que Formulación/Evaluaciones):

- `src-tauri/src/repositories/documents.rs`
- `src-tauri/src/services/documents.rs`
- `src-tauri/src/commands/documents.rs`
- Un nuevo módulo de cifrado de archivo — ubicación a decidir: ¿`security::file_encryption`
  (dentro del módulo prohibido por defecto, porque conceptualmente es criptografía) o un módulo
  propio fuera de `security/*` que solo consuma primitivas ya expuestas? **Esta es en sí misma
  una pregunta abierta de diseño (sección 39)**, no una decisión tomada aquí.
- `src/features/documents/*.ts(x)` (frontend, mismo patrón que `src/features/formulation/`)

**Modificados, bajo riesgo** (mismo patrón mecánico ya aplicado en cada fase anterior):

- `src-tauri/src/repositories/mod.rs` / `services/mod.rs` / `commands/mod.rs` / `lib.rs`
  (registro de módulos y comandos).
- `src/features/patients/PatientDetailScreen.tsx` (activar la pestaña "Documentos").
- `src-tauri/src/db/migrations.rs` (si se aprueba una migración — sección 26).

**Modificados, requieren aprobación explícita antes de tocarse (archivos prohibidos por
defecto o de alto impacto)**:

- `src-tauri/src/security/session.rs` — para exponer el DEK del vault (o una operación de
  cifrar/descifrar que lo use internamente sin exponerlo directamente, ver sección 39) a la
  nueva capa de servicio de Documentos (sección 4.3).
- `src-tauri/src/backup/service.rs` — para incluir `files/` en `create_backup`/`restore_backup`
  (sección 17), **solo si se aprueba integrar Documentos con Backup/Restore en esta misma fase**
  (alternativa: dejarlo fuera del V1 de esta fase, backup normal seguiría respaldando solo la DB,
  y los documentos quedarían sin respaldo hasta una fase de integración dedicada — ver pregunta
  en sección 39).
- Posiblemente `src-tauri/src/lib.rs` más allá del registro mecánico de comandos, si se decide
  que la limpieza de temporales (sección 24) debe engancharse al ciclo de vida de la app.

**No se toca en ningún escenario evaluado**: `src-tauri/src/calendar/*`,
`src-tauri/src/db/connection.rs` (la apertura de `vault.db` en sí no cambia), ninguno de los
verticales existentes (`session_notes`, `payments`, `safety_plans`, `assessments`,
`episode_closures`, `formulations`, `goals`).

---

## 39. Dependencias / migraciones / riesgos / decisiones abiertas / recomendación / preguntas

### 39.1 Dependencias que serían necesarias

**Ninguna dependencia nueva es estrictamente necesaria para el diseño recomendado (Opción B,
AES-256-GCM, Camino 1 sin streaming, selector de archivos ya disponible).** Dependencias
**condicionales**, cada una una DECISIÓN PENDIENTE explícita, ninguna adoptada aquí:

| Dependencia | Solo si se decide... | Alternativa sin dependencia nueva |
|---|---|---|
| `aead` (declarada directamente, feature `stream`) | Streaming/chunked real (sección 9, Camino 2) | Límite de tamaño sin streaming (Camino 1) |
| `chacha20poly1305` | Preferir XChaCha20-Poly1305 sobre AES-256-GCM | Usar AES-256-GCM, ya disponible (recomendado, sección 8) |
| Una librería de renderizado de PDF en frontend | Preview de PDF dentro del WebView sin salir de la app | Abrir con la aplicación del sistema (`open`, ya disponible) |
| `unicode-normalization` | Normalizar Unicode de nombres de archivo (sección 20) | Aceptar la limitación, no normalizar |

### 39.2 Migraciones que serían necesarias

**Posiblemente ninguna, o una pequeña, dependiendo de las decisiones pendientes.** Columnas
candidatas a agregar a `documents` (todas aditivas, mismo patrón `ALTER TABLE ADD COLUMN` ya
usado 4 veces en el proyecto — V2/V4/V7/V8), **ninguna decidida aquí**:

- `episode_id TEXT REFERENCES treatment_episodes(id) ON DELETE SET NULL` (sección 14) — solo si
  se aprueba.
- Columna(s) para la DEK de archivo envuelta (p. ej. `wrapped_dek TEXT NOT NULL`, base64, sección
  7 Opción B) y su nonce (podría viajar concatenado en el mismo campo, o en uno separado) — **si
  se aprueba la Opción B**, esta migración es prácticamente inevitable, porque el esquema actual
  no tiene ningún lugar para guardar material de clave de archivo.
- `format_version INTEGER NOT NULL DEFAULT 1` (sección 34).
- Posiblemente reconsiderar `sha256_plaintext` (sección 19) — no necesariamente una migración
  (la columna ya existe y es `NOT NULL`), pero si se decide dejar de calcularla con un valor real
  por el riesgo de inferencia, habría que decidir qué valor poner ahí (¿hash del ciphertext en su
  lugar, reinterpretando la columna? ¿eso sí sería un cambio de significado que vale la pena
  documentar aunque no requiera `ALTER TABLE`?).

**Ninguna migración se crea en este documento** — según la instrucción explícita del encargo.

### 39.3 Riesgos

- **El riesgo más alto identificado en esta auditoría**: cualquier diseño de cifrado por archivo
  que reutilice el DEK del vault requiere tocar `security/session.rs` (sección 4.3) — un archivo
  prohibido por defecto, y el único lugar del proyecto donde vive el único secreto disponible sin
  volver a pedir la contraseña. Un error aquí tendría el mismo nivel de impacto que un error en
  el propio manejo del DEK de la base de datos.
- **Complejidad de Backup/Restore**: aunque el modelo de datos ya es genérico (sección 17), la
  lógica real de orquestación sí necesita extenderse, y un error ahí podría, en el peor caso,
  producir un backup que parece completo pero no lo es (documentos faltantes silenciosamente) —
  mitigado por el diseño de validación estricta de la sección 18, pero el riesgo de implementarlo
  mal existe.
- **Archivos grandes sin streaming (si se elige el Camino 1)**: un usuario podría intentar
  importar un archivo por encima del límite y encontrarse con un límite de producto, no un fallo
  de seguridad — riesgo de experiencia de usuario, no de datos, pero real.
- **Ambigüedad de paciente archivado (sección 31)** y **de proceso cerrado (sección 30)**: 
  implementar sin resolver estas preguntas explícitamente arriesga repetir el mismo tipo de
  inconsistencia que la Fase 11 tuvo que corregir retroactivamente para `status = 'archivado'`.

### 39.4 Decisiones abiertas (resumen, ver preguntas concretas más abajo)

Ver sección de preguntas (39.6) — cada una de las siguientes ya se presentó con su análisis en el
cuerpo del documento, se resumen aquí solo como índice: asociación con proceso (sección 14),
categorías/taxonomía (sección 15), integración con Backup/Restore en esta misma fase o después
(sección 17), hashing de plaintext (sección 19), límites exactos (sección 21), documentos en
proceso cerrado (sección 30), documentos y paciente archivado (sección 31), streaming vs. límite
simple (sección 9), AES-256-GCM vs. XChaCha20-Poly1305 (sección 8, con recomendación clara),
ubicación del nuevo módulo de cifrado (sección 38), sharding de directorios (sección 6).

### 39.5 Recomendación final de esta auditoría

El esquema `documents` de `SCHEMA_V1`, aunque nunca implementado, es un punto de partida sólido y
bien pensado — no hace falta descartarlo. La infraestructura criptográfica real del proyecto
(Argon2id + envelope encryption AES-256-GCM, sección 4) es exactamente el tipo de cimiento sobre
el que se puede construir cifrado por archivo sin improvisar nada nuevo — la Opción B (sección 7)
es, en la evaluación de esta auditoría, la extensión más natural y de menor riesgo de lo que ya
existe, y coincide con lo que la propia documentación del proyecto ya anticipaba desde la Fase 1.
El obstáculo real no es criptográfico ni de esquema: es que **el único secreto de sesión
reutilizable (el DEK del vault) hoy no es accesible fuera de `security::session`**, así que
cualquier camino hacia adelante empieza necesariamente por una modificación deliberada y acotada
de un archivo prohibido por defecto — exactamente el tipo de situación para la que existe la
regla de detenerse. Se recomienda **aprobar el diseño de la Opción B en principio**, y tratar la
extensión de `security::session.rs` como su propio paso explícito de aprobación dentro de la
implementación (un método nuevo, mínimo, auditable — no una reapertura general del módulo de
seguridad).

### 39.6 Preguntas concretas que requieren tu aprobación explícita

1. **Cifrado por archivo**: ¿apruebas la Opción B (DEK aleatoria por archivo + AES-256-GCM +
   envoltura con una clave derivada del DEK del vault, sección 7), incluyendo la modificación
   necesaria de `security/session.rs` para exponer esa capacidad de forma acotada (sección 4.3)?
   ¿O prefieres la Opción A, otra alternativa, o detenerte aquí hasta pensarlo más?
2. **Streaming**: ¿el V1 de esta fase se limita a un tamaño máximo sin streaming (Camino 1,
   sección 9), dejando el formato chunked (Camino 2) para una fase posterior si la necesidad
   real aparece? Si prefieres streaming desde el V1, ¿apruebas declarar `aead` con el feature
   `stream` como dependencia nueva?
3. **Tamaño máximo por archivo**: ¿qué límite exacto prefieres (la auditoría sugiere un rango de
   20-50 MB como punto de partida razonable, sección 21)?
4. **Visualización**: ¿apruebas la estrategia híbrida por MIME (imágenes sin temporal, PDF/otros
   con temporal + apertura externa vía `open`, sección 11), o prefieres una de las otras dos
   opciones puras?
5. **`episode_id`**: ¿agregar un vínculo opcional a proceso terapéutico (sección 14), o mantener
   solo `patient_id`/`session_id` como hoy para el V1 de esta fase?
6. **Backup/Restore**: ¿se integra Documentos con Backup/Restore dentro de esta misma fase
   (requiere modificar `backup/service.rs`, sección 17), o se deja explícitamente fuera del V1
   (los documentos quedarían sin respaldo hasta una fase de integración dedicada posterior)?
7. **Hash de plaintext**: ¿mantener `sha256_plaintext` con un valor real (aceptando el riesgo de
   inferencia descrito en la sección 19), dejar de calcularlo con un valor real, o reinterpretar
   la columna para el hash del ciphertext?
8. **Documentos en un proceso cerrado**: ¿se permite seguir agregando documentos a un proceso ya
   cerrado (documentación administrativa posterior), o se bloquea igual que Formulación
   (sección 30)?
9. **Documentos y paciente archivado**: ¿qué precedente aplica — el estricto de Formulación/Plan
   de Seguridad (bloquea toda creación de contenido nuevo, incluidas nuevas versiones/ediciones
   significativas) o el permisivo de Pagos/Tareas/Evaluaciones (bloquea solo la primera
   creación, permite seguir editando lo existente)? (sección 31)
10. **Categorías**: ¿las seis categorías existentes del `CHECK` (`informe`, `consentimiento`,
    `evaluacion_adjunta`, `receta`, `correspondencia`, `otro`) cubren razonablemente el alcance
    pedido, o falta explícitamente `'derivacion'` como categoría propia (sección 15)?
11. **Ubicación del nuevo código de cifrado de archivo**: ¿debería vivir dentro de
    `security/*` (coherente conceptualmente, pero amplía la superficie de un módulo ya sensible),
    o en un módulo nuevo fuera de `security/*` que solo consuma una operación mínima expuesta
    desde ahí (sección 38)?
12. **Sharding de directorios** (`files/<2-hex>/<uuid>.enc` vs. un único directorio plano,
    sección 6): ¿alguna preferencia, o se deja a criterio técnico en la fase de implementación?

---

## 40. Cierre

Esta ejecución fue exclusivamente **auditoría + planificación**, según lo pedido. No se instaló
ninguna dependencia, no se creó ninguna migración, no se modificó ningún esquema, no se tocó
`backup/*`/`security/*`/`calendar/*`, no se creó frontend, no se generó ninguna clave, no se hizo
ningún commit ni push relacionado con Documentos, y no se implementó absolutamente nada de la
funcionalidad de esta fase. `git status` sigue exactamente igual que en el Bloque 0 (verificado
antes de cerrar este documento, sección 41).

**Este documento se detiene aquí. No se avanza a Fase 16 funcional hasta recibir aprobación
explícita de las preguntas de la sección 39.6 (y de cualquier otro punto que corresponda
según tu revisión).**

## 41. Verificación final de que no se tocó nada

```
$ git status --porcelain=2
?? Plan-Fase-16-Documentos-cifrados-pendiente-de-aprobacion.md
```

Único archivo nuevo: este mismo documento. `git diff` sobre cualquier archivo existente: vacío.
Ningún commit creado. Ningún push realizado.
