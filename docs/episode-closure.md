# Cierre estructurado de procesos terapéuticos (Fase 11)

Documento técnico de la Fase 11. Complementa `docs/ARCHITECTURE.md` (tabla de fases, sección 17) y
`docs/treatment-episodes.md` (Fase 9, modelo base de `treatment_episodes`). Resuelve la limitación
documentada explícitamente al cierre de esa fase: `status = 'cerrado'` era legal en el esquema pero
**inalcanzable** desde cualquier flujo real — `services::treatment_episodes::set_episode_status`
rechazaba incondicionalmente ese valor con `ClosureNotImplemented`. Esta fase construye el único
camino real hacia ese estado: un cierre **estructurado, histórico y trazable**, nunca un simple
cambio de campo.

**Principio rector, tomado literalmente de la aprobación de fase**: esta fase cierra un **proceso
terapéutico**, nunca "da de alta al paciente" como si paciente y proceso fueran la misma entidad.
El historial clínico longitudinal pertenece al paciente; el motivo de consulta y el diagnóstico
pertenecen al proceso; sesiones y objetivos pueden pertenecer al proceso; el cierre pertenece al
proceso. Un reingreso crea un proceso nuevo. Nada de esto llega nunca a Google Calendar.

## Modelo de datos: migración `V5`

Aditiva sobre `SCHEMA_V1`–`V4`, que quedan intactos (verificado por
`v5_migration_creates_episode_closures_and_preserves_v4_data`). Sin `DROP`, sin tocar ninguna
columna existente.

```sql
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
  CHECK ((reverted_at IS NULL AND reverted_reason IS NULL)
      OR (reverted_at IS NOT NULL AND reverted_reason IS NOT NULL))
);
CREATE INDEX idx_episode_closures_episode ON episode_closures(episode_id);
CREATE UNIQUE INDEX idx_episode_closures_active
  ON episode_closures(episode_id) WHERE reverted_at IS NULL;
```

### Por qué una tabla separada, 1:1-con-historial (Opción B)

Ya anticipado literalmente en un comentario de la migración `V4` (Fase 9): "una tabla
`episode_closures` separada, todavía sin crear". No se agregaron los campos de cierre directamente
a `treatment_episodes` (Opción A) porque eso impediría conservar el cierre original tras una
corrección por error — y la aprobación de Fase 11 exigió explícitamente que la reapertura nunca
borre el cierre anterior. Tampoco se diseñó un modelo versionado de múltiples filas mutables
(Opción C): la decisión explícita de la aprobación fue **inmutabilidad total** (ver más abajo).

El índice único parcial `idx_episode_closures_active` replica exactamente el patrón ya usado por
`idx_treatment_episodes_one_active_per_patient` (Fase 9) y por `idx_session_notes_current`
(Fase 4): como máximo **un** cierre vigente (`reverted_at IS NULL`) por proceso, garantizado a
nivel de base de datos y no solo de servicio — verificado directamente con SQL crudo en
`a_second_active_closure_for_the_same_episode_is_rejected_at_database_level`. Cierres anulados se
conservan sin límite: son historia, nunca se eliminan.

## Inmutabilidad: nunca se edita un cierre, se anula y se crea uno nuevo

Decisión explícita de la aprobación de Fase 11 (elegida frente a "editable con log" y "versionado"):
`repositories::episode_closures` **no tiene ninguna función `update` de contenido** — no existe
estructuralmente, no es solo una regla de servicio. La única mutación posible sobre una fila de
`episode_closures` es `revert(conn, id, reverted_reason)`, que únicamente escribe
`reverted_at`/`reverted_reason`; el resto de los campos del cierre original permanece exactamente
como se creó, para siempre.

Corregir un cierre hecho por error es, en consecuencia, siempre una secuencia de dos pasos:
`revert_closure` (anula el cierre vigente, reabre el proceso al estado elegido) seguido de un
`close_episode` nuevo si corresponde. Nunca hay un tercer camino.

## Taxonomía de motivo (`reason`) — 6 categorías fijas

Aprobada explícitamente en el Bloque C de Fase 11 ("propuesta de 6, recomendada"), fijada en el
`CHECK` de `SCHEMA_V5`:

