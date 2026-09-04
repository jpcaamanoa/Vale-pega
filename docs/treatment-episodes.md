# Procesos terapéuticos / Episodios clínicos (Fase 9)

Documento técnico de la Fase 9. Complementa `docs/ARCHITECTURE.md` (tabla de fases, sección 17).
Resuelve el problema estructural **"paciente ≠ proceso"**, identificado en la auditoría posterior
a la Fase 8: hasta esta fase, `sessions`/`therapeutic_goals`/`patient_clinical_profile` estaban
ancladas únicamente a `patient_id`, sin ninguna forma de distinguir dos procesos terapéuticos
distintos de un mismo paciente a lo largo del tiempo (por ejemplo, un alta y un reingreso años
después con un motivo de consulta y un diagnóstico completamente distintos).

En código, la entidad se llama `treatment_episodes` (nombre técnico, coherente con el resto del
esquema en inglés). **En la interfaz nunca se muestra la palabra "episodio"** — la usuaria ve
siempre "Proceso" / "Proceso terapéutico". Este documento usa ambos términos indistintamente según
el contexto (técnico vs. producto).

## Propósito y alcance

Modelo deliberadamente mínimo: `id, patient_id, started_at, status, created_at, updated_at,
deleted_at`. Explícitamente fuera de alcance en esta fase — pertenecen a la futura Fase 10
(Cierre/Alta): `reason_for_end`, `closure_summary`, `recommendations`, `outcome`,
`final_assessment`, `achieved_goals`, `reason_for_closure`. No se creó ninguna tabla
`episode_closures`. Ninguna dependencia nueva.

También fuera de alcance por defecto (no reciben `episode_id`): `payments`, `patient_prep_notes`,
`therapy_tasks`, `documents`, `assessment_administrations`, `case_formulations`, `reminders`,
`appointments`. Ninguna implementación real demostró que alguna de estas tablas necesitara
`episode_id` para evitar una inconsistencia concreta introducida por esta fase — se documenta la
decisión, no se amplía el alcance.

`appointments` en particular **no** recibe `episode_id`: la agenda puede existir antes de que
exista ningún proceso formal (por ejemplo, agendar la primera entrevista). `src-tauri/src/calendar/*`
no se tocó — verificado por `grep` (cero referencias a `episode`/`diagnosis`/`presenting_problem`
en ese módulo) — y ningún dato de proceso (motivo, diagnóstico, nombre o estado de proceso) se
envía nunca a Google Calendar.

## Modelo de datos: migración `V4`

Migración aditiva sobre `SCHEMA_V1`/`V2`/`V3`, que quedan intactos (verificado por
`v4_migration_preserves_all_existing_data_and_does_not_touch_patient_clinical_profile`). Sin
`DROP`, sin eliminar datos, sin resetear el vault.

```sql
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

CREATE TABLE episode_clinical_profile (
  episode_id TEXT PRIMARY KEY REFERENCES treatment_episodes(id) ON DELETE RESTRICT,
  presenting_problem TEXT,
  primary_diagnosis_code TEXT,
  diagnosis_notes TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

ALTER TABLE sessions ADD COLUMN episode_id TEXT REFERENCES treatment_episodes(id) ON DELETE SET NULL;
ALTER TABLE therapeutic_goals ADD COLUMN episode_id TEXT REFERENCES treatment_episodes(id) ON DELETE SET NULL;
```

Ambas tablas nuevas llevan su propio trigger `trg_*_touch_updated_at`, mismo patrón que el resto
del esquema desde `SCHEMA_V1`. `sessions.patient_id` y `therapeutic_goals.patient_id` **siguen
siendo obligatorios** — `episode_id` es puramente opcional en ambas: una sesión (por ejemplo, una
única entrevista de evaluación antes de formalizar un proceso) puede seguir existiendo sin
proceso asociado.

### `'cerrado'`: legal en el esquema, inalcanzable en esta fase (Opción A)

`'cerrado'` es un valor válido en el `CHECK` de `status` — el esquema queda preparado para la
Fase 10 — pero **no es alcanzable** desde esta fase por ningún camino real:

- `services::treatment_episodes::set_episode_status` rechaza explícitamente `'cerrado'` con
  `TreatmentEpisodeError::ClosureNotImplemented` (test:
  `setting_status_to_cerrado_is_rejected_in_this_phase`).
