# Formulación clínica textual versionada (Fase 15)

Documento técnico de la Fase 15. Complementa `docs/ARCHITECTURE.md` (fila de la tabla de fases) y
`Plan-Fase-15-pendiente-de-aprobacion.md` (auditoría de esquema y decisión patient vs. episode que
precedió esta fase).

## 1. Qué es una Formulación (principio rector, no negociable)

> La Formulación es: una hipótesis clínica de trabajo escrita por la profesional que puede cambiar
> con nueva información. Por eso: debe versionarse, no sobrescribirse, no generarse
> automáticamente, no confundirse con diagnóstico, y no copiarse silenciosamente entre procesos.

Esta fase es **estrictamente textual**. Quedan explícitamente fuera de alcance: canvas visual,
nodos, aristas, mapas conceptuales, React Flow, generación automática, sugerencias diagnósticas o
cualquier forma de inferencia automática. Ninguna parte del contenido de una formulación se genera
ni se sugiere — todo lo escribe la profesional.

## 2. Auditoría de esquema previa y decisión patient vs. episode

Antes de escribir código se auditó `case_formulations`/`formulation_versions`/
`formulation_nodes`/`formulation_edges` (presentes sin usar desde `SCHEMA_V1`, Fase 1.3). Hallazgo
clave: `case_formulations` solo tenía `patient_id NOT NULL`, sin ningún vínculo al proceso
terapéutico (`treatment_episodes`) — decisión explícita de la Fase 9 (ver `docs/treatment-
episodes.md`) de no agregarlo entonces "porque ninguna necesidad concreta lo exigió durante la
implementación real".

Esta fase revisó esa decisión porque sí existe ahora una necesidad concreta: el motivo de consulta
y las hipótesis clínicas cambian entre reingresos, y una formulación de un proceso anterior nunca
debe aparecer como si estuviera vigente para un proceso nuevo. Se presentaron tres opciones en
`Plan-Fase-15-pendiente-de-aprobacion.md` (A: mantener solo `patient_id`; B: agregar `episode_id`
opcional; C: alternativas de inferencia temporal o codificación dentro de `summary_text`, ambas
rechazadas por frágiles). La usuaria aprobó explícitamente la **Opción B**.

`SCHEMA_V8` (puramente aditiva):

```sql
ALTER TABLE case_formulations ADD COLUMN episode_id TEXT
  REFERENCES treatment_episodes(id) ON DELETE SET NULL;
CREATE INDEX idx_case_formulations_episode ON case_formulations(episode_id);
CREATE UNIQUE INDEX idx_case_formulations_one_per_episode
  ON case_formulations(episode_id) WHERE episode_id IS NOT NULL AND deleted_at IS NULL;
```

Mismo patrón ya usado tres veces en el proyecto para reglas "una X por Y": el índice único parcial
(`idx_treatment_episodes_one_active_per_patient` en V4, `idx_episode_closures_active` en V5,
`idx_safety_plans_one_current_per_patient` en V6). La columna es nulable a nivel de esquema (no
rompe compatibilidad con filas hipotéticas anteriores a esta fase — aunque, al auditar, se confirmó
que no existía ningún repositorio/servicio/comando/componente de formulación en todo el código: este
era el momento más seguro posible para migrar, con cero filas reales en cualquier entorno).

Verificado con `fresh_database_has_v8_columns`, `v8_migration_is_idempotent_and_preserves_v1_
formulation_data`, `case_formulation_can_link_to_a_treatment_episode_of_the_same_patient` y
`a_second_formulation_for_the_same_episode_is_rejected_at_database_level`.

### `episode_id` obligatorio en el servicio, aunque nulable en el esquema

La columna es nulable en SQL (necesario para `ON DELETE SET NULL` si el proceso se elimina —
nunca ocurre desde un comando normal, pero el esquema lo permite), pero `services::formulations::
FormulationInput.episode_id` es un `String` obligatorio, no `Option<String>`. Permitir una
formulación "suelta" del paciente sin proceso recrearía exactamente la ambigüedad que esta fase
resolvió: ¿a qué proceso pertenece una hipótesis clínica sin proceso? Toda formulación nueva se crea
siempre asociada a un proceso terapéutico concreto.

## 3. Una formulación principal por proceso, versionada

Cada proceso terapéutico (`treatment_episode`) tiene como máximo una formulación principal
(`case_formulations`, forzado por el índice único parcial de la sección 2), con múltiples versiones
históricas (`formulation_versions`, v1 → v2 → v3…) — nunca se sobrescribe ni se elimina una versión
desde la UI normal.