| Valor | Etiqueta en la UI |
|---|---|
| `alta` | Alta terapéutica |
| `cierre_acordado` | Cierre acordado |
| `interrupcion` | Interrupción del proceso |
| `derivacion` | Derivación |
| `decision_profesional` | Decisión profesional |
| `otro` | Otro |

`otro` exige `reason_detail` no vacío (`EpisodeClosureError::MissingReasonDetail`); para las demás
categorías `reason_detail` es opcional. Sin lenguaje estigmatizante, sin categorías redundantes
entre sí — cambiar esta taxonomía exige otra migración, no es un `enum` de solo frontend.

## Resultado (`outcome`) — independiente del motivo

`objetivos_logrados` / `parcialmente_logrados` / `no_logrados` / `no_evaluable`, campo separado de
`reason` a propósito: el ejemplo explícito de la aprobación (una derivación puede coexistir con
objetivos parcialmente logrados) está cubierto — verificado manualmente cerrando un proceso con
`derivacion` + `parcialmente_logrados` simultáneamente, y no hay ninguna regla que acople ambos
campos.

## Campos mínimos, sin duplicar contenido existente

`summary` (resumen del proceso) y `recommendations` son texto libre opcional, **nunca** una copia
automática de `session_notes`, del diagnóstico o de los objetivos — la persona que cierra el
proceso escribe su propio resumen si lo desea. `episode_closures` no tiene columnas de
"antecedentes", "diagnóstico" ni "notas de sesión": esos datos siguen viviendo exclusivamente en
`episode_clinical_profile` y `session_notes`, y la vista histórica del cierre los **referencia**
(objetivos relacionados, sesiones del proceso) en vez de copiarlos.

## Sesiones futuras: resolución manual explícita, obligatoria