- La UI (`TreatmentEpisodeDetailScreen`) nunca renderiza una acción "Cerrar" — solo
  "Pausar"/"Reactivar", verificado también manualmente (captura de pantalla del detalle de
  proceso sin ningún botón de cierre).
- El único lugar donde una fila llega a `status = 'cerrado'` en esta fase es la migración legacy
  (ver más abajo), para pacientes cuyo `patients.status` ya era `'alta'` antes de esta fase.

Esto corresponde a la Opción A evaluada en la aprobación de la fase: preparar el modelo sin
exponer un flujo de cierre incompleto. El cierre estructurado real (motivo, resumen, objetivos
logrados) es explícitamente Fase 10, todavía sin construir. "Reabrir por error" tampoco se
implementa en esta fase — depende de `episode_closure`, que no existe todavía; la arquitectura
queda compatible (nada impide agregarlo después) pero no hay flujo hoy.

## Regla de negocio no negociable: un solo proceso activo por paciente

Confirmado como preferencia explícita de producto durante la aprobación: la aplicación modela un
proceso clínico principal por paciente a la vez, no tratamientos paralelos simultáneos del mismo
profesional. Se protege en **dos capas independientes**, mismo criterio que el versionado de notas
de sesión (`idx_session_notes_current`, Fase 4):

1. **Servicio** (`create_episode`, `set_episode_status` al reactivar): busca un episodio `activo`
   existente del paciente antes de crear/reactivar uno nuevo, devuelve
   `TreatmentEpisodeError::AnotherEpisodeActive` si ya existe.
2. **Base de datos**: `idx_treatment_episodes_one_active_per_patient`, índice único parcial —
   verificado directamente con SQL crudo en
   `a_second_active_episode_for_the_same_patient_is_rejected_at_database_level`, para que la regla
   no dependa únicamente de que el servicio la respete.

`pausado` no es un cierre: `activo → pausado` y `pausado → activo` son transiciones válidas del
servicio, no generan ningún cierre y no tocan `patients.status`. Reactivar un proceso pausado
vuelve a exigir que no exista otro proceso activo en ese momento
(`reactivating_is_rejected_if_another_episode_became_active_meanwhile`).

## Regla de integridad: sesión/objetivo ↔ proceso ↔ paciente

Mismo criterio no negociable ya establecido en Fases 5/7/8: **nunca se confía en el `episodeId`
recibido desde React sin volver a verificarlo contra la fila real**. Nueva función reutilizable,
`services::treatment_episodes::check_episode_assignable(conn, episode_id, patient_id)`:

- `episode_id = None` → siempre válido (una sesión/objetivo sin proceso sigue siendo legítima).
- El proceso debe existir (si no, `NotFound`).
- Debe pertenecer al mismo paciente (si no, `EpisodePatientMismatch`).
- No debe estar archivado (si lo está, `EpisodeArchived`).
- No debe estar `'cerrado'` (si lo está, `EpisodeNotAssignable`).

Reutilizada, sin duplicar lógica, por:

- `services::sessions::create_session` — vía `impl From<TreatmentEpisodeError> for SessionError`.
- `services::goals::create_goal` — vía `impl From<TreatmentEpisodeError> for GoalError`.

`episode_id` se fija **solo al crear** la sesión/objetivo — igual convención que `appointment_id`
en sesiones y que `patient_id` en ambas entidades: reasignar el proceso de una sesión/objetivo ya
creado queda fuera de alcance de este MVP (no hay caso de uso que lo requiera todavía).

### `session_goals`: coherencia de proceso al vincular sesión y objetivo

`services::goals::link_session_goal` agrega una verificación nueva, `GoalError::LinkEpisodeMismatch`:
si **ambos** — la sesión y el objetivo — tienen `episode_id`, deben coincidir. Si **cualquiera de
los dos** no tiene proceso asignado, el vínculo se permite exactamente como antes de esta fase —
sin romper el comportamiento de la Fase 5
(`linking_still_works_when_only_one_side_has_an_episode`).

## `episode_clinical_profile`: antecedentes específicos del proceso