`formulation_versions` no tiene columna de estado ni "borrador": cada versión ya es una versión
confirmada en el momento en que se guarda. La versión vigente es siempre la de
`MAX(version_number)` para esa formulación (`repositories::formulations::latest_version`). Se
evaluó deliberadamente no replicar el modelo más complejo de `safety_plans` (borrador/vigente/
reemplazado): no hay ningún requisito de esta fase que exija un estado intermedio no confirmado, y
agregar uno habría sido una migración/columna innecesaria.

"Actualizar formulación" (`create_new_version`) precarga el contenido de la versión actual en el
formulario (`UpdateFormulationModal` en el frontend) y, al guardar, crea `version_number + 1` en una
transacción — la versión anterior nunca se toca ni se borra.

## 4. Contenido estructurado sin nuevas columnas SQL

El encargo pidió explícitamente evitar dos extremos: un único `textarea` gigante, y treinta columnas
SQL rígidas. La solución: `formulation_versions.summary_text` sigue siendo una única columna `TEXT`
(sin migración), pero el **frontend** la estructura en 10 secciones editables (síntesis,
predisponentes, precipitantes, perpetuantes, protectores, patrones, hipótesis, focos terapéuticos,
plan de intervención, observaciones — `FORMULATION_SECTIONS` en `src/features/formulation/
types.ts`), serializadas como bloques Markdown simples (`## Sección\n\ncontenido`) unidos por líneas
en blanco (`src/features/formulation/sections.ts`: `serializeSections`/`parseSections`). El backend
nunca interpreta esta estructura — para `services::formulations` y `repositories::formulations`,
`summary_text` es texto libre opaco, igual que para cualquier otro TEXT del esquema.

Una sección vacía se omite por completo al serializar (no llena el texto de encabezados sin
contenido). Al volver a abrir una versión, si el texto no calza con el formato esperado (por
ejemplo, contenido legado o escrito fuera de la app), `parseSections` nunca descarta nada: todo el
texto no reconocido se coloca en la sección "Síntesis del caso".

`model_type` es texto libre (`FORMULATION_MODEL_SUGGESTIONS` solo alimenta un `<datalist>` de
sugerencias — TCC, 5P, Transdiagnóstico — nunca una lista cerrada; cualquier texto propio es válido,
sin necesidad de una opción "Otro" porque no hay `<select>` restrictivo).

## 5. Historial desde el primer momento

Explícito en el encargo: "no repetir la deuda de Historial de cierres" (Fase 11/14, donde el
historial se agregó en una fase posterior). `FormulationHistory.tsx` existe desde esta misma fase:
lista todas las versiones (`list_formulation_versions`, orden descendente por `version_number`) y
permite abrir cualquiera en modo exclusivamente de lectura. Ningún botón de edición ni de
eliminación aparece nunca en el historial.

## 6. Procesos cerrados, reingreso y paciente archivado

- **Proceso cerrado**: `create_new_version` revalida el proceso vinculado con
  `treatment_episodes::check_episode_assignable` (mismo mecanismo reutilizado de
  `sessions`/`therapeutic_goals`/`assessment_administrations`) usando el `episode_id` ya guardado en
  la formulación. Un proceso cerrado rechaza la validación → no se puede crear una versión nueva. La
  formulación y su historial siguen siendo de solo lectura normalmente (no hay ocultamiento, solo
  ausencia del botón "Actualizar formulación" cuando el backend lo rechazaría).
- **Reingreso**: un proceso nuevo (`treatment_episode` nuevo) nunca hereda ni copia
  automáticamente la formulación de un proceso anterior del mismo paciente. Verificado con el test
  `reingreso_never_copies_the_previous_episodes_formulation`: crear un segundo proceso y consultar
  `get_formulation_by_episode` para él devuelve `None` aunque el primer proceso ya tenga una
  formulación con contenido.
- **Paciente archivado**: `create_formulation` y `create_new_version` rechazan explícitamente un
  paciente archivado (`FormulationError::PatientArchived`) — mismo criterio que
  `require_editable_draft` de `safety_plans` (Fase 12): crear una versión nueva es "crear contenido
  clínico nuevo", igual que la primera creación, y ambas quedan bloqueadas. La lectura (formulación
  actual e historial completo) permanece siempre disponible para un paciente archivado. Verificado
  con `rejects_a_new_version_for_an_archived_patient` y
  `allows_a_new_version_again_after_the_patient_is_restored`.

## 7. Objetivos terapéuticos (`therapeutic_goals.formulation_id`) — decisión de no integrar

