# Documentos y adjuntos clínicos cifrados (Fase 16)

Documento técnico de la Fase 16. Complementa `docs/ARCHITECTURE.md` (tabla de fases, secciones 5/
7/10/16) y `Plan-Fase-16-Documentos-cifrados-pendiente-de-aprobacion.md` (auditoría de 40 bloques
que precedió esta fase, con la arquitectura recomendada y las 12 preguntas que la usuaria aprobó
explícitamente antes de implementar).

**Prioridad no negociable de esta fase, en este orden**: 1) no perder archivos; 2) no almacenar
plaintext inadvertidamente; 3) no romper el vault; 4) no debilitar la criptografía existente; 5)
backup/restore completo y verificable; 6) consistencia DB/filesystem; 7) funcionalidad; 8) UX.
Ninguna garantía de seguridad se simplificó para terminar antes.

## 1. Modelo: qué es un "documento" en Cuaderno Clínico

Un documento es un archivo arbitrario (informe, consentimiento firmado externamente, receta,
correspondencia, derivación, evaluación adjunta, u "otro") asociado siempre a un paciente, y
opcionalmente a un proceso terapéutico (`episode_id`) y/o una sesión (`session_id`) de ese mismo
paciente. Un documento **también puede ser puramente longitudinal** del paciente, sin proceso ni
sesión concretos — a diferencia de Formulación (Fase 15), donde `episode_id` es obligatorio a
nivel de servicio.

Categorías válidas (`services::documents::VALID_CATEGORIES`): `informe`, `consentimiento`,
`evaluacion_adjunta`, `receta`, `correspondencia`, `derivacion` (nueva en esta fase), `otro`.
Taxonomía deliberadamente pequeña y administrativa — nunca un catálogo diagnóstico, y no se
expande más allá de estos siete valores en esta fase.

El contenido de un documento es **inmutable una vez importado**: no existe ninguna función de
"reemplazar el archivo". Solo la metadata administrativa (categoría, descripción) es editable
después de crear el documento.

### Consentimientos: qué cubre esta fase y qué no

Esta fase permite guardar un consentimiento **firmado externamente** (en papel y escaneado, o
firmado en otra herramienta) como cualquier otro documento de categoría `consentimiento`.
**Explícitamente fuera de alcance**: captura de firma manuscrita dentro de la aplicación, firma
electrónica, firma electrónica avanzada/legal, certificados, o cualquier validación legal de
firma. Nada de esto se implementó ni se diseñó como efecto colateral de esta fase.

## 2. Auditoría de esquema previa: `documents` ya existía, sin usar, desde `SCHEMA_V1`

La tabla `documents` estaba definida completa desde la Fase 1.3 (columnas base: `id`, `patient_id`,
`category`, `original_filename`, `mime_type`, `size_bytes`, `sha256_plaintext`, `storage_path`,
`is_clinical`, `description`, `created_at`/`updated_at`/`deleted_at`), pero sin ningún
repositorio/servicio/comando/componente que la usara — igual situación que
`formulation_nodes`/`formulation_edges` antes de una futura Formulación Visual. Esta fase la activa
por primera vez.

## 3. `SCHEMA_V9`: por qué es una reconstrucción de tabla, no un `ALTER TABLE` simple

La aprobación exigió agregar `derivacion` al `CHECK` de `category`. SQLite **no permite modificar
un `CHECK` existente mediante `ALTER TABLE`** — la única forma correcta de cambiarlo es crear la
tabla con la definición correcta, copiar los datos, eliminar la tabla vieja y renombrar la nueva.
Esto **no es "editar una migración ya publicada"**: el texto de `SCHEMA_V1` permanece exactamente
igual en el código; `SCHEMA_V9` es una migración nueva, adicional, que se ejecuta después.

```sql
CREATE TABLE documents_v9 (
  id TEXT PRIMARY KEY,
  patient_id TEXT REFERENCES patients(id) ON DELETE SET NULL,
  episode_id TEXT REFERENCES treatment_episodes(id) ON DELETE SET NULL,
  session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  category TEXT CHECK (category IN
    ('informe','consentimiento','evaluacion_adjunta','receta','correspondencia','derivacion','otro')),
  original_filename TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
  sha256_plaintext TEXT NOT NULL CHECK (length(sha256_plaintext) = 64),
  storage_path TEXT NOT NULL UNIQUE,
  is_clinical INTEGER NOT NULL DEFAULT 1 CHECK (is_clinical IN (0,1)),
  description TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT,
  wrapped_file_dek TEXT,
  wrap_nonce TEXT,
  format_version INTEGER NOT NULL DEFAULT 1
);
INSERT INTO documents_v9 (id, patient_id, session_id, category, original_filename, mime_type, size_bytes,
    sha256_plaintext, storage_path, is_clinical, description, created_at, updated_at, deleted_at)
  SELECT id, patient_id, session_id, category, original_filename, mime_type, size_bytes,
    sha256_plaintext, storage_path, is_clinical, description, created_at, updated_at, deleted_at
  FROM documents;
DROP TABLE documents;
ALTER TABLE documents_v9 RENAME TO documents;
CREATE INDEX idx_documents_patient ON documents(patient_id);
CREATE INDEX idx_documents_episode ON documents(episode_id);
CREATE TRIGGER trg_documents_touch_updated_at
AFTER UPDATE ON documents
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE documents SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;
```

