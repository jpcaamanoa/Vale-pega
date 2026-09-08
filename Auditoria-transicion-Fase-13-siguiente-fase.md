# Auditoría post Fase 13 — Camino a V1

Auditoría exclusivamente de lectura/planificación. No se modificó ningún archivo, no se creó
ninguna migración, no se instaló ninguna dependencia, no se hizo ningún commit ni push como parte
de este documento. Todos los hallazgos están verificados directamente contra el código y el
esquema reales, no asumidos.

## 1. Baseline

Verificado con `git rev-parse HEAD`, `git branch --show-current`, `git status --porcelain=2`,
`git log --oneline --decorate -15` y `git fetch` + comparación contra `origin`.

- **HEAD real**: `5427aee` (`docs: agregar informe de cierre de Fase 13`).
- **HEAD esperado según tu prompt**: `bd6d9b1` (`Fase 13: evaluaciones clínicas y seguimiento
  psicométrico`).
- **Discrepancia encontrada y detenida antes de continuar** (regla del propio encargo): el commit
  `5427aee` agrega únicamente `Informe-de-cierre-Fase-13-Evaluaciones.md` (275 líneas, un solo
  archivo nuevo) — 0 cambios de código productivo, 0 cambios de esquema, 0 cambios de tests. Fue
  forzado por el stop-hook del repositorio ("hay archivos sin trackear, commitea y pushea"), visible
  en esta misma conversación, no una acción no autorizada ni un cambio de alcance.
- **Confirmado con `origin`**: `origin/claude/cuaderno-clinico-desktop-udijjq` = `5427aee` — sin
  divergencia, rama sincronizada.
- **Working tree**: limpio (`git status --porcelain=2` sin salida) al momento de iniciar esta
  auditoría.
- Presentada la discrepancia vía `AskUserQuestion`, confirmaste explícitamente usar `5427aee` como
  baseline real. Se registra aquí como decisión ya tomada, no pendiente.

**Conclusión de la sección**: baseline verificado y aceptado. No se detectó ninguna divergencia
real de código/esquema/tests respecto a lo que el informe de Fase 13 declaraba.

## 2. Regresión

Ejecutado exactamente lo pedido, sin asumir el resultado:

| Comando | Resultado real | Esperado según informe | Coincide |
|---|---|---|---|
| `cargo test --release` | **668/668 en verde**, 0 fallidos | 668/668 | ✅ |
| `cargo clippy --release --all-targets` | **0 warnings** | 0 warnings | ✅ |
| `cargo build --release` | Compila limpio | limpio | ✅ |
| `npm run build` | Compila limpio, sin errores TS | limpio | ✅ |
| `npm run lint` | **23 warnings, 0 errors**, exit 0 | 23 warnings, 0 errors | ✅ |
| `git diff --check` | Sin salida (limpio) | limpio | ✅ |

Ninguna categoría de warning nueva en lint respecto a lo declarado. Ningún test rojo. **Regresión
confirmada sin discrepancias.**

## 3. Estado Fase 13

Verificado directamente contra el código, no contra el informe:

- `assessment_instruments`: confirmado con `abbreviation TEXT` y `category TEXT` (columnas de
  `SCHEMA_V7`, `ALTER TABLE` verificado línea a línea en `db/migrations.rs`).
- `assessment_administrations`: confirmado con `episode_id TEXT REFERENCES treatment_episodes(id)
  ON DELETE SET NULL` + `idx_assessment_administrations_episode`.
- `SCHEMA_V7`: presente, aditiva, aplicada después de `SCHEMA_V6` en `migrations()`, con
  `.foreign_key_check()` como todas las anteriores.
- Catálogo de instrumentos: `create_instrument`/`get_instrument`/`list_instruments`/
  `update_instrument` en `services::assessments` — confirmado. **Sin `archive_instrument`/
  `soft_delete_instrument` en ningún punto** (ni repositorio, ni servicio, ni comando) — coincide
  exactamente con lo declarado en el informe (sección 5 del punto siguiente).
- Administraciones: `create_administration`/`get_administration`/`list_administrations`/
  `list_archived_administrations`/`list_administrations_for_instrument`/`update_administration`/
  `archive_administration`/`restore_administration` — todos presentes y cubiertos por tests.
- Evolución longitudinal: `list_by_patient_and_instrument` (repositorio) /
  `list_administrations_for_instrument` (servicio) — confirmado que filtra por
  `instrument_id` exacto y ordena `administered_at ASC` — un test dedicado
  (`list_by_patient_and_instrument_orders_oldest_first_and_excludes_other_instruments`) verifica
  explícitamente que un segundo instrumento no se mezcla.
- `raw_responses`: presente en el esquema (`db/migrations.rs` línea 371), **cero referencias** en
  `repositories::assessments`, `services::assessments`, `commands::assessments` ni en
  `src/features/assessments/*` fuera de comentarios explicativos — confirmado con `grep`.
- Privacidad: `commands/assessments.rs` no importa `calendar::*` (la única coincidencia de `grep`
  es el propio comentario que lo declara, no un `use` real).
- Backup: `backup::service::current_app_schema_version()` sigue siendo dinámico; los dos literales
  de test (`manifest.schema_version` y `supported_schema_version`) están actualizados a `7`.
  Confirmado, sin cambio de diseño.
- Google Calendar: sin ninguna referencia cruzada a evaluaciones — dominio intacto.

**Fase 13 está técnicamente cerrada** — todo lo declarado en su informe de cierre se verificó
directamente contra el código, sin ninguna discrepancia.

## 4. Validación manual pendiente

El informe de Fase 13 declara explícitamente que la prueba GUI manual no se pudo ejecutar en el
entorno remoto (mismo bloqueo del clasificador de permisos que ya afectó el micro-hardening de
Plan de Seguridad). **Esto no invalida el commit técnico** — la cobertura automatizada (37 tests
nuevos de Fase 13 + 15 del micro-hardening, más clippy/build/lint limpios) es real y verificable.
Pero se clasifica correctamente como **VALIDACIÓN OPERACIONAL PENDIENTE**, no como "probado".

**Escenarios pendientes** (de las instrucciones ya entregadas en el informe de cierre):
catálogo de instrumentos (crear/editar), registrar evaluación con subescalas e interpretación,
crear instrumento desde el formulario sin perder el contexto, vínculo a proceso terapéutico,
evolución longitudinal con dos administraciones, editar, archivar/restaurar, paciente archivado,
nombre de instrumento duplicado, ciclo Backup/Restore, auditoría de privacidad con marcador. Más
los pendientes ya heredados de Fase 12: "Actualizar plan" copiando contactos en vivo, botones
deshabilitados correctamente al archivar.

**¿Es blocker para continuar desarrollo?** No. Ninguno de estos escenarios reveló, en su momento,
un problema de diseño — son verificaciones de que la UI conectada al backend ya probado se comporta
como se espera. Bloquear todo el roadmap por esto sería desproporcionado.

**¿Debe ejecutarse antes de RC/V1?** Sí, sin excepción — antes de declarar cualquier milestone
"instalable para pruebas" o "V1 lista para uso real", exactamente como pediste.

**¿Se puede agrupar con la validación física de escritorio?** Sí, y es la recomendación de esta
auditoría (ver sección 22): construir un único "Pre-V1 Manual Acceptance Test" que cubra Plan de
Seguridad + Evaluaciones + todo lo demás de una sola vez en hardware real, en vez de repetir
mini-validaciones aisladas fase por fase que este entorno no puede ejecutar de todos modos.

