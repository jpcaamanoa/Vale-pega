# Informe de cierre — Fase 16: Documentos y adjuntos clínicos cifrados

Sigue el formato obligatorio de `CLAUDE.md` sección 10, más las secciones específicas pedidas en
el mensaje de aprobación de 45 bloques que precedió esta fase (Bloque 44). Documento único y
autocontenido — pensado para poder entregarse completo a un tercero sin contexto adicional de la
conversación.

## 1. Baseline

Verificado antes de tocar código:

- `git rev-parse HEAD` → `2c179ca` — `docs: agregar auditoría y plan pendiente de aprobación de
  Fase 16` (commit puramente documental: `Plan-Fase-16-Documentos-cifrados-pendiente-de-
  aprobacion.md`, sin cambios de código).
- `git branch --show-current` → `claude/cuaderno-clinico-desktop-udijjq`.
- `origin/claude/cuaderno-clinico-desktop-udijjq` → `2c179ca` — sin divergencia.
- Regresión inicial: 695/695 tests Rust en verde, `cargo clippy --release --all-targets` sin
  advertencias, `cargo build --release` limpio, `npm run build` limpio, `npm run lint` 24
  warnings/0 errors (baseline heredado de la Fase 15), `git diff --check` limpio.

## 2. Auditoría previa (Bloque A/B, ya cerrada antes de este cierre)

El diseño completo (schema, filesystem, cripto, threat model, alternativas evaluadas,
arquitectura recomendada, formato de archivo, manejo de claves, archivos grandes, import/open/
export, temporales, soft delete, asociaciones, backup/restore, consistencia, privacidad, logging,
lock/unlock, portabilidad, tests, plan de prueba manual, archivos a tocar, dependencias,
migraciones, riesgos, decisiones abiertas, recomendación, y 12 preguntas explícitas) se presentó
en `Plan-Fase-16-Documentos-cifrados-pendiente-de-aprobacion.md` (commit `2c179ca`, sin código).
La usuaria respondió con un mensaje de 45 bloques aprobando explícitamente la implementación
completa, con restricciones muy específicas y vinculantes — resumidas en las secciones siguientes
allí donde corresponde a cada decisión concreta tomada.

## 3. Prioridad no negociable declarada, y cómo se respetó

Orden explícito: 1) no perder archivos; 2) no almacenar plaintext inadvertidamente; 3) no romper
el vault; 4) no debilitar la criptografía existente; 5) backup/restore completo y verificable; 6)
consistencia DB/filesystem; 7) funcionalidad; 8) UX. Ninguna garantía de seguridad se simplificó
para terminar antes — en particular: la importación nunca crea una copia plaintext intermedia
(verificado leyendo bytes físicos, sección 9), `create_backup` rechaza el respaldo completo ante
cualquier ciphertext faltante en vez de producir uno incompleto (sección 15), y el reconciliador de
consistencia nunca borra nada automáticamente (sección 13).

## 4. Arquitectura criptográfica final

Envelope encryption con **una DEK aleatoria de 256 bits e independiente por archivo**
(`services::document_crypto::generate_file_dek`, `getrandom`), cifrada con AES-256-GCM. La DEK del
archivo **nunca se almacena en claro**: se envuelve con AES-256-GCM usando una clave derivada de
la DEK del vault, vía dos métodos nuevos y mínimos en `security::VaultSession`:

```rust
pub fn wrap_file_key(&self, file_key: &[u8; FILE_KEY_LEN]) -> Result<WrappedFileKey, WrapFileKeyError>
pub fn unwrap_file_key(&self, wrapped: &WrappedFileKey) -> Result<FileKey, UnwrapFileKeyError>
```

Exactamente la forma preferida por la aprobación (nunca `get_vault_dek()`) — **la DEK cruda del
vault nunca cruza la frontera de `security::session`**. No fue necesario exponer material de clave
sin encapsular en ningún punto; la condición de stop explícita de la aprobación para ese caso nunca
se activó. `FileKey` tiene `Debug` redactado, no deriva `Clone`, y se zeroiza automáticamente al
salir de scope (`Drop` + `zeroize`), igual nivel de higiene que la DEK del vault.