Decisión explícita de la aprobación (elegida frente a "advertir y permitir", "bloquear
completamente" y "desvincular automáticamente"): cerrar un proceso con sesiones futuras
(`status = 'programada' AND session_date > date('now')`, `sessions::list_upcoming_by_episode`)
exige que el llamador envíe `session_resolutions: [{ sessionId, cancel }]` cubriendo **exactamente**
ese conjunto — ni de más ni de menos, y la exigencia está en el backend, no solo en la UI:

- Falta resolver alguna → `EpisodeClosureError::PendingSessionResolutionRequired(ids)`.
- Se incluye una sesión que no es futura-agendada de este proceso →
  `EpisodeClosureError::UnknownSessionInResolution(id)`.
- `cancel: true` → la sesión pasa a `status = 'cancelada'` (mismo valor ya usado por Sesiones,
  ninguna migración de esquema nueva).
- `cancel: false` → la sesión queda exactamente igual, sin ningún campo tocado.

`sessions` (visitas clínicas) es una entidad distinta de `appointments` (citas de agenda /
Google Calendar) — esta resolución opera únicamente sobre `sessions`; la agenda no se toca desde
este flujo.

## Pendientes administrativos: nunca bloquean, nunca se pierden

Decisión explícita de la aprobación: "no bloquear artificialmente un cierre clínico real por una
tarea administrativa pendiente". `close_episode` **nunca** consulta ni exige nada sobre
`therapy_tasks`/`patient_prep_notes` — la única función nueva agregada,
`therapy_tasks::list_pending_or_partial_by_patient` (`status IN ('pendiente','parcial')`), se usa
exclusivamente para mostrar una advertencia **informativa, no bloqueante** en el modal de cierre
("Este paciente tiene N tarea(s) pendiente(s)... No se modificarán ni se perderán al cerrar el
proceso"). Ninguna tarea ni preparación de sesión se modifica, marca ni elimina como efecto del
cierre — verificado manualmente: tras cerrar un proceso con una tarea `pendiente` y una preparación
`pendiente`, ambas siguen exactamente iguales.

Nota de precisión heredada de Fase 8: `therapy_tasks`/`patient_prep_notes` no tienen `episode_id`
(decisión de Fase 8, no reabierta aquí), así que esta advertencia es **del paciente**, no
estrictamente del proceso que se está cerrando — la UI lo dice con esas palabras exactas, nunca
afirma una precisión que no tiene.

## Reapertura (corrección de un cierre por error)

`revert_closure(conn, closure_id, { reverted_reason, reopen_status })`:

- `reverted_reason` es obligatorio — un cierre nunca se anula en silencio.
- `reopen_status` es **siempre explícito** (`'activo'` o `'pausado'`) — decisión de la aprobación
  frente a "asumir siempre activo"; el backend rechaza cualquier otro valor
  (`InvalidReopenStatus`).
- El cierre debe existir y estar vigente (`ClosureNotFound`, `AlreadyReverted`).
- El proceso debe seguir `'cerrado'` (`EpisodeNotClosedForRevert`).

### El caso crítico: nunca dos procesos activos

Escenario explícitamente exigido por la aprobación, con test dedicado
(`reverting_to_activo_is_rejected_if_another_episode_became_active_meanwhile`): Proceso A se
cierra → Proceso B se crea (reingreso) y queda activo → se intenta reabrir A como `'activo'`.

Esto **nunca** puede producir dos procesos activos del mismo paciente. La solución reutiliza, sin
duplicar lógica, la regla `AnotherEpisodeActive` ya probada desde Fase 9
(`treatment_episodes::find_active_by_patient`): si `reopen_status = 'activo'` y ya existe otro
proceso activo del mismo paciente, `revert_closure` devuelve `AnotherEpisodeActive` y no escribe
nada. Reabrir A como `'pausado'` en ese mismo escenario sí es válido y no requiere el chequeo de
proceso activo — verificado automatizada y manualmente (UI: mensaje "este paciente ya tiene un
proceso activo — solo puede haber uno a la vez").

## Reingreso: proceso nuevo, nunca una reapertura del anterior

Confirmado con evidencia de código, no solo de diseño: `create_episode` ya excluye procesos
`'cerrado'` en su verificación de proceso activo existente (`find_active_by_patient` filtra
`status = 'activo'`) — **cero código nuevo** fue necesario para que un reingreso funcione tras un
cierre. Un reingreso reutiliza el mismo paciente y sus antecedentes longitudinales
(`patient_clinical_profile`); el proceso nuevo empieza limpio, sin copiar automáticamente
diagnóstico, motivo de consulta u objetivos del proceso anterior — mismo principio ya establecido
en Fase 9, sin cambios.

## `patients.status`: sigue sin ser la fuente de verdad

Esta fase no toca `patients.status` en ningún punto. Cerrar o reabrir un proceso terapéutico
**nunca** escribe `patients.status = 'alta'` ni ningún otro valor — la fuente de verdad de "¿está
cerrado el proceso?" es exclusivamente `treatment_episodes.status` + `episode_closures`. La
inconsistencia preexistente entre pacientes con `status = 'alta'` (heredado, previo a Fase 9) y sus
procesos legacy no se corrigió con ninguna migración: sigue documentada, no resuelta, exactamente
como al cierre de Fase 9.

## Arquitectura

Mismas capas que el resto de las verticales:

```
React (features/treatment-episodes/ClosureSection.tsx — modal de cierre y de reapertura)
   │  invoke('close_treatment_episode', ...), invoke('revert_episode_closure', ...), etc.
   ▼
commands::episode_closures   (7 comandos, capa fina sobre VaultSession::with_connection)
   │
services::episode_closures
   │  — close_episode, revert_closure, get_active_closure, list_closure_history.
   │  — reutiliza services::treatment_episodes::AnotherEpisodeActive (Fase 9) sin duplicar lógica.
   ▼
repositories::episode_closures   (insert, find_by_id, find_active_by_episode,
   │                               list_history_by_episode, revert — sin update de contenido)
   ▼
SQLite + SQLCipher (vault.db, migración V5)
```

`close_episode` y `revert_closure` son transaccionales (`conn.unchecked_transaction()`, mismo
patrón que `services::sessions::create_session` desde fases anteriores): cancelar sesiones futuras
+ insertar el cierre + cambiar el estado del proceso ocurre como una sola unidad atómica; anular un
cierre + reabrir el proceso, igual.

## UI: nunca "episodio", siempre "Proceso"

`ClosureSection.tsx`, integrado en `TreatmentEpisodeDetailScreen`:

- **Proceso sin cierre vigente**: botón "Cerrar proceso" abre un modal deliberado — nunca un botón
  instantáneo — que revisa fecha de término, motivo, resultado, resumen, recomendaciones, y (si
  corresponde) exige resolver cada sesión futura antes de habilitar el envío. Muestra las
  advertencias no bloqueantes de tareas/preparaciones pendientes.
- **Proceso con cierre vigente**: vista histórica de solo lectura — fecha de cierre, motivo,
  resultado, resumen, recomendaciones, objetivos relacionados del proceso (con su estado actual en
  vivo vía `list_episode_goals`) y sesiones del proceso (`list_episode_sessions`) — más un botón
  "Reabrir proceso" que abre el modal de reapertura (motivo de reapertura + elección explícita
  activo/pausado).
- El botón "Cerrar proceso" del detalle del proceso desaparece en cuanto existe un cierre vigente;
  no puede crearse una sesión u objetivo nuevo **asignado explícitamente** a ese proceso (rechazado
  por `check_episode_assignable`, Fase 9, sin cambios) — una sesión/objetivo *sin* proceso asociado
  sigue siendo posible, exactamente como antes de esta fase (`episode_id` opcional, decisión de
  Fase 9, no reabierta aquí).

La palabra "episodio"/"treatment_episode" nunca aparece en ningún texto visible — siempre
"Proceso" / "Proceso terapéutico", mismo criterio que Fase 9.

## Editabilidad tras el cierre

Decidido explícitamente en la aprobación de Fase 11: **inmutable, solo anular + recrear** (frente a
"editable simple", "editable con log" y "versionado" — a diferencia del patrón de `session_notes`,
que sí es editable con versionado; se evaluó ese patrón y se descartó deliberadamente para el
cierre, por ser un evento clínico distinto con menor frecuencia de corrección esperada y mayor
necesidad de trazabilidad exacta del original).

## Backup / Restore: sin cambios de diseño

`backup::service::current_app_schema_version()` calcula la versión soportada dinámicamente
corriendo `db::run_migrations` sobre una base en memoria — `SCHEMA_V5` no exigió ningún cambio de
diseño en Backup/Restore, solo dos literales de test mecánicos (`schema_version: 4 → 5`,
`supported_schema_version: 4 → 5`). Verificado manualmente con un ciclo completo: vault con un
proceso cerrado y otro pausado con un cierre anulado en su historial → crear respaldo → modificar
datos (paciente nuevo) → restaurar → el paciente nuevo desaparece, ambos procesos y toda su
historia de cierres vuelven exactamente iguales. Un respaldo corrupto (truncado) es rechazado con
un mensaje genérico ("el archivo de respaldo no se pudo leer") sin tocar el vault activo — el
paciente de prueba original permaneció intacto tras el intento fallido.

## Privacidad

- **Sin logging de contenido clínico.** Ninguno de los archivos nuevos de backend
  (`repositories::episode_closures`, `services::episode_closures`, `commands::episode_closures`)
  contiene una sola llamada a `log::`/`println!`/`dbg!`/`eprintln!`.
- **Google Calendar intacto.** `src-tauri/src/calendar/*` no se modificó en esta fase; verificado
  por inspección directa de código: `calendar/client.rs::event_payload(starts_at, ends_at)` toma
  únicamente dos parámetros de tipo `&str` — no puede recibir estructuralmente ningún dato de
  cierre, motivo o resultado — y `EVENT_SUMMARY = "Sesión clínica"` es una constante fija, no
  interpolada. `grep` sobre `calendar/*.rs` no encuentra ninguna referencia real a
  `episode`/`closure`/`treatment` (las dos únicas coincidencias son la palabra "closure" en su
  sentido de Rust — una función `FnOnce` — dentro de un comentario de `sync.rs`, sin relación con
  el cierre clínico). Clasificación epistemológica: **VERIFICADO POR INSPECCIÓN DE CÓDIGO** para la
  imposibilidad estructural; **NO VERIFICADO CONTRA UNA CUENTA GOOGLE REAL** (no hay credenciales
  configuradas en el entorno de prueba — `Credenciales: No`, `Conexión: No`).
- **Sin `localStorage`/`sessionStorage`.** `grep` sobre todo `src/` confirma cero usos en cualquier
  parte del frontend, no solo en el código de esta fase.
- **Sin portapapeles.** Ninguna función de `ClosureSection.tsx` usa la API de clipboard.
- **Auditoría manual con marcador ficticio (`XYZFASE11CIERREPRIVACY`)**, sembrado en las notas
  diagnósticas de un proceso de prueba real (Fase 11): cero coincidencias, mediante `grep`
  binary-safe, en el log de la aplicación, en el directorio completo de datos/caché de WebKit
  (`WebKitCache`, `storage`, `CacheStorage`), en el respaldo `.cclinbackup` generado (confirmado
  cifrado, no texto plano) ni en ningún otro archivo fuera del propio `vault.db` — que a su vez se
  confirmó como `data` binario opaco (SQLCipher), no una base SQLite legible en claro. Título de
  ventana confirmado genérico ("Cuaderno Clínico"). Directorios `backup-scratch/` y
  `vault-restore-tmp/` confirmados vacíos tras las operaciones de respaldo/restauración —  sin
  archivos temporales en texto plano dejados atrás.

## Decisiones de negocio tomadas en esta fase

1. **Tabla separada `episode_closures`, 1:1-con-historial vía índice único parcial** (Opción B).
2. **Inmutable tras crearse — corregir es anular + recrear**, nunca editar.
3. **Taxonomía de motivo fija de 6 categorías**, `otro` exige detalle.
4. **`outcome` independiente de `reason`.**
5. **Sesiones futuras: resolución manual explícita y obligatoria**, exigida en el backend.
6. **Tareas/preparaciones pendientes: advertencia informativa, nunca bloqueo ni modificación.**
7. **Reapertura siempre pregunta el estado destino** (`activo`/`pausado`), nunca asume.
8. **La regla de un-solo-proceso-activo se aplica también a la reapertura**, reutilizando la
   validación de Fase 9 sin duplicarla.
9. **Reingreso confirmado como creación de un proceso nuevo**, sin código adicional necesario.
10. **`patients.status` permanece sin cambios de semántica** — sigue sin ser la fuente de verdad.

## Exclusiones explícitas de esta fase

Historial de cierres visible en una pantalla dedicada de la UI (existe `episodeClosuresApi.listHistory`
en la capa de API y está cubierto por tests de servicio/repositorio, pero no se construyó una vista
específica más allá del cierre vigente — ver Limitaciones), taxonomía de riesgo clínico o motor de
riesgo, deprecación física de `patients.status = 'alta'`, corrección de la inconsistencia
preexistente `patients.status` vs. `deleted_at`, estadísticas de cierres, Sync, iOS/iPadOS,
cualquier cambio a Google Calendar/OAuth, Export/PDF/Documentos/Evaluaciones/Formulación/Plan de
seguridad/Derivaciones como funcionalidad separada/Plantillas/Boletas.

## Archivos creados o modificados

| Archivo | Rol |
|---|---|
| `src-tauri/src/db/migrations.rs` | `SCHEMA_V5` (tabla `episode_closures`, dos índices), registro en `migrations()`, 6 tests nuevos. |
| `src-tauri/src/repositories/episode_closures.rs` (nuevo) | SQL puro: `insert`, `find_by_id`, `find_active_by_episode`, `list_history_by_episode`, `revert`. Sin `update` de contenido. |
| `src-tauri/src/services/episode_closures.rs` (nuevo) | `close_episode`, `revert_closure`, `get_active_closure`, `list_closure_history`; taxonomías `VALID_REASONS`/`VALID_OUTCOMES`/`VALID_REOPEN_STATUSES`. |
| `src-tauri/src/commands/episode_closures.rs` (nuevo) | 7 comandos Tauri. |
| `src-tauri/src/repositories/sessions.rs`, `services/sessions.rs` | `list_by_episode`, `list_upcoming_by_episode` (y sus wrappers de servicio). |
| `src-tauri/src/repositories/goals.rs` | `list_by_episode`. |
| `src-tauri/src/repositories/therapy_tasks.rs`, `services/therapy_tasks.rs`, `commands/therapy_tasks.rs` | `list_pending_or_partial_by_patient` — nueva, no reemplaza `list_pending_by_patient`. |
| `src-tauri/src/repositories/mod.rs`, `services/mod.rs`, `commands/mod.rs`, `lib.rs` | Registro de módulos y de los 8 comandos nuevos. |
| `src-tauri/src/backup/service.rs` | Dos literales de test (`schema_version`/`supported_schema_version`: `4 → 5`). |
| `src/features/treatment-episodes/ClosureSection.tsx` (nuevo) | `ClosureSection`, `CloseEpisodeModal`, `ReopenClosureModal`. |
| `src/features/treatment-episodes/types.ts` | `ClosureReason`, `ClosureOutcome`, `EpisodeClosure`, `CloseEpisodeInput`, `RevertClosureInput`, etiquetas en español. |
| `src/features/treatment-episodes/api.ts` | `episodeClosuresApi`; `listUpcomingSessions`/`listSessions`/`listGoals` en `treatmentEpisodesApi`. |
| `src/features/treatment-episodes/TreatmentEpisodeDetailScreen.tsx` | Integra `<ClosureSection>`. |
| `src/features/therapy-tasks/api.ts` | `listPendingOrPartial`. |

## Tests ejecutados

`cargo test` en `src-tauri/`: **570/570 en verde**. `cargo clippy --release --all-targets`: 0
advertencias. `cargo build --release`: sin errores. `npm run build`: sin errores. `npm run lint`:
21 warnings, todas de las mismas dos categorías preexistentes (`react(incompatible-library)`,
`react(set-state-in-effect)`) ya presentes en prácticamente todas las pantallas con `useEffect` de
carga de datos desde fases anteriores — incluida una nueva en `ClosureSection.tsx:48`, misma
categoría exacta que sus pares (`GoalsTab`, `SessionsTab`, `PaymentsTab`,
`TreatmentEpisodeDetailScreen`) — no es una regresión ni una categoría nueva.

Tests representativos de las reglas de negocio centrales:

| Requisito | Test |
|---|---|
| `V5` preserva datos de `V1`–`V4` | `db::migrations::v5_migration_creates_episode_closures_and_preserves_v4_data` |
| Un solo cierre vigente por proceso, a nivel de base de datos | `db::migrations::a_second_active_closure_for_the_same_episode_is_rejected_at_database_level` |
| `reason`/`outcome` inválidos rechazados por el `CHECK` | `db::migrations::episode_closure_rejects_invalid_reason` / `_invalid_outcome` |
| Cerrar con sesión futura sin resolver, rechazado | `services::episode_closures::rejects_closing_with_an_unresolved_future_session` |
| Resolución de una sesión que no es futura del proceso, rechazada | `services::episode_closures::rejects_an_unknown_session_in_resolution` |
| `cancel: true` marca la sesión cancelada / `cancel: false` no la toca | `closing_with_cancel_resolution_marks_the_session_cancelled` / `closing_with_keep_resolution_leaves_the_session_untouched` |
| `otro` exige detalle | `services::episode_closures::reason_otro_requires_a_detail` |
| Reapertura preserva el contenido original del cierre | `services::episode_closures::revert_reopens_to_activo_and_preserves_the_original_closure` |
| **Caso crítico**: A cerrado, B creado y activo, reabrir A como activo rechazado; como pausado, aceptado | `services::episode_closures::reverting_to_activo_is_rejected_if_another_episode_became_active_meanwhile` |
| Tras anular, se puede cerrar de nuevo con un cierre distinto, historial conserva ambos | `services::episode_closures::after_revert_the_episode_can_be_closed_again_with_a_new_closure` |
| Proceso legacy (Fase 9) se puede cerrar sin caso especial | `services::episode_closures::legacy_episode_can_be_closed` |

## Prueba manual realizada (aplicación real, no solo tests)

Compilada con `cargo build`, ejecutada bajo Xvfb con `xdotool` (clics y tecleo reales) sobre un
vault de prueba desechable (el vault real se guardó aparte antes de empezar y se restauró
exactamente al terminar), con capturas de pantalla en cada paso. Paciente ficticio
"Marcela Ibanez Soto".

- **Caso A/B — alta normal, con pendientes** — proceso con una tarea `pendiente`, una preparación
  `pendiente` y una sesión de hoy: cerrado con motivo "Alta terapéutica" + resultado "Objetivos
  logrados" + resumen + recomendaciones. Vista histórica confirmada completa (motivo, resultado,
  resumen, recomendaciones, objetivo relacionado, sesión del proceso). Tarea y preparación
  confirmadas sin modificar. Confirmado que una sesión/objetivo sin proceso asociado sigue
  pudiendo crearse (comportamiento heredado de Fase 9, no una brecha de esta fase) mientras que
  asignar explícitamente el proceso cerrado no es ni siquiera ofrecido por el selector.
- **Caso C — reingreso** — creado un Proceso 2 tras cerrar el Proceso 1: mismo paciente, mismos
  antecedentes longitudinales, Proceso 1 queda histórico, Proceso 2 arranca sin antecedentes
  propios (sin copia automática).
- **Caso D — corrección de un cierre por error** — "Reabrir proceso" sobre el Proceso 1 (ya
  cerrado con motivo "Derivación" + "Objetivos parcialmente logrados" en una segunda ronda de
  prueba), con motivo de reapertura y elección explícita de estado destino.
- **Caso E — conflicto A/B** — con Proceso 2 activo, reabrir Proceso 1 como "activo" rechazado en
  vivo por la UI ("este paciente ya tiene un proceso activo — solo puede haber uno a la vez");
  reabrir como "pausado" en el mismo estado, aceptado.
- **Caso F — persistencia** — ciclo bloquear/desbloquear: datos intactos. Cierre completo del
  proceso de la aplicación (`kill -9`) y relanzamiento: arranca bloqueado, desbloquear muestra
  Proceso 2 activo y Proceso 1 pausado exactamente como antes del reinicio.
- **Caso G — ciclo Backup/Restore** — respaldo creado con un proceso cerrado y otro con un cierre
  anulado en su historial; paciente nuevo agregado después; restaurado; el paciente nuevo
  desaparece y ambos procesos vuelven exactos. Backup corrupto (truncado) rechazado sin tocar el
  vault activo.
- **Caso H — privacidad** — marcador `XYZFASE11CIERREPRIVACY` sembrado en antecedentes de proceso:
  cero coincidencias fuera de `vault.db` cifrado, en ningún directorio de datos/caché de la
  aplicación, en el respaldo (cifrado) ni en los logs.

**Regresión funcional**: Pacientes (listado, ficha), Antecedentes (paciente y proceso), Sesiones,
Objetivos (estado en vivo intacto tras cierre/reapertura), Agenda, Pagos, Estadísticas, Dashboard —
todos revisados en el mismo flujo sin cambios de comportamiento respecto a fases anteriores.

## Limitaciones y decisiones que quedan pendientes de aprobación

- **Sin pantalla dedicada de historial de cierres.** `episodeClosuresApi.listHistory` existe y está
  cubierta por tests de repositorio/servicio (incluido el caso con dos cierres — uno anulado, uno
  vigente — para el mismo proceso), pero la UI de esta fase solo renderiza el cierre **vigente**.
  Un proceso con más de un cierre en su historia (tras una corrección) no tiene hoy una vista para
  repasar los cierres anulados anteriores desde la interfaz — verificable únicamente por evidencia
  de test/DB en esta fase. Se documenta como limitación explícita, no como algo "verificado" que no
  lo está: cualquier fase futura que agregue esa vista debe presentarse como su propio cambio.
- La verificación de aislamiento de Google Calendar es estructural (inspección de código +
  ausencia total de referencias), no una prueba contra una cuenta de Google real — no había
  credenciales configuradas en el entorno de prueba.
- La inconsistencia preexistente `patients.status = 'alta'` vs. procesos legacy `'cerrado'` (Fase 9)
  sigue sin resolverse — no se tocó en esta fase.