Mismo patrón exacto que `patient_clinical_profile` (Fase 6): mutable, sin versionado, 1:1 vía
`episode_id` como `PRIMARY KEY` propia, `create`/`update` como operaciones separadas (nunca un
upsert), reflejando directamente los dos estados distintos de la UI ("Agregar antecedentes" vs.
"Editar").

Contiene exactamente los tres campos que la fase determinó como específicos de un proceso:
`presenting_problem` (motivo de consulta), `primary_diagnosis_code`, `diagnosis_notes`.
**`risk_flags` y `relevant_medical_notes` permanecen exclusivamente en `patient_clinical_profile`**
— longitudinales, del paciente, no del proceso — sin ningún cambio de esquema ni de datos sobre
esa tabla, que queda completamente intacta (verificado explícitamente por el mismo test de
preservación de la migración `V4`).

### Por qué `risk_flags` no se separó en esta fase

La aprobación de Fase 9 fue explícita: no diseñar una taxonomía de riesgo clínico, no crear un
"motor de riesgo", no inferir niveles de riesgo, no automatizar decisiones clínicas. Sin una
taxonomía que distinga riesgo histórico (longitudinal) de riesgo actual/específico de un proceso,
mover `risk_flags` fuera de `patient_clinical_profile` sería inventar una distinción clínica que
esta fase no está autorizada a diseñar. Se documenta aquí, como pidió la aprobación, que una
evolución futura podría separar el historial de riesgo longitudinal del riesgo actual de un
proceso — **no implementado, solo documentado**.

## Migración legacy: un proceso automático por paciente con actividad clínica real

Política aprobada explícitamente: crear automáticamente **un** proceso legacy por paciente con
datos clínicos relevantes ya existentes, sin intervención de la usuaria, no destructivo, idempotente
(la migración corre una única vez por vault — `rusqlite_migration` lo garantiza — y
`v4_migration_is_idempotent` confirma que reaplicarla sobre un vault ya migrado no duplica nada).

**Criterio de inclusión** (evaluado explícitamente antes de escribir el SQL, con el criterio
"preservación > perfección histórica" de la aprobación): un paciente recibe un proceso legacy si
tiene **al menos una** de estas tres señales de actividad clínica real —

- al menos una sesión (`sessions`),
- al menos un objetivo terapéutico (`therapeutic_goals`),
- una fila en `patient_clinical_profile`.

Pacientes cuya única actividad son `patient_prep_notes`/`therapy_tasks`/`payments` (tablas que no
reciben `episode_id` en esta fase) **no** reciben un proceso legacy — crear uno solo para esos
casos sería un "proceso basura" sin sesiones ni objetivos ni antecedentes que asociarle, exactamente
lo que la aprobación pidió evitar.

**Sin heurísticas de fecha.** Nunca se intenta reconstruir múltiples procesos históricos a partir
de huecos temporales ("si pasaron X meses, es otro proceso") — eso inventaría estructura clínica
que no está en los datos. Como máximo **un** proceso legacy automático por paciente en esta
migración.

**Estado del proceso legacy**: derivado únicamente de la señal ya existente `patients.status` —
`'alta'` → `'cerrado'`, cualquier otro valor → `'activo'`. Ninguna heurística de fecha ni de
volumen de datos interviene en esta decisión.

**`started_at` del proceso legacy**, en orden de prioridad:

```sql
COALESCE(
  (SELECT MIN(session_date) FROM sessions WHERE patient_id = p.id),
  patients.intake_date,
  substr(patients.created_at, 1, 10)
)
```

Verificado por `v4_legacy_episode_started_at_falls_back_to_intake_date_then_created_at`: la primera
sesión real si existe, si no la fecha de ingreso histórica, y si tampoco existe, la fecha de
creación del registro del paciente — nunca queda `NULL`.

**ID determinístico**: `'legacy-' || patient_id`, no un UUID aleatorio. Decisión deliberada para
auditabilidad — el origen de la fila es visible a simple vista en cualquier consulta o backup, y
garantiza como máximo un proceso legacy por paciente sin necesitar ninguna función UUID en SQL.