Se auditó `docs/goals.md`: `therapeutic_goals.formulation_id` es nulable, `ON DELETE SET NULL`, y
está mapeado en el struct `Goal`, pero **ningún comando actual lo escribe ni lo expone**. Construir
en esta fase un visor de "objetivos relacionados con esta formulación" habría mostrado siempre una
lista vacía en la práctica, sin ningún valor real, y habría requerido tocar `services::goals`/
`repositories::goals` — expresamente fuera de alcance salvo detención explícita. Se decidió **no
integrar Goals en absoluto** en esta fase. Si una fase futura decide poblar `formulation_id` desde
algún comando de Goals, es una decisión de esa fase, no de esta.

## 8. `formulation_nodes`/`formulation_edges` — reservado para Formulación Visual futura

Ambas tablas existen desde `SCHEMA_V1`, completamente construidas (incluye triggers de integridad
cruzada entre versión y nodo/arista), pero **no se usan, no se leen, no se escriben ni se modifican
en esta fase**. Quedan reservadas explícitamente para una futura fase de "Formulación Visual" (mapa
conceptual/canvas), fuera del alcance textual de la Fase 15. Ningún archivo de este vertical
(`repositories::formulations`, `services::formulations`, `commands::formulations`,
`src/features/formulation/*`) las referencia.

## 9. Archivado de formulaciones — decisión de no implementar

`case_formulations.deleted_at` existe desde `SCHEMA_V1` pero ningún comando de esta fase lo
escribe ni lo lee (todas las consultas filtran `deleted_at IS NULL`, pero no existe ningún
`archive_formulation`/`restore_formulation`). El encargo de esta fase no pidió explícitamente un
botón de archivar formulación — solo versionado, historial y las reglas de proceso cerrado/
reingreso/paciente archivado ya cubiertas arriba. Mismo criterio que `raw_responses` en
`assessment_administrations` (Fase 13, ver `docs/assessments.md` sección 2): una columna presente en
el esquema, deliberadamente sin usar, documentada aquí en vez de una reactivación silenciosa futura.

## 10. Minimización de IPC

`FormulationSummary` (usado por `list_formulations`) incluye metadatos (título, `model_type`,
`episode_id`, número de versión actual, fecha de actualización) pero **nunca** `summary_text` — el
contenido completo de una versión solo viaja al pedir explícitamente `get_current_formulation_
version`/`get_formulation_version`/`list_formulation_versions`. Mismo criterio de minimización ya
usado por `SafetyPlanSummary` (Fase 12) y `AssessmentAdministrationSummary` (Fase 13).

## 11. Frontend

`src/features/formulation/`:

- `types.ts`: interfaces TS (`CaseFormulation`, `FormulationVersion`, `FormulationSummary`,
  `FormulationInput`, `NewVersionInput`), `FORMULATION_SECTIONS` y `FORMULATION_MODEL_SUGGESTIONS`.
- `sections.ts`: `serializeSections`/`parseSections` (sección 4).
- `api.ts`: wrapper de los 8 comandos Tauri.
- `schema.ts`: validación `zod` del formulario (título obligatorio; secciones sin validación de
  esquema, son texto libre).
- `FormulationHistory.tsx`: historial de solo lectura (sección 5).
- `FormulationTab.tsx`: pestaña "Formulación" de la ficha del paciente. Distingue **"Formulación
  del proceso actual"** (el proceso con `status === 'activo'` — mismo criterio ya usado por
  `ProcessesTab.tsx`, deliberadamente no redefinido para mantener consistencia con el único
  precedente existente en el código, aceptando su particularidad conocida de que un proceso
  `pausado` cae en "otros procesos" en vez de en "el actual") de **"Formulaciones de otros
  procesos"** — nunca duplica el listado de procesos en sí, que sigue viviendo únicamente en la
  pestaña "Procesos". La editabilidad real siempre depende del backend
  (`check_episode_assignable`): una formulación de un proceso pausado sigue ofreciendo "Actualizar
  formulación" si el backend lo permite, aunque esté agrupada visualmente como "otro proceso".

No se usa ningún editor de texto enriquecido (no se agregó TipTap, Quill ni ninguna librería
nueva) — el contenido se edita con `<textarea>` simples por sección, mismo componente `Textarea` ya
usado en el resto de la aplicación. **Cero dependencias nuevas** en esta fase, backend y frontend.

## 12. Google Calendar

Ninguna formulación se sincroniza jamás con Google Calendar. Verificado por grep: cero referencias a
`formulation`/`summary_text`/`hypothesis`/`predisponente`/`precipitante`/`perpetuante`/`protector`
dentro de `src-tauri/src/calendar/` ni de `src/features/agenda/`, y cero importaciones de
`calendar::*` desde ningún archivo de este vertical.