## 5. Inventario funcional

| Vertical | Estado |
|---|---|
| Vault / Security | **IMPLEMENTADA** |
| Pacientes | **IMPLEMENTADA** |
| Dashboard | **IMPLEMENTADA** (parcial en el sentido de que solo agrega datos ya reales de otras verticales, sin tabla propia — no es una carencia, es su diseño) |
| Agenda | **IMPLEMENTADA** |
| Google Calendar | **IMPLEMENTADA** (funcionalmente completa; validación real contra una cuenta Google pendiente — ver sección 10) |
| Sesiones / Notas | **IMPLEMENTADA** |
| Objetivos | **IMPLEMENTADA** |
| Antecedentes | **IMPLEMENTADA** |
| Ubicación / Estadísticas | **IMPLEMENTADA** |
| Pagos | **IMPLEMENTADA** |
| Continuidad (preparación + tareas) | **IMPLEMENTADA** |
| Procesos (`treatment_episodes`) | **IMPLEMENTADA** |
| Cierre/Alta | **PARCIAL** — creación de cierres y reapertura completas; historial de cierres solo tiene backend (ver sección 14) |
| Backup/Restore | **IMPLEMENTADA** (validación real de instalación/restore físico pendiente, ver sección 9) |
| Plan de Seguridad | **IMPLEMENTADA** (validación GUI manual pendiente, sección 4) |
| Evaluaciones | **IMPLEMENTADA** (validación GUI manual pendiente, sección 4) |
| Formulación | **SCHEMA-ONLY** |
| Documentos | **SCHEMA-ONLY** |
| Biblioteca | **SCHEMA-ONLY** |
| Recordatorios | **SCHEMA-ONLY** |
| Línea temporal | **NO IMPLEMENTADA** (ni siquiera tabla propia — sería una vista agregada, sección 15) |
| Export | **DIFERIDA** (sin tabla propia, sin ninguna decisión de formato tomada) |

## 6. Schema-only restante

Confirmado con `grep`/lectura directa de `db/migrations.rs` y con la ausencia total de archivos en
`repositories/`, `services/`, `commands/` y `src/features/` para estos nombres:

- **Formulación** (`case_formulations`, `formulation_versions`, `formulation_nodes`,
  `formulation_edges`).
- **Documentos** (`documents`).
- **Biblioteca** (`library_resources`, `library_tags`, `library_resource_tags`).
- **Recordatorios** (`reminders`).

Exactamente las cuatro que el informe de Fase 13 proyectaba como remanentes — **confirmado, no
asumido**. `case_formulations`/`documents`/`reminders` al menos tienen un espacio reservado en la
navegación (pestañas "Formulación"/"Documentos" en la ficha del paciente, sin ruta separada para
Recordatorios ni Biblioteca — ninguna de las dos tiene ni siquiera un placeholder de UI).

## 7. Deuda

Clasificación exacta de los siete puntos pedidos:

| # | Ítem | Clasificación | Detalle verificado |
|---|---|---|---|
| A | Historial de cierres: API existe, UI no | **DEUDA** | `episodeClosuresApi.listHistory` (frontend) y `list_episode_closure_history` (backend) existen y están cubiertos por tests; `grep` confirma que ningún componente de `src/features/treatment-episodes/*` los invoca. |
| B | Línea temporal: reservada, sin implementación | **PENDIENTE DE VALIDACIÓN → NO IMPLEMENTADA** | Confirmado: `'linea_temporal'` existe en `SectionId` y en `SECTIONS`, ausente de `SECTIONS_WITH_REAL_CONTENT` — sigue mostrando "Próximamente". No es deuda de código roto, es simplemente no construida todavía. |
| C | Instrumentos sin archivado real | **LIMITACIÓN (por diseño, documentada)** | Confirmado: `assessment_instruments` no tiene `deleted_at` en el esquema, y no existe ninguna función de archivado/borrado en ningún nivel. `docs/assessments.md` sección 6 ya documenta esta decisión explícitamente (`ON DELETE RESTRICT` ya impide huérfanos; sin caso de uso pedido para eliminar uno sin usar). No es un bug — es una decisión de diseño ya tomada y ya escrita. |
| D | Prueba manual Plan de Seguridad pendiente | **PENDIENTE DE VALIDACIÓN** | Confirmado, ver sección 4. |
| E | Prueba manual Evaluaciones pendiente | **PENDIENTE DE VALIDACIÓN** | Confirmado, ver sección 4. |
| F | SQL directo en `services::episode_closures::today_utc_date` | **DEUDA (confirmada, sigue presente)** | `services/episode_closures.rs` líneas 161-162: `fn today_utc_date(conn: &Connection) -> rusqlite::Result<String> { conn.query_row("SELECT strftime('%Y-%m-%d','now')", [], \|r\| r.get(0)) }` — SQL crudo ejecutado directamente en la capa de servicio, no delegado al repositorio. Contraste: `services::treatment_episodes::today_utc_date()` (mismo propósito, otro dominio) deriva la fecha de `SystemTime` en Rust, sin SQL directo. Es una inconsistencia de capas, no un bug funcional — el valor resultante es correcto y está cubierto por tests; el defecto es puramente arquitectónico (la regla del proyecto es "SQL puro vive en `repositories`"). |
| G | `patients.status` vs `deleted_at` — ¿sigue correcto el hardening? | **CONFIRMADO SIN REGRESIÓN** | `ArchivedStatusIsNotManuallySelectable` presente en `services/patients.rs` (línea 105, 284, y los dos tests que la ejercitan), incluida en los 668 tests verdes. Sin regresión. |

## 8. Privacidad

- `assessment_administrations`/`assessment_instruments`: sin ninguna referencia cruzada a
  `calendar::*` (verificado con `grep`, cero resultados fuera de un comentario).
- `raw_responses`: confirmado sin uso en ningún punto del código nuevo (sección 3).
- Minimización de IPC: `AssessmentAdministrationSummary` no lleva `subscaleScores` ni
  `interpretationText` — confirmado en `repositories/assessments.rs` (el struct y su `map_row`
  explícitamente omiten esos dos campos).
- No se detectó ningún `println!`/log de contenido clínico en los archivos nuevos de Fase 13 (no
  hay ninguna llamada de logging en `repositories/assessments.rs`, `services/assessments.rs` ni
  `commands/assessments.rs` — cero referencias a `log::`/`println!` en esos tres archivos).
- Pendiente real: la auditoría de privacidad **con marcador ficticio en un vault real** (Caso K del
  informe de Fase 13) sigue sin ejecutarse por el mismo bloqueo de entorno — es la misma limitación
  de la sección 4, no un hallazgo nuevo.

## 9. Backup

- Backup/Restore fue **probado end-to-end en fases anteriores** (Fase 10, con corrupción
  deliberada; Fase 11, con un proceso cerrado) — no en este entorno ahora, pero sí como parte del
  historial verificable de fases previas.
- `documents/` sigue reservado en el manifest (`backup/manifest.rs` líneas 20-24): "directorio
  opcional reservado para una fase futura (Documentos clínicos, todavía no implementada) — un
  backup v1 de hoy simplemente no lo incluye, y el restore nunca exige que exista." Confirmado, sin
  cambios desde Fase 10.