**Explícitamente descartado, tal como exigía la aprobación**: cifrar todos los documentos con la
DEK del vault directamente; guardar claves en SQLite en claro; guardar la clave junto al
ciphertext sin protección propia; criptografía propia. Se reutilizan únicamente primitivas ya
auditadas del proyecto (`aes-gcm`, `getrandom`, `sha2`, `base64ct`) — cero dependencias nuevas.

## 5. Cambios a `security/*` — alcance respetado exactamente

Autorización explícita: modificar `security/session.rs` únicamente, en la medida mínima necesaria,
sin convertir `security/*` en el módulo de Documentos. Cumplido: el único archivo tocado dentro de
`security/*` fue `session.rs` (los dos métodos de la sección 4, más los tipos
`FileKey`/`WrappedFileKey`/`WrapFileKeyError`/`UnwrapFileKeyError` y las constantes
`FILE_KEY_LEN`/`FILE_KEY_WRAP_NONCE_LEN`) y `mod.rs` (solo para reexportar esos tipos nuevos hacia
afuera del módulo). **No se tocó `security/envelope.rs`** — se evaluó generalizarlo y se decidió
en cambio duplicar un patrón pequeño y ya bien entendido de AES-256-GCM directamente en
`session.rs`, precisamente para no tener que tocar un segundo archivo dentro de `security/*` más
allá de lo autorizado. Toda la lógica específica de cifrado de archivos (generar la DEK del
archivo, cifrar/descifrar contenido, construir rutas físicas opacas) vive en
`services::document_crypto`, fuera de `security/*`, tal como exigía la aprobación.

## 6. Migración `SCHEMA_V9`