Al momento de auditar no existía ningún repositorio/servicio que usara `documents` — cero filas
reales en cualquier entorno — así que este era el momento más seguro posible para esta
reconstrucción; la migración además preserva explícitamente cualquier fila que pudiera existir
(verificado por `v9_migration_is_idempotent_and_preserves_v1_document_data`).

Columnas nuevas: `episode_id` (nulable, `ON DELETE SET NULL`, mismo patrón que
`sessions.episode_id`/`therapeutic_goals.episode_id`), `wrapped_file_dek`/`wrap_nonce` (la DEK del
archivo, envuelta — nunca en claro; nulables a nivel de esquema porque no hay valor por defecto
razonable, pero `repositories::documents::insert_document` siempre las exige como parámetros
obligatorios), y `format_version` (`NOT NULL DEFAULT 1`).

### `sha256_plaintext`: se resolvió el nombre heredado, no se reinterpretó

Se evaluó explícitamente convertir esta columna en un hash del ciphertext, y se descartó: **el
nombre de una columna debe conservar su significado**. Sigue siendo, literalmente, lo que dice su
nombre — un SHA-256 del contenido **descifrado**, calculado una sola vez al leer el archivo de
origen durante la importación. Sirve como verificación adicional de integridad al descifrar (ver
sección 8) — nunca se expone por IPC, nunca se usa para comparar documentos entre sí ni para
detectar duplicados.

No se agregó ninguna columna nueva de hash de ciphertext ("no agregar hash por costumbre"): el
mecanismo de Backup ya calcula y verifica un SHA-256 de cada archivo del contenedor
(`backup::manifest::BackupFileEntry.sha256`), documento incluido — no hacía falta duplicar esa
garantía en el esquema de `documents`.

No se agregó ningún `CHECK` de formato sobre `storage_path` en SQL — la prevención de path
traversal se hace exclusivamente en Rust (sección 6), porque `storage_path` siempre se genera
internamente, nunca a partir de un input directo de la usuaria.

## 4. Cifrado de sobre (envelope encryption): una DEK aleatoria por archivo

Cada documento tiene su propia DEK de 256 bits, generada aleatoriamente
(`services::document_crypto::generate_file_dek`, `getrandom`), **independiente de la del vault y
de la de cualquier otro documento**. El contenido se cifra con esa DEK usando AES-256-GCM
(`aes-gcm`, ya en uso en `security::envelope`). La DEK del archivo nunca se guarda en claro:
se envuelve (wrap) con AES-256-GCM usando una clave derivada de la DEK del vault, ya disponible en
la sesión desbloqueada.

**Explícitamente descartado**: cifrar todos los documentos directamente con la DEK del vault (un
documento comprometido no debe comprometer ningún otro, ni la clave del vault en sí);
guardar claves en SQLite en claro; guardar la clave junto al ciphertext sin protección propia;
criptografía propia — se reutilizan primitivas ya auditadas del proyecto (`aes-gcm`, `getrandom`,
`sha2`, `base64ct`), cero dependencias nuevas.

### `security::session.rs`: la única modificación dentro de `security/*`

La autorización explícita limitó cualquier cambio dentro de `security/*` a `session.rs`, en la
medida mínima necesaria para dar al módulo de Documentos una capacidad criptográfica segura —
nunca convirtiendo `security::*` en el módulo de Documentos. Se agregaron dos métodos a
`VaultSession`:

```rust
pub fn wrap_file_key(&self, file_key: &[u8; FILE_KEY_LEN]) -> Result<WrappedFileKey, WrapFileKeyError>
pub fn unwrap_file_key(&self, wrapped: &WrappedFileKey, version: KeyWrapVersion) -> Result<FileKey, UnwrapFileKeyError>
```

(`unwrap_file_key` recibió un segundo parámetro, `version: KeyWrapVersion`, en Fase 17 — ver
sección 4.1 más abajo. `wrap_file_key` mantiene su firma original.)

Esta es exactamente la forma preferida por la aprobación (`wrap_file_key`/`unwrap_file_key`, no
`get_vault_dek()`): **la DEK cruda del vault nunca cruza la frontera de `security::session`**.
`FileKey` (el tipo que envuelve una DEK de archivo ya desenvuelta) tiene `Debug` redactado, no
deriva `Clone`, y se zeroiza automáticamente al salir de scope (`Drop` + `zeroize`) — mismo nivel
de higiene que la DEK del vault.