- **Lo que falta antes de producción real** (nunca ejecutado en ningún entorno de este proyecto
  hasta ahora): un *restore drill* con un backup movido físicamente a otro disco/computador; un
  ciclo real de instalar → usar → desinstalar/reinstalar → restaurar en macOS/Windows reales; un
  backup creado por una versión de la app y restaurado por una versión posterior con una migración
  de por medio (probado solo a nivel de base de datos en tests, nunca a nivel de instalador real).

## 10. Calendar

La integración con Google Calendar es estructuralmente minimizada por diseño (confirmado en
`docs/google-calendar.md` y en el propio código: el evento espejo solo lleva resumen genérico +
dos timestamps). Pero, tal como ya declaraba la Fase 3 en `ARCHITECTURE.md` ("Backend de keychain
real y login OAuth completo contra Google no pudieron ejercitarse de punta a punta en este entorno
headless... declarado explícitamente, no asumido resuelto"), **sigue sin haber una prueba real
contra una cuenta de Google** en ningún punto de la historia del proyecto hasta ahora.

Tu preferencia explícita: **sí es requisito de V1** si Google Calendar se ofrece como función
productiva — se adopta esa postura en el roadmap de esta auditoría (secciones 21/26).

## 11. Formulación

Esquema real (`db/migrations.rs` líneas 194-266), verificado línea a línea:

- `case_formulations` (`id`, `patient_id` `NOT NULL REFERENCES patients ON DELETE RESTRICT`,
  `title NOT NULL`, `model_type` libre, `created_at`/`updated_at` con trigger, `deleted_at` — soft
  delete disponible).
- `formulation_versions` (`id`, `formulation_id ON DELETE CASCADE`, `version_number CHECK >= 1`,
  `summary_text` libre, `UNIQUE(formulation_id, version_number)`) — **versionado ya modelado en el
  esquema**, sin campos de "vigente/reemplazado" explícitos (a diferencia de `session_notes` o
  `safety_plans`) — sería responsabilidad de la capa de servicio decidir cuál versión es "la
  actual" (por ejemplo, la de `version_number` más alto), no hay un flag dedicado.
- `formulation_nodes` (`id`, `formulation_version_id ON DELETE CASCADE`, `node_type` libre,
  `label NOT NULL`, `description`, `position_x`/`position_y REAL NOT NULL`) — **campos de posición
  geométrica ya presentes**, pensados explícitamente para un editor visual (canvas de
  nodos/conexiones).
- `formulation_edges` (`id`, `formulation_version_id`, `source_node_id`/`target_node_id
  REFERENCES formulation_nodes ON DELETE CASCADE`, `relation_label` libre, `CHECK
  (source_node_id <> target_node_id)`) — **dos triggers `BEFORE INSERT`/`BEFORE UPDATE`** que
  fuerzan que ambos nodos de un edge pertenezcan exactamente a la misma `formulation_version_id`
  (el FK por sí solo no lo garantizaría). Índices: `idx_formulation_nodes_version`,
  `idx_formulation_edges_version`.

**React Flow**: el único rastro en todo el repositorio es un comentario SQL de `SCHEMA_V1`
("-- Formulación clínica (versionada; nodos y conexiones para React Flow)"). **No está en
`package.json`, no está mencionado en `docs/ARCHITECTURE.md`.** Confirmado con `grep` en ambos
archivos: cero resultados. **Nunca fue formalmente aprobada como dependencia — solo mencionada de
paso en un comentario de esquema hace muchas fases, nunca revisada desde entonces.**

**Comparación de estrategias**:

- **A. Formulación textual primero**: usa `formulation_versions.summary_text` (ya existe, campo de
  texto libre) sin tocar `formulation_nodes`/`formulation_edges` en absoluto. Compatible al 100%
  con un canvas visual futuro — los nodos/edges seguirían existiendo en el esquema sin usarse,
  exactamente el mismo patrón ya usado para `raw_responses` (Fase 13) o `is_custom`
  (`assessment_instruments`): una columna/tabla del esquema original que una fase no usa no bloquea
  ni invalida una fase posterior que sí la use.
- **B. Formulación visual completa desde el inicio**: requiere una librería de diagramación
  interactiva (React Flow u otra) — dependencia nueva no evaluada, complejidad de UI
  significativamente mayor, y readapta directamente el "criterio conservador de dependencias" que
  ya se aplicó explícitamente en Fase 6.1 y en Fase 13 (evitar librerías nuevas salvo necesidad
  real).

**La estrategia A no invalida el futuro canvas**: `formulation_nodes`/`formulation_edges` quedan
intactos en el esquema, listos para una fase posterior dedicada a la interacción visual —
exactamente como el propio proyecto ya trató la relación `case_formulations`↔`therapeutic_goals`
(`therapeutic_goals.formulation_id` ya existe desde `SCHEMA_V1`, sin que Formulación tuviera
todavía ninguna vertical construida).

## 12. Documentos

**Auditoría de seguridad, con el rigor pedido.**

Esquema real (`db/migrations.rs` líneas 166-189):

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
  created_at/updated_at/deleted_at  -- patrón estándar
);
```

**Qué ya está diseñado**: metadatos completos (nombre original, MIME, tamaño, hash SHA-256 del
**contenido en claro** — nombrado explícitamente `sha256_plaintext`, distinguiéndolo de un eventual
hash del blob cifrado), `storage_path` como referencia **indirecta** (no el archivo mismo en la
base de datos — el archivo físico vive fuera de `vault.db`, en algún directorio del vault), un flag
`is_clinical` para distinguir contenido clínico de administrativo, categoría cerrada de 6 valores,
vínculo opcional a paciente y a sesión (`ON DELETE SET NULL` — un documento nunca se pierde si se
borra —de hecho nunca se borra físicamente ningún registro— el paciente o la sesión asociados).

**Qué falta por completo**: absolutamente todo el mecanismo de cifrado individual del archivo. No
existe ningún código que cifre, descifre, escriba o lea un archivo del disco del vault. No existe
ninguna decisión tomada sobre qué clave usar, qué algoritmo (más allá de que el proyecto ya usa
AES-256-GCM para el envelope encryption del DEK — ver sección 13), ni cómo se relaciona con el
ciclo de vida del vault (bloqueo/desbloqueo, cambio de contraseña, recuperación de acceso).

## 13. Cifrado de documentos

**Ninguna decisión criptográfica nueva se toma en esta auditoría — se identifica el problema y se
marca explícitamente como RIESGO ALTO, tal como pediste.**

- **¿Reutilizar el DEK directamente es correcto?** **No es la práctica recomendada.** El DEK
  (`security::session::UnlockedSession::dek`, tipo `VaultKey`) hoy tiene un único propósito: envolver/
  desenvolver la clave de SQLCipher. Usar exactamente los mismos bytes de clave para cifrar,
  además, archivos individuales con un AEAD distinto (aunque sea el mismo algoritmo, AES-256-GCM)
  es un caso clásico de reutilización de clave entre dominios criptográficos distintos — el riesgo
  concreto es de gestión de nonces: dos subsistemas cifrando de forma independiente con la misma
  clave pueden, por error de implementación futura, terminar reutilizando un nonce, lo que rompe
  la seguridad de GCM por completo. No es solo teoría: es exactamente el tipo de error que las
  guías de NIST/RustCrypto advierten evitar mediante separación de claves por dominio de uso.
- **¿Debe derivarse una subclave?** Sí — la alternativa estándar y ya compatible con las librerías
  que el proyecto ya usa (RustCrypto, regla no negociable de "sin criptografía propia") es derivar
  una subclave de documentos a partir del DEK mediante una KDF de derivación (HKDF-SHA256, con una
  cadena de "info" de separación de dominio, p. ej. `b"cuaderno-clinico:documents:v1"`), nunca usar
  el DEK en claro para un segundo propósito.
- **Nonce único**: cada archivo cifrado necesita su propio nonce aleatorio de 96 bits (estándar
  GCM), generado con el mismo generador ya usado por el proyecto (`security::random`, ya presente),
  nunca reutilizado entre archivos ni derivado de forma predecible.
- **Autenticación**: GCM ya provee autenticación integrada (tag de 128 bits) — ningún mecanismo
  adicional necesario si se usa correctamente, pero el diseño debe decidir explícitamente qué
  metadatos (nombre de archivo, tipo MIME) se autentican como *additional authenticated data* (AAD)
  para impedir que se sustituyan sin invalidar el archivo.
- **Metadata**: `original_filename`/`mime_type`/`size_bytes` ya viven en `documents` (SQLCipher, ya
  cifrado a nivel de base) — no necesitan cifrado adicional propio, pero si se derivan de contenido
  sensible del archivo (ej. un nombre de archivo con el nombre real del paciente) deberían tratarse
  con el mismo cuidado que cualquier otro campo de texto libre de la aplicación.
- **Cambio de contraseña**: el proyecto ya resuelve este problema para el DEK mismo (envelope
  encryption: cambiar la contraseña solo re-envuelve el DEK, nunca re-cifra la base completa,
  Fase 1.4). Si la subclave de documentos se deriva del DEK en el momento de uso (nunca almacenada
  por separado), un cambio de contraseña **no requeriría re-cifrar ningún documento** — la
  derivación produce la misma subclave mientras el DEK subyacente no cambie. Esto es una ventaja
  fuerte a favor de la derivación por HKDF sobre cualquier esquema que almacene una clave de
  documentos independiente.
- **Restore**: como la subclave se derivaría del DEK en tiempo de uso, restaurar un backup que
  incluya documentos cifrados no requiere ningún paso adicional más allá de lo que Backup/Restore
  ya hace hoy con `vault.db` — el DEK viaja envuelto en `vault.meta.json`, ya incluido en todo
  backup.
- **Sync futuro**: fuera de alcance por regla permanente del proyecto (nunca se diseña Sync dentro
  de otra fase) — pero vale dejar señalado que una subclave derivada por HKDF es, en general, más
  compatible con un futuro esquema E2EE multi-dispositivo que una clave de documentos generada y
  almacenada de forma ad hoc, sin que esto implique diseñar Sync ahora.

**Clasificación: RIESGO ALTO** — es una decisión criptográfica real que debe aprobarse
explícitamente antes de escribir cualquier código de Documentos, siguiendo el mismo patrón ya usado
en Fase 1.4 (revisión explícita del esquema de envelope encryption antes de implementarlo).

## 14. Plaintext temporal

Analizado por flujo:

- **Importar PDF/imagen**: el archivo llega del diálogo nativo de selección de archivos
  (`tauri-plugin-dialog`, ya integrado desde Fase 10) como una ruta en el sistema de archivos del
  usuario, en texto plano, por definición — eso está fuera del control de la aplicación (es
  responsabilidad del usuario dónde tenía el archivo). El riesgo real de la aplicación es qué hace
  **después**: leer, cifrar y escribir la copia cifrada dentro del vault (sin dejar residuos), y
  nunca escribir una copia intermedia sin cifrar en un directorio temporal del sistema.
- **Preview**: **si se implementa preview interno** (opción B de la sección 16), probablemente
  requeriría descifrar el contenido a memoria (no a disco) para renderizarlo — factible sin tocar
  disco para imágenes (buffer en memoria → `<img src="data:...">` o similar) pero más delicado para
  PDF, que normalmente requiere que el visor (nativo o embebido) lea un archivo real, no un buffer
  en memoria arbitrario.
- **Abrir con la app del sistema** (opción C de la sección 16): **este es el flujo de mayor riesgo
  de plaintext temporal.** Abrir un documento con la aplicación por defecto del sistema operativo
  exige, casi siempre, escribir una copia descifrada real a un directorio temporal del disco — no
  hay forma de pasarle un buffer en memoria a una app externa arbitraria.
- **Exportar**: por definición, el usuario pide una copia en claro en un destino que ella elige —
  no es una fuga, es el propósito explícito de la acción, siempre que ocurra solo cuando el usuario
  lo pide activamente.
- **Drag & drop**: mismo análisis que "abrir con la app del sistema" si se arrastra hacia afuera de
  la aplicación (el sistema operativo típicamente necesita un archivo real en disco para completar
  un drag hacia otra aplicación).

**Si algún flujo requiere plaintext temporal** (el caso C es el más probable de necesitarlo):

- **Dónde**: un directorio temporal dedicado de la aplicación, nunca el directorio temporal
  genérico del sistema compartido con otras apps sin control de permisos.
- **Cuánto tiempo**: el mínimo posible — idealmente borrado inmediatamente después de que el
  proceso externo termine de leerlo, con un límite de tiempo de respaldo (ej. al cerrar la
  aplicación) para el caso de que el proceso externo no notifique cuándo terminó.
- **Limpieza tras crash**: mismo patrón ya usado por `backup::service::run_startup_recovery`
  (Fase 10) — una recuperación al arrancar que limpia cualquier residuo temporal de una sesión
  anterior interrumpida, basada en presencia de archivos, no en un flag frágil.
- **Caché de OS/antivirus/indexadores**: un archivo temporal en disco, aunque se borre después, es
  visible mientras existe a herramientas de indexación (Spotlight en macOS, Windows Search) y
  algunos antivirus pueden copiarlo a su propia cuarentena/caché de análisis — un riesgo real que
  ninguna limpieza posterior de la aplicación puede controlar completamente.

**Esto es, correctamente, uno de los criterios centrales para decidir si Documentos es la próxima
fase**: no es solo "cifrar un archivo", es diseñar con cuidado el único flujo (abrir con app
externa / drag out) que casi inevitablemente toca disco en claro, algo que ninguna otra vertical
de este proyecto hasta ahora ha tenido que resolver.

## 15. Documentos y Backup

Confirmado (sección 9 y `backup/manifest.rs`): la preparación de Fase 10 **sigue existiendo
intacta** — `BackupFileEntry { path, size_bytes, sha256 }` ya es genérico (no específico de
`vault.db`), así que un archivo `documents/<id>.enc` encajaría en la misma estructura sin
rediseñar el manifest. `VAULT_DB_ENTRY`/`VAULT_META_ENTRY`/`MANIFEST_ENTRY` son constantes fijas;
`documents/` sería una carpeta con un número variable de archivos, no una constante única — el
código de backup necesitaría iterar el directorio de documentos del vault en el momento de crear el
respaldo, algo que hoy no hace (solo empaqueta los dos archivos fijos).

**Qué haría falta diseñar** (no implementar): cómo enumerar los documentos a incluir (¿todos los
no archivados? ¿todos, incluidos los borrados lógicamente?), verificación de hash por archivo en
el restore (ya existe el campo `sha256` en el manifest, reutilizable sin cambios), compatibilidad
hacia atrás (un backup `v1` sin `documents/` debe seguir restaurándose sin error — ya es cierto hoy
porque `documents/` es "opcional" en el diseño), y un límite de tamaño razonable (ni el manifest ni
el `BackupFileEntry` imponen ninguno hoy — sin límite, un vault con muchos documentos grandes
podría producir un `.cclinbackup` de tamaño considerable; es una decisión de producto pendiente,
no un defecto).

## 16. Recordatorios

Esquema real (`db/migrations.rs` líneas 453-469):

```sql
CREATE TABLE reminders (
  id TEXT PRIMARY KEY,
  patient_id TEXT REFERENCES patients(id) ON DELETE SET NULL,
  related_entity_type TEXT CHECK (related_entity_type IS NULL OR related_entity_type IN
    ('session','payment','document','goal','assessment')),
  related_entity_id TEXT,
  title TEXT NOT NULL,
  description TEXT,
  due_at TEXT,
  status TEXT NOT NULL DEFAULT 'pendiente' CHECK (status IN ('pendiente','completado','descartado')),
  priority TEXT NOT NULL DEFAULT 'media' CHECK (priority IN ('baja','media','alta')),
  created_at TEXT NOT NULL DEFAULT (...),
  completed_at TEXT
);
CREATE INDEX idx_reminders_due_status ON reminders(due_at, status);
```

- **`related_entity_type`/`related_entity_id`**: referencia polimórfica sin FK real — ya
  documentado como limitación explícita desde `docs/db-schema.md` (punto 9, Fase 1.3): SQLite no
  soporta un FK condicional a "una de varias tablas posibles" sin complejidad considerable, así que
  la integridad de `related_entity_id` quedó deliberadamente delegada a la capa de servicio de una
  fase futura — nunca implementada todavía.
- **Nótese que `'assessment'` ya está en la lista de tipos válidos** desde `SCHEMA_V1` — el
  esquema anticipó Evaluaciones antes de que existiera ninguna vertical construida sobre ella,
  igual que anticipó `'goal'`/`'session'`/`'payment'`/`'document'`.
  Esto **no es una filtración de Fase 13 hacia atrás** — es una coincidencia de que la taxonomía
  original de `SCHEMA_V1` ya cubría el dominio.
- **Sin `deleted_at`**: a diferencia de la mayoría de las tablas del proyecto, `reminders` no tiene
  soft delete — su propio `status` (`pendiente`/`completado`/`descartado`) ya representa el ciclo
  de vida completo, mismo criterio de diseño ya usado en `patient_prep_notes` (Fase 8, documentado
  explícitamente por la misma razón).
- **Frontend**: cero — ni ruta, ni pestaña placeholder, ni ningún componente.

**Valor sin notificaciones del sistema**: sí, alto. Los cinco casos de uso que mencionas (revisar
Plan de Seguridad, seguimiento de tarea, pago pendiente, próxima acción, evaluación futura) son
todos, en esencia, "una lista de pendientes con fecha" — el mismo patrón que ya funciona bien en
este proyecto para `therapy_tasks`/`patient_prep_notes` (Fase 8), que tampoco usan notificaciones
del SO y ya demostraron ser útiles solo como listas visibles en el Dashboard y en la ficha del
paciente.

## 17. Notificaciones del sistema

No se implementa nada aquí. Conclusión: **sí, una "lista interna de pendientes" sin notificaciones
del SO es una V1 razonable de Recordatorios** — reduce la complejidad drásticamente (sin
permisos del SO, sin comportamiento diferente entre macOS/Windows, sin necesidad de que la app
esté corriendo en segundo plano) a cambio de depender de que la usuaria abra la aplicación para
verlos, exactamente el mismo trade-off ya aceptado para Continuidad (Fase 8).

## 18. Historial de cierres

Confirmado (sección 7, ítem A): `episodeClosuresApi.listHistory` (frontend, `api.ts` línea 47) y
`list_episode_closure_history` (comando Tauri, ya registrado en `lib.rs`) existen y funcionan —
verificado que están cubiertos por tests desde la Fase 11. Ningún componente de
`src/features/treatment-episodes/` los invoca.

**Qué falta exactamente para tener "Historial de cierres" en la UI**: una vista (probablemente
dentro de `TreatmentEpisodeDetailScreen.tsx`, junto a `ClosureSection.tsx` ya existente) que llame
a `episodeClosuresApi.listHistory(episodeId)` y renderice la lista de `EpisodeClosure[]` — motivo,
resultado, fecha, y si fue anulado. **No requiere ningún cambio de backend, ningún tipo nuevo,
ninguna migración.**

**Complejidad estimada**: **BAJA** — es, en esencia, una tabla de solo lectura sobre datos que ya
existen y ya están tipados en TypeScript, con un patrón de "abrir/cerrar" ya usado en varias otras
pantallas del proyecto (ej. `SafetyPlanHistoryView`, que resuelve exactamente el mismo problema de
forma para otro dominio).

**¿Conviene como microfase antes de una vertical grande?** Sí — es la recomendación de esta
auditoría (ver secciones 24/26/28): cierra una deuda ya documentada desde la Fase 11 con esfuerzo
mínimo, antes de abrir cualquier vertical nueva de mayor complejidad.

## 19. Línea temporal

Confirmado: la pestaña existe solo como placeholder ("Próximamente"), sin tabla propia en el
esquema, sin ningún componente en `src/features/`.

**¿Puede construirse sin tabla nueva?** Sí — sería una vista de **lectura agregada** sobre entidades
ya existentes, cada una con su propia fecha: sesiones (`session_date`), objetivos (`created_at`/
`target_date`), evaluaciones (`administered_at`), plan de seguridad (`confirmed_at`/`reviewed_at`),
pagos (`paid_at`/`due_date`), cierres/reaperturas de proceso (`closed_at`/`reverted_at`). Ninguna
tabla nueva sería estrictamente necesaria — una función de agregación en el backend que consulte
varias tablas y combine resultados por fecha, ordenados, bastaría.

**Distinción pedida — no meter todo automáticamente**:

- **Eventos clínicos** (candidatos): sesión realizada, objetivo creado/logrado, evaluación
  administrada, plan de seguridad confirmado/actualizado, proceso iniciado/pausado/cerrado/
  reabierto.
- **Eventos administrativos** (candidatos, más discutibles en una timeline clínica): pago
  registrado/vencido, tarea de continuidad asignada/resuelta.

Es razonable que una primera versión de la línea temporal muestre **solo eventos clínicos** con un
filtro explícito para administrativos (u omitirlos por completo en V1), en vez de mezclar ambos
tipos sin distinción — decisión de producto pendiente, no resuelta aquí.

## 20. Privacidad de línea temporal

Evaluado sin implementar: una timeline mal diseñada podría convertirse, sin quererlo, en el
resumen clínico más denso de toda la aplicación en una sola pantalla. Recomendación de diseño (no
implementada): mostrar **solo tipo de evento + fecha** en la vista principal (ej. "Evaluación
administrada — 10 de enero"), nunca el contenido narrativo directamente en la lista — el mismo
criterio de minimización de IPC/UI ya aplicado consistentemente en `SafetyPlanSummary` (Fase 12) y
`AssessmentAdministrationSummary` (Fase 13). El contenido completo de un evento (la interpretación
de una evaluación, el contenido de una nota) debería requerir un click explícito hacia la pantalla
de detalle ya existente de esa entidad — la timeline sería un índice, nunca un contenedor de
contenido clínico narrativo por sí misma.

## 21. Export

Auditado sin decidir una librería:

- **Candidatos de exportación reales**: ficha de paciente (resumen), un proceso terapéutico
  completo, historial de sesiones, un cierre de proceso, un plan de seguridad (para entregar
  físicamente a la persona o a otro profesional en una derivación), una evaluación con su
  evolución, un resumen de pagos.
- **Formato**: PDF es el más natural para "entregar a alguien" (plan de seguridad, informe de
  cierre); Markdown serviría como formato intermedio legible/editable; CSV tendría sentido casi
  exclusivamente para Pagos (tabla numérica, útil para llevar a una planilla externa) — pero ningún
  formato se decide en esta auditoría.
- **¿V1 o V1.1?** Ninguna función clínica de este proyecto depende de que Export exista — todo el
  trabajo clínico (registrar, versionar, consultar) ya funciona sin exportación. Es una capa de
  "salida" sobre datos que ya existen, no un vertical de datos nuevo. Argumento a favor de V1.1: no
  bloquea ningún flujo de trabajo diario; argumento a favor de V1: la entrega física de un Plan de
  Seguridad a la persona atendida es, en la práctica clínica real, un caso de uso frecuente que hoy
  no tiene ninguna vía dentro de la aplicación (solo copiar/pegar texto manualmente). Se deja como
  pregunta abierta para tu decisión en la sección 31, no como una recomendación cerrada aquí.

## 22. Biblioteca

`library_resources.file_document_id REFERENCES documents(id) ON DELETE SET NULL` — **nullable**,
así que Biblioteca **no depende estrictamente** de Documentos para existir: un recurso puede ser
solo un enlace externo (`source_url`) o un resumen de texto sin ningún archivo adjunto. Documentos
es prerequisito únicamente para la función específica de "adjuntar el archivo completo de un
recurso" (ej. subir el PDF de un protocolo), no para el catálogo en sí.

**Clasificación**: probablemente no es V1 — es una utilidad de organización personal de la
profesional (protocolos, artículos, referencias), sin urgencia clínica directa comparado con
cualquier otra vertical pendiente. **MEJORA / POST-V1.**

## 23. macOS

Confirmado directamente: este proyecto **nunca se ha compilado, instalado ni ejecutado en un
macOS real** en toda su historia — todas las fases se desarrollaron y probaron en este mismo tipo
de entorno Linux headless (Xvfb cuando fue posible). `tauri.conf.json` no tiene ninguna
configuración de firma de código (`bundle` solo define íconos y `targets: "all"`, sin sección de
firma/notarización de macOS). `keyring` (Keychain) está en el código pero nunca ejercitado contra
un Keychain real.

**Qué falta exactamente**:

- Una máquina macOS real (o CI con runner macOS) para `cargo tauri build` generando `.app`/`.dmg`
  reales.
- Verificar que SQLCipher (vendorizado vía `bundled-sqlcipher-vendored-openssl`) compila y enlaza
  correctamente en el toolchain de macOS (nunca verificado — solo Linux hasta ahora).
- Verificar Keychain real (guardar/leer el refresh token de Google, sección 10).
- Verificar el diálogo de archivos nativo (`tauri-plugin-dialog`) en macOS real.
- Un ciclo real de Backup/Restore sobre el sistema de archivos de macOS.
- OAuth con navegador real (el flujo PKCE nunca se completó de punta a punta contra Google, en
  ningún sistema operativo, sección 10).
- Lock/unlock, migración de esquema, y reinicio completo del proceso — todo esto está probado a
  nivel de lógica (tests de Rust), pero nunca en un binario `.app` real corriendo en macOS.
- Firma de código y notarización de Apple — **cero configuración existente** — sin esto, macOS
  moderno bloquea o advierte fuertemente contra abrir la aplicación.

**No confundir build Linux con macOS verificado**: correcto, y confirmado que hasta hoy solo existe
lo primero.

## 24. Windows

Mismo análisis, mismo resultado: **nunca ejecutado en Windows real**.

- Build: nunca generado un `.msi`/`.exe` real (`tauri.conf.json` no tiene ninguna configuración
  específica de Windows más allá de heredar `targets: "all"`).
- WebView2: Tauri en Windows depende de que el sistema tenga WebView2 instalado — nunca verificado
  si el proyecto necesita empaquetar el instalador de WebView2 o asumir que ya está presente
  (viene preinstalado en Windows 11 y en la mayoría de instalaciones de Windows 10 actualizadas,
  pero no universalmente).
- SQLCipher: mismo caso que macOS — nunca compilado/enlazado en el toolchain de Windows.
- Credential Manager: `keyring` lo soporta en teoría, nunca ejercitado contra un Credential Manager
  real.
- OAuth loopback: el flujo de Google usa PKCE con un listener local — nunca probado si el
  comportamiento de loopback/firewall de Windows introduce alguna fricción particular.
- Filesystem/Backup/restore/lock-unlock: mismo caso, probado solo a nivel de lógica, nunca en un
  binario real de Windows.

**No se marca nada como probado sin prueba física** — correcto, se aplica ese criterio en toda
esta sección y la anterior.

## 25. Instaladores

Milestone propuesto: **RC DESKTOP / INSTALLABLE FOR TESTING**. Debe existir, como mínimo:

- Build real de macOS (`.app`/`.dmg`) y de Windows (`.msi`/`.exe`), generados por `cargo tauri
  build` en cada sistema operativo real respectivamente (no cross-compilado sin verificar).
- Instalación limpia en una máquina sin el proyecto instalado previamente.
- Instalación de actualización (una versión anterior ya instalada, sobrescrita por una nueva) —
  verificando que el vault existente persiste intacto y que las migraciones se aplican
  correctamente sobre datos reales, no solo sobre fixtures de test.
- El vault persiste correctamente a través de un ciclo completo de cerrar la app → reabrir.
- Un ciclo real de Backup → modificar → Restore sobre el sistema de archivos real del SO de
  destino.
- Comportamiento de desinstalación verificado explícitamente: ¿la desinstalación borra el vault
  del usuario, o lo deja intacto? (Decisión de producto no tomada todavía — con implicancia directa
  en la regla no negociable de "nunca perder datos clínicos": un desinstalador que borre el vault
  por defecto sería, en la práctica, una forma de pérdida de datos no advertida.)
- Recuperación tras un cierre inesperado (crash) seguido de reapertura.
- Una prueba real de OAuth contra una cuenta de Google (sección 10).

## 26. Hardening pre-V1

| Ítem | Clasificación |
|---|---|
| SQL directo en `services::episode_closures::today_utc_date` (sección 7.F) | **SHOULD FIX** — inconsistencia de capas, sin impacto funcional; corregible en minutos moviendo la consulta a `repositories::episode_closures`. |
| Warnings de lint (23, todos preexistentes/mismo patrón) | **CAN WAIT** — ninguno es un error, todos de la misma categoría react ya aceptada en todo el proyecto. |
| Comandos "muertos" (sin usar desde el frontend) | No se detectó ninguno en Fase 13 — todos los comandos nuevos de `commands/assessments.rs` están conectados a `AssessmentsTab.tsx`. |
| Semántica de archivado (`patients.status` vs `deleted_at`) | **MUST FIX BEFORE V1 → ya resuelto** (sección 7.G), sin trabajo pendiente. |
| UI de historial de cierres (sección 18) | **SHOULD FIX** — deuda documentada desde Fase 11, esfuerzo bajo. |
| Gaps de prueba manual GUI (sección 4) | **MUST FIX BEFORE V1** (antes de RC/uso real, no antes de continuar desarrollando). |
| Limitación de UTC en `date('now')` (Pagos, Sesiones) | **CAN WAIT** — ya documentada explícitamente desde Fase 7, es una limitación conocida y aceptada, no un bug oculto. |
| Fixtures de restauración histórica | No se detectó ninguna deuda nueva — los tests de migración (`V1`→`V7`) cubren explícitamente preservación de datos anteriores en cada transición. |
| Prueba real de Google Calendar (sección 10) | **MUST FIX BEFORE V1** si Calendar se ofrece como función productiva (tu preferencia explícita). |

## 27. Manual acceptance test

Diseño conceptual de un **"Pre-V1 Manual Acceptance Test"**, sin ejecutarlo:

Un único protocolo, ejecutado una sola vez en hardware real (macOS y Windows, por separado), que
cubra en una sola pasada: creación de vault → paciente ficticio completo → un proceso terapéutico
con sesiones, objetivos, antecedentes → un pago → una tarea de continuidad → un plan de seguridad
(crear, actualizar copiando contactos, confirmar, archivar el paciente y verificar los bloqueos) →
una evaluación (catálogo, administración, evolución, edición, archivado) → un cierre de proceso y
una reapertura → un backup real → modificar datos → restaurar → verificar exactamente el estado
anterior → cerrar la aplicación → reabrir → verificar persistencia completa → (si aplica) una
conexión real a Google Calendar → auditoría de privacidad con un marcador ficticio buscado en todo
el disco. Un solo recorrido that agrupa los pendientes de Fase 12 y Fase 13 en vez de repetir
mini-validaciones separadas cada vez que este entorno remoto no puede ejecutarlas.

## 28. V1 clínica

**"V1 clínica" = las funciones de trabajo clínico necesarias para que una profesional use la
aplicación en su práctica diaria del inicio al fin de un proceso terapéutico**, sin necesidad de
ningún papel ni sistema externo para lo esencial: ingreso del paciente, antecedentes, planificación
(objetivos), registro de sesiones con notas versionadas, evaluación psicométrica, seguimiento entre
sesiones, gestión de pagos, plan de seguridad cuando corresponde, y cierre/alta del proceso.

Con Fase 13 cerrada, **todo lo anterior ya existe y funciona** (con la validación manual pendiente
de la sección 4). Lo que falta para una "V1 clínica" más completa —no imprescindible, mejora real—
es Formulación (documentar hipótesis clínicas de forma estructurada) y, en menor medida,
Recordatorios (seguimiento proactivo de pendientes).

## 29. V1 operacional

**"V1 operacional" = la aplicación realmente se instala, se actualiza y protege los datos de la
usuaria en un computador real**, con o sin funciones clínicas adicionales. Esto incluye
instaladores reales verificados (sección 25), backup/restore probado en producción real (no solo en
tests, sección 9), y, en un sentido más amplio pero explícitamente fuera de "imprescindible", un
mecanismo de actualización (updater) y firma de código.

**Diagnóstico directo, como pediste**: clínicamente el proyecto está considerablemente más
avanzado que operacionalmente. Ningún build ha corrido nunca en un macOS o Windows real; no existe
ninguna configuración de firma de código; no existe ningún mecanismo de actualización. La brecha
operacional es hoy más ancha que la brecha clínica.

## 30. V2 multiplataforma

Fuera de alcance de cualquier fase actual, por regla permanente del proyecto: Sync,
iOS/iPadOS y multiusuario requieren su propia fase dedicada con E2EE, gestión de dispositivos,
resolución de conflictos y compatibilidad explícita entre macOS/Windows/iOS/iPadOS diseñada desde
el principio (nunca "Apple primero"). Nada de esta auditoría recomienda avanzar en esa dirección
ahora.

## 31. Matriz de candidatos

Puntuación 1–5 (5 = más alto) sobre los nueve candidatos pedidos:

| Candidato | Valor clínico | Frecuencia | Valor V1 | Complejidad | Riesgo técnico | Riesgo privacidad | Migración | Dependencia | Retrabajo | Valor operacional | Facilidad de probar | Impacto multiplataforma |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A. Historial de cierres + Línea temporal | 3 | 2 | 2 | 1 | 1 | 2 | 0 (sin migración) | 0 | 0 | 1 | 5 | 1 |
| B. Formulación textual | 4 | 3 | 3 | 2 | 1 | 2 | 0-1 (posible ajuste menor) | 0 | 0 | 1 | 4 | 1 |
| C. Formulación visual completa | 4 | 3 | 3 | 5 | 3 | 2 | 0 | 4 (React Flow, no evaluada) | 2 (si B se hace primero, la UI de A se reutiliza) | 1 | 2 | 2 |
| D. Documentos cifrados | 4 | 4 | 4 | 5 | 5 | 5 | 0 (esquema ya existe) | 2-3 (posible crate KDF/AEAD adicional) | 1 | 3 | 2 | 3 |
| E. Recordatorios internos | 3 | 3 | 2 | 2 | 1 | 2 | 0 | 0 | 0 | 1 | 4 | 1 |
| F. Export | 3 | 2 | 2 | 2 | 1 | 3 | 0 | 1-2 (según formato) | 0 | 1 | 3 | 1 |
| G. Validación macOS real | 1 | 1 | 5 | 3 | 2 | 1 | 0 | 0 | 0 | 5 | 2 (requiere hardware) | 5 |
| H. Validación Windows real | 1 | 1 | 5 | 3 | 2 | 1 | 0 | 0 | 0 | 5 | 2 (requiere hardware) | 5 |
| I. Hardening/cleanup pre-V1 | 1 | 1 | 4 | 1 | 1 | 2 | 0 | 0 | 0 | 4 | 5 | 1 |

(Escala de "Riesgo técnico"/"Riesgo privacidad"/"Complejidad"/"Dependencia"/"Retrabajo": 5 = mayor
riesgo/complejidad/dependencia, no "mejor". "Migración": 0 = ninguna necesaria.)

## 32. Roadmap rápido

**ROADMAP A — V1 más rápida** (solo lo imprescindible):

1. Hardening pre-V1 (sección 26, ítems SHOULD/MUST FIX) + microfase Historial de cierres (esfuerzo
   trivial, cierra deuda documentada).
2. Pre-V1 Manual Acceptance Test (sección 27) — validación manual completa de Plan de Seguridad +
   Evaluaciones + todo lo demás en un solo recorrido.
3. Validación macOS real + Validación Windows real (secciones 23-24) + Instaladores (sección 25).
4. Prueba real de Google Calendar contra una cuenta real (si Calendar se mantiene como función
   productiva).
5. RC Desktop.

**Sin Documentos, sin Formulación, sin Recordatorios, sin Export** — todos post-V1/V1.1. Costo:
bajo, riesgo bajo, pero entrega una V1 sin capacidad de adjuntar archivos clínicos (una carencia
real para uso profesional diario, ver sección 34) y sin formulación estructurada.

**ROADMAP B — V1 más completa** (incluye verticales de alto valor antes de release):

1. Hardening pre-V1 + Historial de cierres (igual que A).
2. Formulación textual (Roadmap A de contenido, sección 11) — reutiliza `summary_text`, sin
   migración, complejidad baja-media.
3. Documentos cifrados — **precedido por una decisión explícita de diseño criptográfico (sección
   13, RIESGO ALTO)** antes de escribir una sola línea de código.
4. Recordatorios internos (complejidad baja, alto valor de continuidad).
5. Pre-V1 Manual Acceptance Test (ampliado para cubrir también Formulación/Documentos/
   Recordatorios).
6. Validación macOS/Windows real + Instaladores.
7. RC Desktop.

**Costo/riesgo**: notablemente mayor que A (Documentos introduce la única decisión criptográfica
nueva de todo el proyecto desde Fase 1.4, y el único flujo con riesgo real de plaintext temporal),
pero entrega una V1 con la capacidad que, según el análisis de la sección 34, con mayor probabilidad
se considera necesaria (no solo "muy útil") para uso clínico profesional real.

## 33. Estado Git final

```
git status --porcelain=2
```
→ vacío (confirmado antes de escribir este archivo; este mismo archivo es el único agregado por
esta auditoría, y permanece sin commitear, tal como se pidió: "Commits: 0. Push: 0.").

- **HEAD inicial** (al empezar esta auditoría): `5427aee`.
- **HEAD final**: `5427aee` — sin cambios.
- **Commits creados por esta auditoría**: 0.
- **Push realizados**: 0.
- **Migraciones creadas**: 0.
- **Dependencias instaladas**: 0.
- **Archivos de código modificados**: 0.

---

## Recomendación obligatoria

1. **¿Fase 13 está técnicamente cerrada?** Sí — verificado directamente contra el código, sin
   ninguna discrepancia respecto a lo declarado en su informe de cierre.
2. **¿Qué significa la falta de GUI manual?** Validación operacional pendiente, no invalidación
   técnica — el código está probado exhaustivamente a nivel automatizado; falta la confirmación
   visual en una interfaz real.
3. **¿Bloquea continuar?** No, continuar desarrollando no está bloqueado. Sí bloquea declarar
   cualquier milestone de "instalable para pruebas" o "V1 lista para uso real" sin haberla resuelto
   primero.
4. **¿Qué debe ocurrir antes de V1?** Como mínimo: el Pre-V1 Manual Acceptance Test completo
   (sección 27), validación física real en macOS y Windows (secciones 23-24), instaladores
   verificados (sección 25), y una decisión explícita sobre Google Calendar en producción
   (sección 10).
5. **¿Qué debería ser la próxima fase?** Una **microfase de cierre de deuda UX longitudinal**:
   Historial de cierres en la UI (esfuerzo trivial) — antes de abrir cualquier vertical nueva de
   mayor tamaño. No Documentos todavía, no Formulación todavía.
6. **¿Debe ser grande o pequeña?** Pequeña, deliberadamente — el objetivo ahora es cerrar deuda
   barata antes de invertir en algo con una decisión de riesgo alto pendiente (Documentos) o una
   dependencia no evaluada (Formulación visual).
7. **¿Documentos ahora?** No todavía — no por inercia de "es lo que decía el informe anterior",
   sino porque tiene una decisión criptográfica real sin resolver (RIESGO ALTO, sección 13) que
   debe aprobarse explícitamente antes de escribir código, exactamente el mismo criterio ya usado
   para aprobar el esquema de envelope encryption en Fase 1.4.
8. **¿Formulación ahora?** No inmediatamente — pero es la vertical de mayor valor clínico entre las
   pendientes (matriz de la sección 31), y una versión **textual** (sin canvas visual, sin React
   Flow) es viable con complejidad baja-media y sin ninguna dependencia nueva. Candidata razonable
   para después de la microfase de deuda.
9. **¿Línea temporal ahora?** No como vertical propia — pero el Historial de cierres (que sí se
   recomienda ahora) es un primer paso pequeño hacia el mismo tipo de "vista agregada de solo
   lectura" que la Línea temporal necesitará más adelante.
10. **¿Validación macOS/Windows ahora?** Sí, en paralelo con la microfase de deuda — es
    independiente del desarrollo de nuevas verticales y tiene el mayor "valor operacional" de toda
    la matriz (sección 31), con el trade-off explícito de que requiere hardware real que este
    entorno no tiene.
11. **¿Qué dejarías fuera de V1?** Biblioteca (mejora/post-V1, sección 22), Export (pregunta
    abierta hacia ti en la sección siguiente, pero con argumento razonable para V1.1), Sync/iOS/
    iPad/multiusuario (V2 explícito, fuera de discusión para V1).
12. **¿Cuántas fases aproximadas faltan hasta RC desktop?** Estimación cualitativa: 2 a 3 —
    (i) microfase de deuda + validación manual agrupada, (ii) validación física macOS/Windows +
    instaladores, y opcionalmente (iii) una ronda de correcciones que esa validación real
    seguramente revele (nunca se ha probado en hardware real, es razonable esperar hallazgos).
13. **¿Cuántas hasta V1 desktop?** 4 a 6 — las de RC más, como mínimo, Formulación textual; si
    decides que Documentos es necesario para V1 (sección 34), una fase adicional dedicada
    exclusivamente a la decisión criptográfica antes de la fase de implementación misma.
14. **¿Qué orden propones?** Microfase de deuda (Historial de cierres) → Pre-V1 Manual Acceptance
    Test → Validación macOS/Windows real + instaladores → Formulación textual → (decisión sobre
    Documentos) → RC → V1.

## Decisiones que necesito aprobar

**A. Alcance V1**: ¿confirmas que Sync/iOS/iPad/multiusuario quedan fuera de cualquier fase
próxima (ya establecido como regla permanente, se re-confirma aquí solo para que quede explícito en
este documento de decisión)?

**B. Siguiente fase**: ¿apruebas la microfase pequeña de "Historial de cierres en la UI" como
próximo paso, antes de cualquier vertical grande?

**C. Documentos**: dado el RIESGO ALTO de la decisión criptográfica (sección 13) y el flujo de
plaintext temporal (sección 14), ¿quieres que la próxima fase dedicada a Documentos empiece
**exclusivamente** por la decisión de diseño criptográfico (sin código todavía, mismo formato que
`Plan-Fase-13-pendiente-de-aprobacion.md`), antes de cualquier implementación?

**D. Formulación**: ¿confirmas la estrategia textual primero (sección 11, opción A) como la vía a
seguir cuando corresponda, dejando el canvas visual (React Flow u otra librería) como una fase
separada y explícitamente evaluada más adelante, nunca asumida?

**E. Distribución**: ¿tienes acceso a hardware macOS y/o Windows reales (propio o vía un servicio
de CI con runners de esos sistemas) para ejecutar la validación de las secciones 23-25? Esta
auditoría no puede determinar la vía concreta sin saber qué tienes disponible.

**F. Hardening**: ¿apruebas mover `services::episode_closures::today_utc_date` (SQL directo) a
`repositories::episode_closures` como parte de la microfase de deuda, o prefieres agruparlo con
otra corrección posterior?

## Regla de detención

No se encontró ningún bug grave, fuga de privacidad, Backup roto, corrupción de esquema, test rojo,
envío indebido de contenido por Google Calendar, ni ningún problema de integridad durante esta
auditoría. La única anomalía real (discrepancia de HEAD, sección 1) fue detenida, explicada y
resuelta explícitamente contigo antes de continuar, como corresponde. **No fue necesario aplicar la
regla de detención de la sección 45 más allá de ese punto — el resto de la auditoría se completó
con normalidad.**