## 13. Backup / Restore

Sin cambios de diseño: `SCHEMA_V8` es aditiva, así que un respaldo creado después de esta fase
incluye `episode_id` como cualquier otra columna del esquema. Único cambio en `backup/service.rs`:
actualización mecánica de los literales de test `schema_version`/`supported_schema_version` de `7` a
`8` (mismo patrón ya aplicado en Fase 13).

## 14. Privacidad

El contenido de una formulación (hipótesis, patrones, factores de vulnerabilidad) nunca debe
aparecer en logs, Google Calendar, `localStorage`, `sessionStorage`, la URL, el título del
documento, el portapapeles ni ningún mecanismo de analytics/telemetría (inexistentes en el
proyecto). Verificado por grep en `src/features/formulation/`: cero referencias a `console.*`,
`localStorage`, `sessionStorage`, `navigator.clipboard`, `window.location`, `document.title`,
`analytics` o `telemetry`. Marcador ficticio usado para la auditoría manual pendiente:
`XYZFASE15FORMULACION` (ver sección 16).

## 15. Tests

695 tests de backend (25 nuevos respecto al baseline de 670 tras Fase 14): 4 en `db::migrations`
para `SCHEMA_V8` (columnas presentes desde el arranque, idempotencia y preservación de datos
anteriores a `V8`, vínculo a un proceso terapéutico, regla de una formulación por proceso a nivel de
base de datos), 7 en `repositories::formulations`, 14 en `services::formulations` (creación,
paciente inexistente/archivado, proceso inexistente/de otro paciente/cerrado, regla de una
formulación por proceso, título vacío, versionado sin sobrescritura, paciente archivado bloquea
nueva versión y la desbloquea al restaurar, proceso cerrado bloquea nueva versión, reingreso nunca
copia, listado incluye formulaciones de procesos anteriores).

`cargo clippy --release --all-targets`: 0 warnings. `npm run build`: compila sin errores. `npm run
lint`: 24 advertencias (23 preexistentes + 1 nueva en `FormulationTab.tsx`, misma categoría
`react(set-state-in-effect)` ya presente en `GoalsTab`/`PaymentsTab`/`SessionsTab`/
`AssessmentsTab`/`SafetyPlanTab` — no es una categoría nueva), 0 errores.

## 16. Prueba manual GUI — bloqueada por el entorno, documentada como pendiente

Se intentó la prueba manual end-to-end (Casos A–I del encargo: crear formulación v1, actualizar a
v2, verificar que v1 permanece intacta en el historial, cerrar el proceso y confirmar que bloquea
una versión nueva, reingreso sin copia automática, archivar paciente, bloquear/desbloquear vault,
reiniciar la aplicación, y verificar el marcador de privacidad `XYZFASE15FORMULACION`) en un vault
desechable bajo Xvfb, con el binario `release` reconstruido tras el build de frontend.

La aplicación se lanzó correctamente (ventana "Cuaderno Clínico" creada, proceso vivo), pero
WebKitGTK no pudo cargar el contenido ("Could not connect to localhost: Connection refused" — un
error de la capa de renderizado del protocolo personalizado de Tauri en este entorno, no un error de
la aplicación). Se intentó tres veces con configuraciones distintas (lanzamiento directo, con sesión
D-Bus vía `dbus-run-session`, y forzando renderizado por software con
`WEBKIT_DISABLE_COMPOSITING_MODE`/`LIBGL_ALWAYS_SOFTWARE`), con el mismo resultado en las tres. No
se relaciona con el código de esta fase: es una limitación del entorno de este contenedor en este
momento. Siguiendo la política explícita del proyecto de nunca inventar resultados de prueba manual,
esto queda documentado como **pendiente** y se agrega al Pre-V1 Manual Acceptance Test acumulado
(junto con las pruebas pendientes de fases anteriores documentadas de la misma forma).

## 17. Limitaciones conocidas

- No hay integración con Objetivos terapéuticos (sección 7) ni con Formulación Visual (sección 8) —
  ambas explícitamente fuera de alcance de esta fase.
- No hay archivado de formulaciones individuales (sección 9) — solo las reglas de paciente
  archivado/proceso cerrado que bloquean contenido nuevo.
- No hay exportación/PDF/impresión de una formulación ni de su historial.
- La prueba manual GUI de esta fase quedó pendiente por una limitación del entorno (sección 16), no
  por falta de intento.