Internamente, `wrap_file_key`/`unwrap_file_key` duplican un patrón pequeño y ya bien entendido de
AES-256-GCM (igual forma que `security::envelope`), **en vez de generalizar
`security/envelope.rs`** — deliberado: la autorización cubría modificar únicamente `session.rs`,
no un segundo archivo dentro de `security/*`. La pequeña duplicación es el costo explícito de
respetar ese límite exacto.

`services/document_crypto.rs` (donde vive toda la lógica específica de cifrado de archivos: cifrar
contenido, construir rutas opacas) está **deliberadamente fuera de `security/*`** — la única
capacidad que le pide a `security::VaultSession` es envolver/desenvolver una DEK de archivo; nunca
ve la DEK del vault en sí.

No fue necesario exponer la DEK cruda del vault en ningún punto — la condición de stop explícita
de la aprobación ("si exponer material de clave resulta técnicamente necesario, detenerse y
explicar por qué") nunca se activó.

### 4.1. CRYPTO-1 (hardening pre-RC, Fase 17): separación de dominio con HKDF-SHA256

Hasta aquí (Fase 16), `wrap_file_key`/`unwrap_file_key` usaban la DEK cruda del vault
**directamente** como clave AES-256-GCM para envolver la DEK de cada archivo — la misma DEK que,
sin ninguna separación de dominio, también se usa como clave raw de SQLCipher (`PRAGMA key`,
`db::connection::open_vault`). La auditoría pre-RC (`Auditoria-Pre-RC-Desktop-post-Fase-16.md`,
hallazgo CRYPTO-1) señaló esto como el único hallazgo B (bloqueante antes de RC) del dominio
criptográfico. Fase 17 lo corrige **solo para documentos nuevos**, sin migrar los ya existentes:

- **`documents.key_wrap_version`** (`SCHEMA_V10`, aditiva, `NOT NULL DEFAULT 1 CHECK (IN (1, 2))`)
  versiona el **esquema de envoltura de la DEK del archivo** — un concepto y una numeración
  deliberadamente independientes de `documents.format_version` (formato físico del ciphertext
  `.enc`, sección 5). Ambas columnas conviven en la misma fila sin relación entre sí.
- **`key_wrap_version = 1` ("legacy")**: el algoritmo exacto de Fase 16, sin ningún cambio — la DEK
  cruda del vault como clave AES-256-GCM. Se sigue soportando **indefinidamente**: `unwrap_file_key`
  nunca deja de aceptarlo. Toda fila creada antes de Fase 17 recibió este valor automáticamente al
  migrar (por `DEFAULT`), porque es literalmente el algoritmo con el que fue envuelta.
- **`key_wrap_version = 2` ("domain-separated")**: la clave de envoltura ya no es la DEK cruda del
  vault, sino una subclave de 256 bits derivada vía **HKDF-SHA256** (RFC 5869,
  `security::session::derive_file_wrap_key`, crate `hkdf` v0.12.4 — RustCrypto, la misma familia que
  `aes-gcm`/`sha2`/`argon2` ya en uso, compatible con el `digest 0.10` que ya usa `sha2` en el
  proyecto sin introducir una segunda versión de esa dependencia). `salt = None`: la entrada (la
  DEK del vault) ya es material de alta entropía generado por un CSPRNG, nunca una contraseña —
  RFC 5869 §3.1 no exige salt en ese caso. El `info` de la derivación es la constante
  `b"cuaderno-clinico:file-key-wrap:v1"` — su "v1" versiona la **derivación HKDF en sí** (un
  concepto distinto de `key_wrap_version = 2`, que versiona el **esquema de envoltura completo**;
  ambas numeraciones coinciden en texto por casualidad, no por relación). `wrap_file_key` produce
  **exclusivamente** este esquema desde Fase 17 — no existe ningún camino de producción para volver
  a generar un envoltorio `key_wrap_version = 1` nuevo.

**Sin migración automática, eager, lazy ni en background.** La columna `key_wrap_version` de cada
fila es la única fuente de verdad sobre qué esquema usar al leer — `unwrap_file_key` despacha
exclusivamente según ese valor, nunca hay autodetección ni un intento silencioso con el otro
esquema si uno falla (una versión desconocida, o un fallo de autenticación AES-GCM, son siempre un
error explícito). **Los documentos legacy de Fase 16 permanecen deliberadamente en
`key_wrap_version = 1` para preservar compatibilidad y evitar una migración criptográfica riesgosa
justo antes de RC.** Esto no es "una migración pendiente": `key_wrap_version = 1` es un estado
válido y soportado, no una etapa transitoria — se evaluaron explícitamente tres estrategias de
compatibilidad (migración eager al desbloquear, migración lazy al leer, compatibilidad dual
permanente) en `Plan-Fase-17-Hardening-Pre-RC-pendiente-de-aprobacion.md` §6-9, y se descartaron
las dos primeras por requerir de todas formas soporte de lectura dual indefinido (haciendo la
migración una complejidad neta, no una simplificación) y por introducir riesgo de interacción con
crash/auto-lock/multi-instancia a mitad de una re-envoltura. La remigración v1→v2 (una herramienta
manual futura, opcional) queda **explícitamente diferida a una fase futura independiente** — su
ausencia en documentos legacy es riesgo/deuda técnica residual **conocida y aceptada**, no un
descuido.

## 5. Formato de archivo versionado (`.enc`)

```text
[4 bytes: magic "CCD1"] [1 byte: format_version] [12 bytes: nonce] [ciphertext + tag AES-GCM]
```

`format_version = 1` para todo archivo de esta fase. Vive **redundantemente** en el propio header
físico y en la columna `documents.format_version` — mismo principio de defensa en profundidad que
`vault.meta.json::FORMAT_VERSION`: si un archivo `.enc` alguna vez se separa de su fila de
metadata, sigue sabiendo cómo debe descifrarse. **El header nunca lleva metadata clínica** — ni
nombre, ni categoría, ni ningún identificador de paciente/proceso/sesión.

Detección explícita de cada caso de fallo, en este orden (`document_crypto::decrypt_document`):

1. **Archivo truncado** (más corto que el header mínimo de 17 bytes) → `Truncated`.
2. **Magic inválido** (no es un archivo `.enc` de Cuaderno Clínico) → `InvalidMagic`.
3. **`format_version` desconocida** → `UnknownFormatVersion(v)`.
4. **Autenticación AES-GCM fallida** (clave incorrecta, o ciphertext/nonce manipulado o truncado
   dentro del cuerpo — indistinguibles por diseño, mismo criterio que
   `security::envelope::EnvelopeError::UnwrapFailed`) → `DecryptionFailed`.

### Tamaño máximo y streaming diferido

**50 MB por archivo** (`MAX_DOCUMENT_SIZE_BYTES`), comprobado con `std::fs::metadata` **antes** de
leer el archivo de origen completo a memoria. Explícitamente **sin streaming/chunks en esta fase**
— no se agregó la feature `stream` de `aead`. El único byte reservado para `format_version` en el
header existe precisamente para que una versión futura con chunks pueda introducirse como
`format_version = 2` sin reinterpretar nunca los archivos ya cifrados con `format_version = 1`.

## 6. Rutas físicas opacas — `vault/files/<2-hex>/<uuid>.enc`

`document_crypto::opaque_storage_path(document_id)` construye `files/<2-hex>/<uuid>.enc`, donde el
sharding usa los dos primeros caracteres hexadecimales (minúsculas) del propio UUID del documento
— solo evita miles de archivos en un directorio plano, no revela nada adicional. **El filesystem
nunca revela** paciente, RUT, nombre original, categoría, sesión, proceso ni diagnóstico. El
nombre original vive exclusivamente dentro de SQLCipher (`documents.original_filename`).

`storage_path` **siempre se genera internamente** a partir de un UUID nuevo — nunca a partir de un
input de la usuaria. `document_crypto::resolve_within_files_root` es una segunda capa de defensa
(no la única) que valida que un `storage_path` tenga exactamente la forma esperada antes de
resolverlo a una ruta absoluta dentro de `vault/files/`: rechaza explícitamente `..`, rutas
absolutas disfrazadas de shard, shards que no son 2 hex minúsculas, nombres que no son un UUID
válido, extensión distinta de `.enc`, y un shard que no coincide con el propio UUID del archivo —
cubierto por 8 tests dedicados de path traversal.

## 7. Importación: sin copia plaintext intermedia, atómica, sin filas huérfanas

`services::documents::create_document` (recibe `&VaultSession` + `files_root: &Path`, mismo patrón
ya establecido por `backup::service::create_backup`/`restore_backup`):

1. Valida paciente (existe, no archivado), proceso (existe, del mismo paciente, sin rechazar
   `cerrado` — sección 9) y sesión (existe, del mismo paciente, sin conflicto con el proceso
   indicado — sección "combinaciones contradictorias" más abajo) **dentro de la misma transacción
   de lectura**.
2. Valida el archivo de origen: existe, no está vacío, no supera 50 MB — comprobado con
   `fs::metadata`, sin leer el contenido todavía.
3. **Lee** el archivo de origen elegido por la usuaria (que puede estar en plano porque le
   pertenece a ella, fuera del vault) y calcula su SHA-256.
4. Genera una DEK aleatoria nueva, cifra el contenido en memoria, **zeroiza el buffer de
   plaintext inmediatamente después**.
5. Envuelve la DEK del archivo con `session.wrap_file_key`, zeroiza la DEK del archivo en claro.
6. Escribe el ciphertext a un archivo temporal (`<uuid>.enc.tmp`) dentro del mismo directorio de
   shard destino, y lo **renombra atómicamente** (`std::fs::rename`) a su ruta final `.enc`.
7. Solo entonces inserta la fila en `documents` — **el `INSERT` es siempre el último paso**.

**Cuaderno Clínico nunca crea una copia temporal en plano durante la importación** — verificado
explícitamente por `creates_a_document_and_persists_it_encrypted_on_disk`, que lee los bytes
físicos del `.enc` resultante y confirma que el plaintext original no aparece en ellos.

Si el `INSERT` falla después de que el `rename` ya tuvo éxito (por ejemplo, disco lleno), se
intenta limpiar el ciphertext recién escrito antes de propagar el error — con certeza total de que
nada más pudo haber llegado a referenciar ese archivo todavía, porque se acaba de crear en esa
misma llamada. Esto es distinto de un huérfano descubierto más tarde por el reconciliador (sección
10), que nunca se borra automáticamente sin ese mismo nivel de certeza.

### Combinaciones contradictorias de proceso/sesión

Si se informan tanto `episode_id` como `session_id`, y la sesión indicada ya pertenece a un
proceso distinto del indicado, se rechaza (`SessionEpisodeMismatch`) — nunca se permite una
asociación contradictoria. Una sesión sin proceso asignado nunca es contradictoria con ningún
`episode_id` informado. Nunca se permiten asociaciones cruzadas entre pacientes distintos
(`EpisodePatientMismatch`/`SessionPatientMismatch`), verificado con datos reales de dos pacientes
de prueba.

## 8. Visualización y apertura: híbrida por tipo MIME, sin plaintext persistente

`services::documents::get_document_content` descifra el contenido completo, valida `format_version`
antes de intentar nada más, y **revalida integridad** recomputando el SHA-256 del contenido
descifrado contra `sha256_plaintext` — detecta, por ejemplo, un backup restaurado de forma
inconsistente (fila de `documents` de un origen, ciphertext de otro). Esta función nunca escribe a
disco ni cachea nada por sí sola: el llamador decide qué hacer con el resultado.

- **Imágenes** (`get_document_data_url`): se descifran en memoria y se devuelven como `data:` URL
  — **cero huella en disco**, el `Vec<u8>` de plaintext se zeroiza inmediatamente después de
  codificar a base64.
- **PDF/DOCX/otros** (`open_document_externally`): se descifran a un temporal de nombre opaco
  (UUID nuevo, prefijo reconocible `cuaderno-clinico-doc-`) en el directorio temporal del sistema
  (`std::env::temp_dir()`, **nunca dentro de `vault/files/`**), registrado en
  `DocumentTempRegistry`, y se abre con la aplicación por defecto del sistema operativo (crate
  `open`, ya en uso para el flujo OAuth de Google Calendar — sin dependencia nueva).

El MIME nunca se confía ciegamente a la extensión ni al valor enviado desde el frontend para
decidir ninguna ruta física ni ningún comando a ejecutar — solo decide, en el frontend, cuál de
los dos comandos anteriores invocar.

### Temporales descifrados: ciclo de vida completo

`services::document_temp::DocumentTempRegistry` (fuera de `security/*` — no necesita ningún
secreto del vault, solo rastrea rutas): `Mutex<HashSet<PathBuf>>` gestionado como estado de Tauri.
`allocate(extension_hint)` genera un nombre opaco (rechaza cualquier hint de extensión con
separadores o caracteres no alfanuméricos) y lo registra; `cleanup_all()` borra y desregistra todo
lo pendiente, best-effort (un archivo ya inexistente o aún abierto en la aplicación externa nunca
hace fallar la limpieza de los demás).

Limpieza conectada en tres puntos:

- **Bloqueo manual del vault** (`commands::vault::lock_vault`).
- **Bloqueo automático por inactividad** (`lib.rs`, el mismo tick loop que ya revisaba
  `tick_auto_lock()` cada 10 segundos).
- **Cierre de la aplicación** (`lib.rs`, hook nuevo en `RunEvent::Exit` — cambio de `.run(context)`
  a `.build(context)?.run(|app_handle, event| ...)`, best-effort, nunca bloquea el cierre).

**Barrido de arranque** (`sweep_stale_temp_files`, llamado en `lib.rs` antes de crear
`VaultSession`): si un crash impidió la limpieza normal, no depende del registro en memoria (que
se pierde entre reinicios) — busca directamente, en el directorio temporal del sistema, cualquier
archivo con el prefijo reconocible y lo borra, sin tocar ningún archivo ajeno.

**Limitación honesta, documentada sin edulcorar**: "borrar" un archivo no garantiza borrado físico
seguro en un SSD (wear leveling, remapeo de bloques). Esta fase no implementa ni promete borrado
seguro a nivel de disco — solo elimina la entrada del sistema de archivos tan pronto como es
razonablemente posible.

## 9. Proceso cerrado vs. paciente archivado — dos reglas deliberadamente distintas

- **Proceso cerrado SÍ puede recibir documentos nuevos** — a propósito, distinto de Formulación
  (Fase 15). `check_episode_belongs_to_patient` (nueva, separada de
  `treatment_episodes::check_episode_assignable`) valida que el proceso exista, pertenezca al
  mismo paciente y no esté archivado, pero **deliberadamente no rechaza `status = 'cerrado'`** —
  documentación administrativa o clínica recibida después del cierre (un informe, una derivación,
  un consentimiento escaneado) no reabre el proceso ni modifica su cierre, formulación o sesiones.
  Verificado con `allows_a_document_for_a_closed_episode_unlike_formulation`.
- **Paciente archivado NO puede recibir documentos nuevos** — mismo criterio que
  Pagos/Tareas/Evaluaciones: `create_document` rechaza (`PatientArchived`) si
  `patient.deleted_at` está presente. Los documentos ya existentes permanecen completamente
  visibles (listar, abrir, exportar, editar metadata, archivar/restaurar) — solo se bloquea crear
  contenido nuevo. Restaurar al paciente vuelve a habilitar la importación.

## 10. Consistencia DB/filesystem: diagnóstico conservador, nunca borrado automático

`services::documents::check_consistency` compara `documents.storage_path` (activos y archivados —
un documento archivado conserva su ciphertext) contra lo realmente presente en `vault/files/`, y
devuelve un `DocumentConsistencyReport` con dos contadores:

- **`missing_ciphertext_count`**: filas de `documents` sin archivo físico correspondiente —
  posible pérdida de datos real. **Nunca se borra la fila automáticamente.**
- **`orphan_ciphertext_count`**: archivos bajo `vault/files/` sin fila de `documents`
  correspondiente — podrían ser recuperables (por ejemplo, de una importación interrumpida antes
  del `INSERT`, o restos de una limpieza fallida). **Nunca se borran automáticamente.**

Este comando es puramente diagnóstico — no repara ni elimina nada por sí solo. La decisión de qué
hacer con un huérfano o una fila sin archivo queda para una intervención manual futura o una fase
posterior de reconciliación activa, si se decide implementarla.

## 11. UI: pestaña "Documentos"

`src/features/documents/` convierte el placeholder anterior en funcional:
`DocumentsTab.tsx` (listado activos/archivados con el mismo patrón de toggle de `PaymentsTab`),
`AddDocumentModal` (selector de archivo nativo + categoría + descripción + asociación opcional a
proceso/sesión), `EditMetadataModal`, `ImagePreviewModal` (usa `getDataUrl` para imágenes).

Cada fila muestra: nombre original, categoría, fecha, tamaño, descripción cuando aplica, y
asociación a proceso/sesión cuando existe. **Nunca se muestra** `storage_path`, la clave envuelta,
el nonce, el hash, el UUID físico ni ningún detalle criptográfico — estructuralmente imposible por
IPC, ya que `DocumentSummary` (el único tipo que cruza IPC) no tiene esos campos, y `Document` (la
fila completa) ni siquiera deriva `Serialize`.

Acciones por documento: Abrir (según MIME: imagen en modal in-app, o aplicación externa),
Exportar copia, Editar metadata, Archivar/Restaurar. El selector de archivo nativo reutiliza
`tauri-plugin-dialog` (ya usado por Backup, Fase 10) — **sin dependencia ni plugin nuevo**.
Drag & drop no se implementó: se evaluó como no obligatorio frente al selector de archivos
correcto, sin ampliar el alcance de la fase.

### "Exportar copia" — explícitamente distinto de "Abrir"

La usuaria elige un destino explícito y consciente (diálogo nativo "Guardar como…"), y la copia
exportada puede quedar en plano fuera del vault **porque es una acción consciente y explícita de
la usuaria**, nunca automática. La UI muestra una advertencia breve, no alarmista, de que la copia
exportada queda fuera del almacenamiento cifrado de Cuaderno Clínico.

## 12. Backup / Restore — obligatorio en esta fase

Un backup creado después de esta fase **incluye** todos los ciphertexts de documentos activos y
archivados. `backup::service::create_backup` enumera `documents::list_all_storage_paths` **dentro
de la misma llamada a `with_connection` que ejecuta `VACUUM INTO`**, para que la lista de
documentos refleje exactamente el mismo instante que el propio snapshot de `vault.db` — nunca una
lista más nueva o más vieja. Cada ciphertext se copia **tal cual** al área de scratch (nunca se
descifra para respaldar) y se agrega al manifest con su propio `path`/`size_bytes`/`sha256`, en la
misma estructura relativa `files/<shard>/<uuid>.enc` dentro del contenedor.

**Si una fila de `documents` referencia un archivo que no existe físicamente, `create_backup`
rechaza el backup completo** (`BackupError::MissingDocumentFile`) — nunca produce silenciosamente
un backup incompleto.

`restore_backup` no requirió ningún cambio: `BackupManifest.files`/`archive::write_container`/
`extract_container` ya eran completamente genéricos sobre un número arbitrario de entradas con
ruta anidada (incluida la extracción segura vía `enclosed_name()`, que ya rechaza traversal), así
que el mismo bucle de validación por entrada que ya cubría `vault.db`/`vault.meta.json` cubre
`files/*` sin cambios — validación de manifest, hash y tamaño por archivo, ausencia de traversal,
y el mismo swap atómico de staging descrito en `docs/backup-restore.md`.

### Compatibilidad con backups antiguos sin `files/`

Un backup creado antes de esta fase no tiene ninguna entrada `files/` — sigue siendo restaurable
sin cambios: el bucle de restauración solo exige los archivos que el propio manifest de **ese**
backup declara, nunca una lista fija. La ausencia de `files/` nunca se interpreta como corrupción
— se decide exclusivamente por lo que el manifest de ese backup en particular declara, igual que
`docs/backup-restore.md` ya documentaba como diseño reservado desde la Fase 10.

## 13. Privacidad

- Google Calendar **nunca** se toca en esta fase — cero referencias a `documents`/`category`/
  `description`/`storage_path`/`mime_type` dentro de `src-tauri/src/calendar/` ni de
  `src/features/agenda/`, y cero importaciones de `calendar::*` desde ningún archivo de este
  vertical.
- Sin nube, sin OCR, sin IA, sin analytics ni telemetría sobre ningún documento — todo el
  procesamiento de contenido ocurre localmente.
- **Sin logging de contenido clínico**: ninguno de los archivos nuevos (`document_crypto.rs`,
  `document_temp.rs`, `repositories::documents`, `services::documents`, `commands::documents`)
  contiene una sola llamada a `log::`/`println!`/`dbg!`/`eprintln!` que incluya nombre original,
  contenido, descripción, nombre de paciente, claves, DEKs, claves envueltas, plaintext o la ruta
  de origen elegida por la usuaria.
- Auditoría con marcador ficticio `XYZFASE16DOCUMENTOSMARKER`: verificado programáticamente
  (`document_content_is_unrecoverable_directly_from_the_vault_meta_or_db_without_unlocking`) que el
  marcador nunca aparece en los bytes físicos del `.enc` correspondiente.

## 14. Arquitectura

```
React (features/documents/DocumentsTab.tsx, pestaña de la ficha del paciente)
   │  invoke('create_document', ...), invoke('get_document_data_url', ...), etc.
   │  + diálogos nativos de archivo (tauri-plugin-dialog): pickSourceFile()/pickExportDestination()
   ▼
commands::documents   (10 comandos Tauri — capa fina, resuelve files_root vía AppHandle)
   ▼
services::documents   (reglas de negocio: asociaciones, proceso cerrado/paciente archivado,
   │                    orquestación atómica de importación)      ▼
   │                                                    services::document_crypto (cifrado puro,
   │                                                    rutas físicas opacas — sin Tauri, sin SQL)
   ▼
repositories::documents  (SQL puro sobre `documents`)
   ▼
security::VaultSession::wrap_file_key / unwrap_file_key(_, KeyWrapVersion)   (única capacidad
   │                                    expuesta desde security/* — nunca la DEK cruda del vault)
   ▼
SQLCipher (vault.db, metadata) + vault/files/<shard>/<uuid>.enc (ciphertext)
```

`services::document_temp::DocumentTempRegistry` vive como estado de Tauri paralelo, sin relación
con la cadena SQL — solo rastrea temporales descifrados de visualización externa (sección 8).

## 15. Tests

50 en `db::migrations` (incluye 5 nuevos de `SCHEMA_V9`: columnas presentes desde el arranque,
`format_version` por defecto en `1`, idempotencia y preservación de datos anteriores a V9, vínculo
a un proceso terapéutico, múltiples documentos por proceso), 13 en `security::session` (6 nuevos de
`wrap_file_key`/`unwrap_file_key`: roundtrip, clave incorrecta, nonce manipulado, vault bloqueado,
DEKs independientes por archivo), 22 en `services::document_crypto` (roundtrip, clave/nonce
incorrectos, truncamiento, cuerpo truncado, vacío, límite exacto de 50 MB, sobre el límite, dos
plaintexts idénticos → ciphertexts distintos, DEKs independientes, `format_version` desconocida,
magic inválido, y 8 de `resolve_within_files_root` cubriendo traversal/rutas absolutas/shard
inválido/UUID inválido/extensión faltante/mayúsculas/shard no correspondiente al UUID), 7 en
`services::document_temp` (asignación, sin hint de extensión, hint malicioso rechazado, limpieza
completa, limpieza tolera archivo ya ausente, `remove` puntual, barrido de arranque respeta
archivos ajenos), 10 en `repositories::documents` (CRUD, asociación a proceso/sesión, documento
longitudinal, listado excluye archivados y nunca lleva columnas criptográficas, archivar/restaurar,
editar metadata nunca toca `storage_path`, `list_all_storage_paths` incluye archivados), 18 en
`services::documents` (creación básica, roundtrip vía `get_document_content`, paciente
inexistente/archivado, proceso de otro paciente, proceso cerrado permite documento, sesión de otro
paciente, combinación sesión/proceso contradictoria, sesión sin proceso nunca es contradictoria,
categoría inválida, categoría `derivacion` nueva aceptada, archivo de origen inexistente/vacío/
sobre el límite, editar metadata nunca toca contenido, archivar/restaurar conserva el ciphertext,
dos documentos con mismo contenido usan cifrado independiente, marcador de privacidad nunca en el
ciphertext físico), y 9 nuevos en `backup::service` (backup sin documentos, con uno, con varios,
con uno archivado, restore completo recupera el contenido descifrable, ciphertext faltante rechaza
el backup, ciphertext corrupto se detecta al restaurar, backup antiguo sin `files/` sigue siendo
restaurable, traversal malicioso en una entrada de documento se rechaza).

`cargo clippy --all-targets`: 0 advertencias. `cargo build`: sin errores. `npm run build`: sin
errores. `npm run lint`: sin errores nuevos (advertencias preexistentes de la misma categoría ya
presente desde fases anteriores).

### Fase 17 (CRYPTO-1): tests agregados

6 nuevos en `db::migrations` (columna presente desde el arranque, filas V9 existentes reciben
`key_wrap_version = 1` al migrar a V10 sin tocar `wrapped_file_dek`, valor explícito `2` en una fila
nueva, `DEFAULT` sigue siendo `1` si no se especifica, `CHECK` rechaza cualquier valor fuera de
`{1, 2}`, idempotencia y preservación de datos anteriores a V10). 12 nuevos en `security::session`
(roundtrip `DomainSeparated`, `wrap_file_key` produce siempre ese esquema, un envoltorio v2 no
puede desenvolverse como v1 y viceversa, un **fixture legacy congelado** —bytes calculados una sola
vez con un programa `aes-gcm` independiente, nunca con el código de este módulo— sigue
desenvolviéndose correctamente como v1, HKDF es determinista y distinto tanto de la DEK del vault
como entre DEKs distintas, conversión `KeyWrapVersion ↔ i64` en ambos sentidos y rechazo de valores
desconocidos, wrap/unwrap bloqueados por igual sin importar la versión). 2 nuevos en
`repositories::documents` (persistencia exacta de `key_wrap_version` para ambos valores
soportados). 5 nuevos en `services::documents` (un documento `key_wrap_version = 1` construido con
el algoritmo legacy real sigue abriendo; un documento nuevo queda en `2` y abre igual;
ambos —v1 y v2— siguen siendo legibles después de un cambio de contraseña y después de una
recuperación por código). 1 nuevo en `backup::service` (un backup con documentos de ambos esquemas
mezclados se restaura sin reenvolver ninguno, y ambos decodifican correctamente tras restaurar).

## 16. Limitaciones conocidas

- Sin streaming/chunks — límite fijo de 50 MB por archivo en esta versión del formato (sección 5).
- Sin reconciliación activa de huérfanos/faltantes — solo diagnóstico manual
  (`check_document_consistency`), nunca reparación automática (sección 10).
- Sin borrado físico seguro garantizado a nivel de disco (SSD) para temporales descifrados — solo
  eliminación del sistema de archivos tan pronto como es razonable (sección 8).
- Sin hard delete/purge de documentos — solo soft delete (archivar/restaurar); un borrado físico
  definitivo queda fuera de alcance salvo que se apruebe explícitamente en una fase futura.
- Sin firma electrónica de ningún tipo — solo almacenamiento de un consentimiento ya firmado
  externamente (sección 1).
- Sin drag & drop — se evaluó no obligatorio frente al selector de archivos correcto.
- **Documentos legacy sin separación de dominio criptográfica (CRYPTO-1, Fase 17).** Todo
  documento creado antes de Fase 17 permanece en `key_wrap_version = 1`: su DEK de archivo sigue
  envuelta con la DEK cruda del vault directamente, sin la separación de dominio HKDF que sí
  reciben los documentos nuevos. Es una decisión deliberada (sección 4.1) para evitar una
  migración criptográfica riesgosa justo antes de RC, no un descuido — pero es riesgo/deuda técnica
  residual real y conocida hasta que una fase futura explícita decida remigrar esos documentos.