Reconstrucción completa de tabla (`CREATE TABLE documents_v9` → `INSERT ... SELECT` → `DROP TABLE`
→ `ALTER TABLE ... RENAME TO`), **no** un `ALTER TABLE` simple, porque SQLite no permite modificar
un `CHECK` existente vía `ALTER TABLE` y la aprobación exigía agregar `derivacion` al `CHECK` de
`category` ya definido en `SCHEMA_V1`. Esto **no es editar una migración ya publicada** —
`SCHEMA_V1` permanece exactamente igual en el código; `SCHEMA_V9` es una migración nueva y
adicional. Se confirmó, antes de crearla, que no existía ninguna fila real de `documents` en
ningún entorno (cero repositorio/servicio/comando la usaba hasta esta fase) — el momento más
seguro posible para esta reconstrucción, y la migración además preserva explícitamente cualquier
fila que pudiera existir (verificado con test dedicado).

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
-- INSERT...SELECT preserva cualquier fila V1 existente, DROP + RENAME + índices/trigger nuevos.
```

Sin columnas redundantes, tal como exigía la aprobación: solo `episode_id`,
`wrapped_file_dek`/`wrap_nonce` (obligatorias a nivel de servicio pese a ser nulables en el
esquema, mismo criterio ya usado por `episode_id` de `case_formulations` en la Fase 15), y
`format_version`.

## 7. `sha256_plaintext` — decisión final documentada

Se evaluó explícitamente reinterpretarla como hash del ciphertext, y se descartó: **el nombre de
una columna debe conservar su significado**. Sigue siendo, literalmente, lo que dice su nombre —
un SHA-256 del contenido **descifrado**, calculado al importar, usado como verificación adicional
de integridad al abrir/exportar un documento (`get_document_content` recompara el hash tras
descifrar). No se agregó ninguna columna nueva de hash de ciphertext: el mecanismo de Backup ya
calcula y verifica un SHA-256 de cada archivo del contenedor, documentos incluidos — no hacía
falta duplicar esa garantía en el esquema de `documents` ("no agregar hash por costumbre",
instrucción explícita de la aprobación).

## 8. Formato de archivo versionado (`.enc`)

```text
[4 bytes: magic "CCD1"] [1 byte: format_version] [12 bytes: nonce] [ciphertext + tag AES-GCM]
```

`format_version = 1`, redundante a propósito con la columna homónima de la fila (defensa en
profundidad, mismo principio que `vault.meta.json::FORMAT_VERSION`). El header nunca lleva
metadata clínica. Detecta explícitamente, en este orden: archivo truncado, magic inválido,
`format_version` desconocida, y fallo de autenticación AES-GCM (clave incorrecta o ciphertext/
nonce manipulado — indistinguibles por diseño). Límite de **50 MB por archivo**, comprobado con
`fs::metadata` antes de leer el archivo completo — **sin streaming/chunks en esta fase**, sin
agregar la feature `stream` de `aead`, tal como exigía la aprobación. El único byte reservado para
`format_version` permite introducir chunks en el futuro como `format_version = 2` sin reinterpretar
los archivos ya cifrados con `format_version = 1`.

## 9. Rutas físicas opacas y prevención de path traversal

`vault/files/<2-hex>/<uuid>.enc` — el filesystem nunca revela paciente, RUT, nombre original,
categoría, sesión, proceso ni diagnóstico; el nombre real vive exclusivamente en SQLCipher.
`storage_path` siempre se genera internamente a partir de un UUID nuevo, nunca a partir de un
input de la usuaria. `resolve_within_files_root` es una segunda capa de defensa (parseo manual, sin
regex) que rechaza `..`, rutas absolutas disfrazadas de shard, shards inválidos, UUIDs inválidos,
extensión distinta de `.enc`, y un shard que no corresponde al UUID del archivo — 8 tests
dedicados de traversal, todos en verde.

## 10. Importación — sin copia plaintext intermedia, atómica

Orden implementado: validar paciente/proceso/sesión → validar tamaño con `fs::metadata` (antes de
leer) → leer el archivo de origen (que puede estar en plano, porque le pertenece a la usuaria fuera
del vault) → generar DEK aleatoria → cifrar en memoria → zeroizar el buffer de plaintext → envolver
la DEK → escribir a un temporal `.enc.tmp` dentro del shard destino → `rename` atómico → **recién
entonces** el `INSERT` en `documents`. **Cuaderno Clínico nunca crea una copia temporal plaintext
durante la importación** — verificado por `creates_a_document_and_persists_it_encrypted_on_disk`,
que lee los bytes físicos del `.enc` resultante y confirma la ausencia del plaintext original. Si
el `INSERT` falla después de un `rename` exitoso, se limpia el ciphertext recién escrito con
certeza total de que nada más pudo haberlo referenciado todavía (se acaba de crear en esa misma
llamada) — no se confunde con un huérfano descubierto más tarde por el reconciliador, que nunca se
borra automáticamente.

## 11. Consistencia DB/filesystem

`services::documents::check_consistency` compara `documents.storage_path` (activos y archivados)
contra lo presente en `vault/files/` y devuelve `missing_ciphertext_count`/
`orphan_ciphertext_count`. **Nunca borra nada automáticamente** — puramente diagnóstico, expuesto
como comando manual (`check_document_consistency`). Cumple la instrucción explícita de nunca
eliminar automáticamente un ciphertext huérfano potencialmente recuperable.

## 12. Categorías

Las seis originales (`informe`, `consentimiento`, `evaluacion_adjunta`, `receta`,
`correspondencia`, `otro`) más `derivacion` (nueva). Taxonomía deliberadamente pequeña y
administrativa — no se amplió más allá de estos siete valores, nunca un catálogo diagnóstico.
Consentimientos: solo almacenamiento de un consentimiento ya firmado externamente — sin captura de
firma manuscrita, firma electrónica, firma avanzada/legal, certificados ni validación legal, todo
explícitamente fuera de alcance.

## 13. Proceso cerrado vs. paciente archivado

Implementadas como dos reglas deliberadamente distintas, tal como exigía la aprobación:

- **Proceso cerrado SÍ permite documentos nuevos** — `check_episode_belongs_to_patient` (función
  nueva, separada de `treatment_episodes::check_episode_assignable`) valida existencia, mismo
  paciente y no archivado, pero deliberadamente **no** rechaza `status = 'cerrado'`. Nunca reabre
  el proceso, nunca modifica cierre/formulación/sesiones. Verificado con
  `allows_a_document_for_a_closed_episode_unlike_formulation`.
- **Paciente archivado NO permite documentos nuevos** (`PatientArchived`) — documentos existentes
  permanecen completamente visibles y operables (listar/abrir/exportar/editar metadata/archivar/
  restaurar); solo se bloquea crear contenido nuevo. Restaurar al paciente vuelve a habilitarlo.

Combinaciones contradictorias de proceso/sesión (una sesión que ya pertenece a un proceso distinto
del `episode_id` indicado) se rechazan explícitamente; una sesión sin proceso nunca es
contradictoria. Asociaciones cruzadas entre pacientes distintos se rechazan siempre.

## 14. Visualización, apertura, exportación, temporales

Híbrida por MIME: imágenes se descifran en memoria a un `data:` URL (cero huella en disco, buffer
zeroizado tras codificar); el resto se descifra a un temporal de nombre opaco en
`std::env::temp_dir()` (**nunca dentro de `vault/files/`**) y se abre con la aplicación por
defecto del sistema (crate `open`, ya en uso para el flujo OAuth de Google Calendar — sin
dependencia nueva). `DocumentTempRegistry` registra cada temporal y lo limpia en tres puntos:
bloqueo manual del vault, bloqueo automático por inactividad, y cierre de la aplicación
(`RunEvent::Exit`, best-effort). `sweep_stale_temp_files()` corre al arrancar, antes de crear
`VaultSession`, y limpia por nombre reconocible cualquier temporal huérfano de un crash anterior,
sin depender del registro en memoria (que se pierde entre reinicios).

"Exportar copia" es una acción explícita y consciente de la usuaria, distinta de "Abrir" — destino
elegido conscientemente vía diálogo nativo, con advertencia breve y no alarmista de que la copia
queda fuera del cifrado del vault. Nunca ocurre automáticamente.

**Limitación honesta documentada**: "borrar" un archivo temporal no garantiza borrado físico
seguro en un SSD (wear-leveling) — no se promete ni se implementa borrado forense irreversible en
esta fase.

## 15. Backup / Restore — obligatorio, cumplido

`create_backup` enumera `documents::list_all_storage_paths` **dentro de la misma llamada a
`with_connection` que ejecuta `VACUUM INTO`**, para que la lista de documentos refleje exactamente
el mismo instante que el propio snapshot de `vault.db`. Cada ciphertext se copia tal cual (nunca se
descifra para respaldar) y se agrega al manifest con su propio hash/tamaño. **Si una fila de
`documents` referencia un archivo inexistente, `create_backup` rechaza el backup completo**
(`BackupError::MissingDocumentFile`) — nunca produce un respaldo incompleto en silencio.
`restore_backup` **no requirió ningún cambio**: el manifest y el contenedor ya eran genéricos sobre
un número arbitrario de archivos con ruta anidada, incluida la extracción segura contra traversal.
Backups antiguos sin `files/` siguen siendo restaurables sin cambios — la ausencia de `files/`
nunca se interpreta como corrupción, se decide por lo que el manifest de ese backup declara.

## 16. Google Calendar y nube

Cero referencias a `documents`/`category`/`description`/`storage_path`/`mime_type` dentro de
`src-tauri/src/calendar/` ni de `src/features/agenda/` (verificado por grep), cero importaciones
de `calendar::*` desde este vertical. Sin nube, sin OCR, sin IA, sin analytics ni telemetría sobre
ningún documento.

## 17. UI

Pestaña "Documentos" funcional en la ficha del paciente (`src/features/documents/`): listar
activos/archivados (mismo patrón de toggle que `PaymentsTab`), agregar (selector de archivo nativo
`tauri-plugin-dialog`, ya usado por Backup — sin dependencia nueva), abrir/previsualizar (imagen en
modal in-app o aplicación externa según MIME), exportar copia, editar metadata, archivar/restaurar.
Cada fila muestra nombre original, categoría, fecha, tamaño, descripción y asociación a proceso/
sesión cuando existe — **nunca** `storage_path`, clave envuelta, nonce, hash ni UUID físico:
estructuralmente imposible por IPC, porque `Document` (la fila completa) ni siquiera deriva
`Serialize`, y `DocumentSummary` (el único tipo que cruza IPC) no tiene esos campos. Drag & drop no
se implementó — evaluado como no obligatorio frente al selector de archivos correcto, sin ampliar
el alcance.

## 18. Tests nuevos

71 tests nuevos: 5 en `db::migrations` (`SCHEMA_V9`), 6 en `security::session`
(`wrap_file_key`/`unwrap_file_key`: roundtrip, clave incorrecta, nonce manipulado, vault bloqueado,
DEKs independientes), 22 en `services::document_crypto` (roundtrip, clave/nonce incorrectos,
truncamiento, cuerpo truncado, vacío, límite exacto y sobre el límite, ciphertexts distintos para
plaintexts idénticos, DEKs independientes, `format_version` desconocida, magic inválido, 8 de
traversal), 7 en `services::document_temp` (asignación, extensión maliciosa rechazada, limpieza
completa/tolerante, barrido de arranque respeta archivos ajenos), 10 en `repositories::documents`
(CRUD, asociaciones, listados con exclusión de crypto/archivados, archivar/restaurar), 18 en
`services::documents` (creación, roundtrip, paciente/proceso/sesión inválidos, proceso cerrado
permite documento, combinación contradictoria rechazada, categorías, límites de tamaño, marcador de
privacidad ausente del ciphertext), 9 en `backup::service` (backup sin/con documentos, archivado,
restore completo, ciphertext faltante/corrupto, backup antiguo sin `files/`, traversal malicioso).

## 19. Total de tests

**766/766 en verde** (695 previos sin cambios + 71 nuevos). Ningún test eliminado ni debilitado.

## 20. Build / Clippy / Lint

| Comando | Resultado |
|---|---|
| `cargo test` | 766/766 en verde |
| `cargo clippy --release --all-targets` | 0 warnings |
| `cargo build --release` | limpio |
| `npm run build` | limpio, sin errores TS |
| `npm run lint` | 25 warnings (24 preexistentes + 1 nuevo en `DocumentsTab.tsx`, misma categoría `react(set-state-in-effect)` ya presente en `GoalsTab`/`PaymentsTab`/`SessionsTab`/`AssessmentsTab`/`SafetyPlanTab`/`FormulationTab` — sin categoría nueva), 0 errors |
| `git diff --check` | limpio |

Comparado explícitamente contra el baseline de la sección 1: mismo número de errores (0), mismas
categorías de warnings de frontend, ningún warning nuevo de clippy.

## 21. Prueba manual GUI — bloqueada por el entorno, documentada como pendiente (no inventada)

Se reconstruyó el binario `release` y se intentó lanzar la aplicación en un vault desechable bajo
Xvfb, siguiendo el mismo procedimiento que en fases anteriores. La ventana se creó correctamente
("Cuaderno Clínico", proceso vivo), pero WebKitGTK no pudo cargar el contenido
("Could not connect to localhost: Connection refused" — confirmado con dos capturas de pantalla
~5s aparte), la misma limitación de renderizado ya documentada en las Fases 13/15 en este entorno,
no relacionada con el código de esta fase. Cleanup realizado (procesos terminados, vault desechable
eliminado). Los casos del futuro test manual en hardware real (importar PDF/imagen ficticios,
listar, reiniciar, abrir, bloquear/desbloquear, exportar, archivar/restaurar un documento, backup,
restore, inspección física de `vault/files`, intento de abrir un `.enc` directamente confirmando
que no revela nada) **no pudieron ejercitarse en vivo en este entorno en este momento** —
documentado como pendiente, siguiendo la política explícita del proyecto de nunca inventar
resultados, y se agrega al Pre-V1 Manual Acceptance Test acumulado. La cobertura funcional
equivalente está cubierta por los 71 tests automatizados de la sección 18.

## 22. Auditoría de privacidad

- Marcador ficticio `XYZFASE16DOCUMENTOSMARKER`, verificado programáticamente
  (`document_content_is_unrecoverable_directly_from_the_vault_meta_or_db_without_unlocking`) que
  nunca aparece en los bytes físicos del `.enc` correspondiente.
- Cero llamadas a `log::`/`println!`/`dbg!`/`eprintln!` con contenido/nombre/descripción/paciente/
  claves/plaintext/ruta de origen en ninguno de los archivos nuevos (verificado por grep).
- Cero referencias cruzadas con Google Calendar (sección 16).
- Cero `localStorage`/`sessionStorage`/`navigator.clipboard`/`window.location`/`document.title`/
  `analytics`/`telemetry` en `src/features/documents/` (verificado por grep).
- Búsqueda de secretos en el diff completo (Bloque 41 de la aprobación): sin DEKs ni claves
  literales, sin rutas personales del entorno, sin restos de temporales, sin documentos clínicos
  reales — solo texto ficticio en fixtures de test y contraseñas sintéticas ya reutilizadas en todo
  el proyecto (`"ContrasenaSegura2026!"`, etc.). `git status --short` confirmó cero archivos
  binarios/documentos accidentalmente incluidos.

## 23. Archivos nuevos

- `docs/documents.md`
- `src-tauri/src/services/document_crypto.rs`
- `src-tauri/src/services/document_temp.rs`
- `src-tauri/src/services/documents.rs`
- `src-tauri/src/repositories/documents.rs`
- `src-tauri/src/commands/documents.rs`
- `src/features/documents/types.ts`
- `src/features/documents/api.ts`
- `src/features/documents/DocumentsTab.tsx`

## 24. Archivos modificados

- `src-tauri/src/db/migrations.rs` (`SCHEMA_V9` + 5 tests)
- `src-tauri/src/security/session.rs` (`wrap_file_key`/`unwrap_file_key` + tipos + 6 tests — única
  modificación productiva dentro de `security/*`)
- `src-tauri/src/security/mod.rs` (reexporte aditivo de los tipos nuevos)
- `src-tauri/src/backup/service.rs` (`MissingDocumentFile`, integración de documentos en
  `create_backup` + 9 tests)
- `src-tauri/src/repositories/mod.rs`, `src-tauri/src/services/mod.rs`,
  `src-tauri/src/commands/mod.rs` (registro de los módulos nuevos)
- `src-tauri/src/commands/vault.rs` (`lock_vault` limpia temporales de Documentos)
- `src-tauri/src/lib.rs` (barrido de arranque, estado de `DocumentTempRegistry`, limpieza en
  auto-lock y en `RunEvent::Exit`, 10 comandos nuevos en `generate_handler!`)
- `src/features/patients/PatientDetailScreen.tsx` (integración de `DocumentsTab`)
- `docs/ARCHITECTURE.md` (fila de la Fase 16 en la tabla de fases; corrección de la referencia
  obsoleta `vault/documents/` → `vault/files/` y descripción desactualizada en la sección 7
  "Archivos y documentos" — directamente relevante a esta fase, no una corrección incidental de
  documentación histórica no relacionada)

Ningún archivo prohibido tocado más allá de lo declarado (`security/*` limitado a `session.rs` +
`mod.rs`, `calendar/*` no tocado, `db/connection.rs` no tocado).

## 25. Tablas / migraciones

`SCHEMA_V9` (sección 6) — la única migración de esta fase. `SCHEMA_V1`–`V8` sin cambios.

## 26. Funcionalidades anteriores afectadas

Ninguna funcionalidad existente se modificó en su comportamiento. `documents` era una tabla
presente sin usar desde `SCHEMA_V1` — esta fase la activa por primera vez; la reconstrucción de
`SCHEMA_V9` preserva cualquier fila previa (no había ninguna real en producción). El único cambio
fuera de archivos nuevos y de `migrations.rs`/`security/session.rs`/`backup/service.rs` es la
integración de una pestaña que antes mostraba "Próximamente" en `PatientDetailScreen.tsx`, y la
extensión del ciclo de vida de bloqueo/cierre en `lib.rs`/`commands/vault.rs` para incluir la
limpieza de temporales de Documentos (aditiva, no cambia el comportamiento de bloqueo/desbloqueo
existente).

## 27. Dependencias nuevas

Cero. Reutiliza `aes-gcm`, `getrandom`, `uuid`, `sha2`, `base64ct`, `tauri-plugin-dialog` y el
crate `open`, todos ya presentes en `Cargo.toml`/`package.json` antes de esta fase. Sin la feature
`stream` de `aead` (explícitamente no agregada, sección 8).

## 28. Commits

- `4f7c5a1` — `Fase 16: documentos y adjuntos clínicos cifrados` (implementación completa: 20
  archivos, 3709 inserciones/31 eliminaciones).
- Este informe se agrega en un commit documental separado inmediatamente después, mismo patrón ya
  usado en las transiciones de fases anteriores.

## 29. Push

Realizado a `claude/cuaderno-clinico-desktop-udijjq`. Sin force push.

## 30. Git final

- HEAD tras el commit de código: `4f7c5a1` — coincide con
  `origin/claude/cuaderno-clinico-desktop-udijjq`.
- Árbol de trabajo limpio (`git status` → "nothing to commit, working tree clean") tras el push.
- Migraciones: 1 (`SCHEMA_V9`).
- Dependencias nuevas: 0.

## 31. Deuda técnica y riesgos residuales

- **Sin streaming** — límite fijo de 50 MB por archivo, diferido explícitamente (sección 8).
- **Sin reconciliación activa** de huérfanos/faltantes — solo diagnóstico manual
  (`check_document_consistency`), nunca reparación automática (sección 11).
- **Sin borrado físico seguro garantizado** para temporales descifrados en SSD — documentado
  honestamente, no prometido (sección 14).
- **Sin hard delete/purge** de documentos — solo soft delete; explícitamente fuera de alcance salvo
  aprobación separada futura.
- **Sin firma electrónica de ningún tipo** — solo almacenamiento de un consentimiento ya firmado
  externamente (sección 12).
- **Sin drag & drop** — evaluado como no obligatorio frente al selector de archivos correcto.
- **Prueba manual GUI pendiente** por la misma limitación de entorno ya documentada en fases
  anteriores (sección 21), no por falta de intento — se suma a las pendientes acumuladas.
- **Validación física macOS/Windows/iOS/iPadOS** sigue en 0% — brecha ya conocida del proyecto, sin
  cambios en esta fase.

## 32. Decisiones nuevas que requieren aprobación

Ninguna decisión de arquitectura, seguridad o modelo de datos quedó pendiente de aprobación en
este cierre — las 12 preguntas planteadas en la auditoría previa (`Plan-Fase-16-...md`) ya fueron
resueltas explícitamente por el mensaje de aprobación de 45 bloques antes de escribir código, y
ninguna de las condiciones de "detenerse y explicar" listadas en esa aprobación se activó durante
la implementación (nunca fue necesario exponer la DEK cruda del vault, tocar `envelope.rs`,
cambiar `vault.meta.json`, cambiar el KDF, romper compatibilidad de backups existentes, o
introducir una dependencia nueva).

## 33. Estimación de completitud V1 (actualizada)

- **Núcleo clínico** (pacientes, sesiones, objetivos, antecedentes, procesos, cierre, evaluaciones,
  plan de seguridad, formulación, documentos): con Documentos cerrado, el núcleo clínico textual y
  documental queda completo — no queda ninguna brecha estructural clínica conocida y no
  implementada.
- **V1 clínica** (núcleo + continuidad + pagos + agenda + documentos): ~95%.
- **V1 operacional** (V1 clínica + backup/restore + validación multiplataforma real): backup/
  restore ahora cubre documentos de punta a punta; la validación física en macOS/Windows/iOS/
  iPadOS sigue siendo la brecha más ancha, sin cambios en esta fase. ~60%.
- **Visión multiplataforma completa** (V1 operacional + iOS/iPadOS + sincronización E2EE): sin
  cambios, ~25-30% — correctamente fuera de alcance de esta fase (regla 7 de `CLAUDE.md`).

## 34. Recomendación de próxima fase

Con Documentos cerrado, el núcleo de funcionalidad clínica textual y documental del V1 queda
prácticamente completo. Se recomienda **evaluar explícitamente con la usuaria** antes de decidir
la fase siguiente, entre al menos estas opciones: (a) **cierre operacional de V1** — validación
física macOS/Windows (la brecha más ancha y menos dependiente de código nuevo, nunca ejercitada en
hardware real en toda la historia del proyecto), auditoría de seguridad end-to-end, consolidación
de la deuda manual de pruebas GUI acumulada a través de las fases; (b) **Línea temporal** (vista
agregada de solo lectura combinando sesiones/cierres/formulaciones/evaluaciones/documentos por
fecha, sin tabla nueva); (c) **Export** (formato abierto, requeriría una fase de diseño explícita
antes de empezar: qué formato, qué alcance, si incluye documentos, y explícitamente distinto de
Backup/Sync por la regla 6 de `CLAUDE.md`). Ninguna de estas líneas se implementa en este cierre —
a la espera de revisión y aprobación explícita de este informe, según la regla permanente del
proyecto.

---

**Siguiendo la instrucción explícita final de la aprobación de esta fase: este cierre se detiene
aquí. No se avanza automáticamente a la Fase 17 ni a ninguna otra. Se espera revisión del informe
antes de autorizar cualquier fase posterior.**