**Asociaciones**: todas las sesiones y objetivos existentes de un paciente con proceso legacy se
reasignan a `'legacy-' || patient_id` (nunca se intenta repartirlos entre "varios procesos
históricos" inventados). Si existía una fila en `patient_clinical_profile`, su contenido
(`presenting_problem`, `primary_diagnosis_code`, `diagnosis_notes`) se **copia** —
nunca se mueve destructivamente — a una fila de `episode_clinical_profile` del proceso legacy.
`patient_clinical_profile` no se toca ni se borra en ningún momento de la migración: la copia se
inserta antes de que exista cualquier necesidad de eliminar el original, y de hecho el original
**nunca se elimina** — la fase completa preserva ambas copias, la longitudinal (paciente) y la
específica de proceso (episodio legacy).

## `patients.intake_date` y `patients.status`: sin cambios de semántica

`patients.intake_date` **no se sobrescribe** al crear un proceso nuevo — sigue siendo dato
histórico/legacy del paciente. `treatment_episode.started_at` es la fecha de inicio del proceso;
futuros reingresos usarán `treatment_episode.started_at` del nuevo proceso, nunca
`patients.intake_date`.

`patients.status` **no pierde ningún valor de su `CHECK`** — `'alta'` sigue siendo válido, por
compatibilidad. Queda documentado explícitamente que `patients.status` **no será** la futura
fuente de verdad de "proceso terapéutico cerrado" — ese rol pasa a `treatment_episode.status` +
la futura `episode_closure` (Fase 10). No se implementó ninguna migración destructiva de
deprecación de `'alta'` en esta fase.

La inconsistencia preexistente, ya documentada en la auditoría previa, entre
`patients.status = 'archivado'` y `patients.deleted_at`, **no se corrigió** en esta fase — no
interfirió con la implementación de procesos, así que se deja documentada, no resuelta.

## Reingreso: nunca se duplica al paciente

Política explícita: un reingreso reutiliza el mismo registro de paciente y sus antecedentes
longitudinales (`patient_clinical_profile`). El proceso anterior queda histórico, con sus propias
sesiones/objetivos/antecedentes de proceso intactos. El nuevo proceso empieza limpio en cuanto a
motivo de consulta/diagnóstico/objetivos/sesiones — **nada se copia automáticamente** desde el
proceso anterior. Si un objetivo terapéutico de un proceso previo vuelve a ser relevante, se crea
un objetivo **nuevo** en el proceso nuevo — no existe (ni se necesita en esta fase) una función de
"copiar objetivo" entre procesos.

## Arquitectura

Mismas capas que el resto de las verticales:

```
React (features/treatment-episodes/*, selector de proceso en Sesiones/Objetivos)
   │  invoke('create_treatment_episode', ...), invoke('get_episode_clinical_profile', ...), etc.
   ▼
commands::treatment_episodes / commands::episode_clinical_profile   (10 comandos en total)
   │  — capa fina, mediada por VaultSession::with_connection.
   ▼
services::treatment_episodes / services::episode_clinical_profile
   │  — un proceso activo por paciente, check_episode_assignable, cierre inalcanzable.
   ▼
repositories::treatment_episodes / repositories::episode_clinical_profile
   │  — SQL puro.
   ▼
SQLite + SQLCipher (vault.db, migración V4)
```

## UI: ficha del paciente, sin mega-interfaz

Pestaña **"Procesos"** nueva en `PatientDetailScreen` (segunda posición, justo después de
"Resumen"): sección **"Proceso actual"** (fecha de inicio, estado, botón "Ver proceso"; si no hay
proceso activo y el paciente no está archivado, botón "Iniciar proceso" con fecha opcional —
por defecto hoy) y sección **"Procesos anteriores"**, solo si existen. Sin gráficos, sin mezclar
con el Dashboard global.

`TreatmentEpisodeDetailScreen` (`/patients/:patientId/episodes/:episodeId`): estado del proceso con
acción Pausar/Reactivar (nunca Cerrar), y el bloque "Antecedentes del proceso"
(`episode_clinical_profile`) con su propio flujo crear/editar — visualmente idéntico al patrón ya
usado por Antecedentes del paciente (Fase 6), pero una entidad completamente separada.

`SessionCreateScreen` y `GoalCreateScreen` incorporan un selector "Proceso terapéutico (opcional)":
se listan los procesos del paciente que no estén `'cerrado'`, con el proceso activo
preseleccionado si existe, y siempre la opción "— Sin proceso —". La UI restringe la lista por
conveniencia; la autoridad real está en `check_episode_assignable`, en el backend.

## Privacidad

- **Sin logging de contenido clínico.** Ninguno de los archivos nuevos de backend
  (`repositories`/`services`/`commands` × 2 entidades) contiene una sola llamada a
  `log::`/`println!`/`dbg!`/`eprintln!`.
- **Google Calendar intacto.** `src-tauri/src/calendar/*` no se modificó; verificado por `grep`
  que no contiene ninguna referencia a `episode`/`diagnosis`/`presenting_problem`.
- **Sin envío externo.** Ninguna de las entidades nuevas se comunica con ningún servicio externo.
- **Auditoría manual con marcador ficticio (`XYZFASE9EPISODIOSMARKER`)**, sembrado en las notas
  diagnósticas de un proceso de prueba real: cero coincidencias en el log de la aplicación, en
  clipboard, ni en ningún directorio de datos/caché/config de la aplicación (incluidos los vaults
  de fases anteriores conservados como respaldo) fuera del propio `vault.db`, cifrado con
  SQLCipher. Título de ventana confirmado genérico ("Cuaderno Clínico"), sin datos de paciente ni
  de proceso.

## Decisiones de negocio tomadas en esta fase

1. **Modelo mínimo, sin campos de cierre.** `reason_for_end`/`closure_summary`/etc. quedan
   explícitamente para Fase 10; no se creó `episode_closures`.
2. **Un proceso activo por paciente, en dos capas** (servicio + índice único parcial).
3. **`'cerrado'` legal en el esquema, inalcanzable en el servicio y en la UI (Opción A).**
4. **`episode_clinical_profile` nuevo, con exactamente 3 campos** (`presenting_problem`,
   `primary_diagnosis_code`, `diagnosis_notes`); `risk_flags`/`relevant_medical_notes` permanecen
   longitudinales en `patient_clinical_profile`, sin taxonomía de riesgo nueva.
5. **Migración legacy: máximo un proceso automático por paciente**, basado únicamente en
   `patients.status`, sin heurísticas de fecha, con criterio de inclusión explícito (sesión,
   objetivo o antecedentes) para evitar procesos vacíos.
6. **`appointments` no recibe `episode_id`.** La agenda puede preceder a cualquier proceso formal.
7. **`patients.intake_date` y `patients.status` sin cambios de semántica** — ningún valor del
   `CHECK` se elimina, ninguna migración destructiva de `'alta'`.
8. **Reingreso nunca duplica al paciente**, ni copia automáticamente contenido específico del
   proceso anterior al nuevo.
9. **Sin `episode_id` en `payments`/`patient_prep_notes`/`therapy_tasks`/`documents`/
   `assessment_administrations`/`case_formulations`/`reminders`** — ninguna necesidad concreta lo
   exigió durante la implementación real.

## Exclusiones explícitas de esta fase

Cierre/alta estructurado (Fase 10), `episode_closures`, reapertura de un proceso cerrado por
error, taxonomía de riesgo clínico, motor de riesgo o inferencia de niveles de riesgo, copia
automática de objetivos entre procesos, clasificación manual de un proceso legacy en varios
procesos reales, deprecación física de `patients.status = 'alta'`, corrección de la inconsistencia
preexistente `patients.status = 'archivado'` vs. `deleted_at`, gráficos o estadísticas de procesos,
cualquier cambio a Google Calendar/OAuth. Ningún lint warning preexistente se corrigió como parte
de esta fase (regla de no-refactorización oportunista).

## Archivos creados o modificados

| Archivo | Rol |
|---|---|
| `src-tauri/src/db/migrations.rs` | `SCHEMA_V4` (dos tablas nuevas, dos columnas nuevas, backfill legacy), registro en `migrations()`, 6 tests nuevos de migración. |
| `src-tauri/src/repositories/treatment_episodes.rs` (nuevo) | SQL puro sobre `treatment_episodes`. |
| `src-tauri/src/services/treatment_episodes.rs` (nuevo) | Un proceso activo por paciente, transiciones válidas, `check_episode_assignable`. |
| `src-tauri/src/commands/treatment_episodes.rs` (nuevo) | 7 comandos Tauri. |
| `src-tauri/src/repositories/episode_clinical_profile.rs` (nuevo) | SQL puro, mismo patrón que `patient_clinical_profile`. |
| `src-tauri/src/services/episode_clinical_profile.rs` (nuevo) | Validación, create/update separados. |
| `src-tauri/src/commands/episode_clinical_profile.rs` (nuevo) | 3 comandos Tauri. |
| `src-tauri/src/repositories/sessions.rs`, `services/sessions.rs` | `episode_id` opcional en `Session`/`NewSessionRow`/`SessionInput`; integración de `check_episode_assignable`. |
| `src-tauri/src/repositories/goals.rs`, `services/goals.rs` | `episode_id` opcional en `Goal`/`NewGoalRow`/`GoalInput`; integración de `check_episode_assignable`; `LinkEpisodeMismatch` en `link_session_goal`. |
| `src-tauri/src/repositories/mod.rs`, `services/mod.rs`, `commands/mod.rs`, `lib.rs` | Registro de módulos y de los 10 comandos nuevos. |
| Varios `#[cfg(test)]` en archivos no prohibidos (`session_notes.rs`, `payments.rs`, `therapy_tasks.rs`, `session_goals.rs`, `patient_prep_notes.rs`, `goal_indicators.rs`) | Ajuste mecánico de literales de struct tras agregar `episode_id` — sin cambio de comportamiento. |
| `src/features/treatment-episodes/*` (nuevo) | `types.ts`, `api.ts`, `ProcessesTab.tsx`, `TreatmentEpisodeDetailScreen.tsx`. |
| `src/features/goals/types.ts`, `schema.ts`, `GoalCreateScreen.tsx` | `episodeId` opcional + selector de proceso. |
| `src/features/sessions/types.ts`, `schema.ts`, `SessionCreateScreen.tsx` | `episodeId` opcional + selector de proceso. |
| `src/features/patients/PatientDetailScreen.tsx` | Pestaña "Procesos" nueva. |
| `src/App.tsx` | Ruta `/patients/:patientId/episodes/:episodeId`. |

## Tests ejecutados

`cargo test` en `src-tauri/`: **491/491 en verde** (423 previos sin cambios + 68 nuevos: ~13 en
`repositories::treatment_episodes`, ~20 en `services::treatment_episodes`, ~6 en
`repositories::episode_clinical_profile`, ~8 en `services::episode_clinical_profile`, 6 en
`db::migrations` para `V4`, 7 en `services::sessions`, 8 en `services::goals`). `cargo clippy
--all-targets`: sin advertencias. `cargo build`: sin errores. `npm run build`: sin errores. `npm run
lint`: 20 warnings (19 preexistentes + 1 nuevo en `TreatmentEpisodeDetailScreen.tsx`, misma
categoría `react(set-state-in-effect)` ya presente en prácticamente todas las pantallas con
`useEffect` de carga de datos desde fases anteriores — no es una regresión ni una categoría nueva,
y no se corrigió por la regla de no-refactorización oportunista).

Tests representativos de las reglas de negocio centrales:

| Requisito | Test |
|---|---|
| `V4` preserva datos de `V1`+`V2`+`V3` y no toca `patient_clinical_profile` | `db::migrations::v4_migration_preserves_all_existing_data_and_does_not_touch_patient_clinical_profile` |
| Un solo proceso activo por paciente, a nivel de base de datos | `db::migrations::a_second_active_episode_for_the_same_patient_is_rejected_at_database_level` |
| Un solo proceso activo por paciente, a nivel de servicio | `services::treatment_episodes::rejects_a_second_active_episode_for_the_same_patient` |
| Reactivar rechazado si otro proceso se activó mientras tanto | `services::treatment_episodes::reactivating_is_rejected_if_another_episode_became_active_meanwhile` |
| `'cerrado'` inalcanzable desde el servicio | `services::treatment_episodes::setting_status_to_cerrado_is_rejected_in_this_phase` |
| Sesión/objetivo con proceso de otro paciente rechazado | `services::treatment_episodes::check_episode_assignable_rejects_an_episode_of_a_different_patient` |
| Proceso archivado no admite asignaciones nuevas | `services::treatment_episodes::check_episode_assignable_rejects_an_archived_episode` |
| Proceso cerrado no admite asignaciones nuevas | `services::treatment_episodes::check_episode_assignable_rejects_a_closed_episode` |
| `episode_id` ausente sigue siendo válido | `services::treatment_episodes::check_episode_assignable_accepts_none` |
| Vínculo sesión↔objetivo con procesos distintos rechazado | `services::goals::linking_a_session_and_goal_of_different_episodes_is_rejected` |
| Vínculo sesión↔objetivo con solo un lado con proceso sigue funcionando (compatibilidad Fase 5) | `services::goals::linking_still_works_when_only_one_side_has_an_episode` |
| `started_at` legacy: sesión → `intake_date` → `created_at` | `db::migrations::v4_legacy_episode_started_at_falls_back_to_intake_date_then_created_at` |
| Procesos legacy visibles a través del listado normal del servicio | `services::treatment_episodes::legacy_episodes_are_visible_through_normal_listing` |
| Migración `V4` es idempotente | `db::migrations::v4_migration_is_idempotent` |

## Prueba manual realizada (aplicación real, no solo tests)

Compilada con `cargo build`, ejecutada bajo Xvfb con `xdotool` (clics y tecleo reales) sobre un
vault de prueba desechable (el vault real se guardó aparte antes de empezar y se restauró
exactamente al terminar), con capturas de pantalla en cada paso. Casos cubiertos (paciente
ficticio "Andrea Molina Rivas"):

- **Caso A** — nuevo paciente → nuevo proceso → sesión vinculada → objetivo vinculado: confirmado
  completo, incluyendo el selector de proceso preseleccionando el proceso activo tanto en
  Sesiones como en Objetivos.
- **Caso B** — sesión sin proceso (opción "— Sin proceso —" elegida explícitamente): válida.
- **Caso C** — asignar proceso de un paciente a la sesión/objetivo de otro: rechazado — cubierto
  extensamente por tests automatizados de servicio (la UI ni siquiera ofrece procesos ajenos en el
  selector, por lo que no hay una ruta de GUI directa para intentarlo).
- **Caso D** — paciente con proceso legacy: datos anteriores visibles — cubierto extensamente por
  6 tests dedicados de migración (crear un vault genuinamente pre-`V4` para una verificación de
  GUI adicional habría sido redundante frente a esa cobertura).
- **Caso E** — proceso activo existente → botón "Iniciar proceso" ausente: confirmado.
- **Caso F** — activo → pausado → activo: confirmado desde el detalle del proceso.
- **Caso G** — archivar paciente → Procesos sigue mostrando el proceso existente, editable, sin
  botón de creación nueva → restaurar paciente → creación disponible de nuevo.
- **Caso H** — bloquear → desbloquear (sin matar el proceso): datos intactos.
- **Caso I** — cierre completo del proceso de la aplicación (`kill -9`) → relanzamiento → arranca
  bloqueado → desbloquear → paciente, proceso activo, antecedentes del proceso y ambas sesiones
  (una con proceso, una sin proceso) confirmados intactos.

**Auditoría de privacidad**: marcador ficticio `XYZFASE9EPISODIOSMARKER` sembrado en las notas
diagnósticas de un proceso real de prueba — cero coincidencias fuera de `vault.db` en el log de la
aplicación, clipboard, y todos los directorios de datos/caché/config de la aplicación (incluidos
los vaults de respaldo de fases anteriores). Título de ventana confirmado genérico.

**Regresión funcional**: Pacientes (listado, ficha), Antecedentes del paciente (Fase 6, formulario
intacto y completamente separado de "Antecedentes del proceso"), Sesiones (listado, continuidad de
Fase 8), Objetivos, Pagos (Fase 7, sin `episode_id`), Agenda, Estadísticas (sin datos de proceso),
Dashboard — todos revisados en el mismo flujo sin cambios de comportamiento respecto a fases
anteriores.

## Limitaciones y decisiones que quedan pendientes de aprobación

Ninguna. Todas las decisiones de esta fase estaban resueltas de forma definitiva en la aprobación
formal de Fase 9. El cierre estructurado de un proceso, la reapertura por error, la separación de
riesgo longitudinal vs. específico de proceso y la clasificación manual de un proceso legacy en
varios procesos reales quedan documentadas explícitamente como **no implementadas** — cualquier
fase futura que las aborde deberá presentarse como su propio cambio, con su propio análisis de
impacto, no como una extensión silenciosa de esta fase.
