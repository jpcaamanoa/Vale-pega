# Objetivos terapéuticos y vínculo con sesiones (Fase 5)

Documento técnico de la Fase 5. Complementa `docs/ARCHITECTURE.md` (secciones 4 y 13.D) y
`docs/sessions.md` (Fase 4, cuyo patrón de capas se reutiliza sin cambios). Cubre el tercer
vertical funcional completo del cuaderno clínico: registrar objetivos terapéuticos, sus
indicadores de progreso, y su vínculo con las sesiones donde se trabajaron.

## Propósito

Antes de esta fase, "Objetivos" era una pestaña de la ficha del paciente que mostraba
"Próximamente". Esta fase la reemplaza por contenido real: crear objetivos terapéuticos,
agregarles indicadores de seguimiento simples, cambiar su estado a lo largo del tratamiento, y
vincularlos con las sesiones donde se trabajaron — en ambas direcciones (desde la sesión se ve
qué objetivos se trabajaron; desde el objetivo se ve en qué sesiones).

## Alcance

Dentro de esta fase: `therapeutic_goals` (CRUD completo, archivado/restauración),
`goal_indicators` (crear/editar/eliminar), `session_goals` (vínculo N:M con validación de
integridad de paciente), integración mínima con `SessionDetailScreen` (Fase 4) para gestionar el
vínculo desde el lado de la sesión.

Fuera de alcance (deliberadamente, ver aprobación de Fase 5): `goal_interventions` y su vínculo
con `clinical_techniques` (Herramientas), Formulación (`case_formulations`, `formulation_id` se
mapea porque la columna existe pero nunca se escribe), Antecedentes, Evaluaciones, Documentos,
Pagos, Biblioteca, Recordatorios, IA, búsqueda global, backup/export, modo privacidad, WAL,
ajustes generales, cambios a Dashboard.

## Modelo de datos usado

Exactamente el de `SCHEMA_V1` (Fase 1.3) — **sin migraciones nuevas**. Tres tablas:

```sql
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

CREATE TABLE goal_indicators (
  id TEXT PRIMARY KEY,
  goal_id TEXT NOT NULL REFERENCES therapeutic_goals(id) ON DELETE CASCADE,
  description TEXT NOT NULL,
  baseline_value TEXT,
  target_value TEXT
);

-- Tabla puente N:M sesión <-> objetivo
CREATE TABLE session_goals (
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  goal_id TEXT NOT NULL REFERENCES therapeutic_goals(id) ON DELETE CASCADE,
  progress_note TEXT,
  PRIMARY KEY (session_id, goal_id)
);
```

Dos particularidades del esquema que la implementación respeta tal cual, sin intentar
"corregirlas":

- **`goal_indicators` no tiene `deleted_at` ni columnas de fecha.** A diferencia de todo lo
  demás en el proyecto (pacientes, sesiones, citas, objetivos), un indicador eliminado se borra
  de verdad (`DELETE`), no se archiva. Esto es una decisión ya tomada por el esquema desde la
  Fase 1.3, no algo que esta fase introduce.
- **`session_goals` no tiene `id` propio.** Su clave primaria es el par
  `(session_id, goal_id)` — eso, por sí solo, impide duplicados a nivel de base de datos, sin
  necesidad de un índice único adicional.

## Arquitectura

Mismas capas que Sesiones (Fase 4), triplicadas para las tres entidades relacionadas:

```
React (features/goals/*)
   │  invoke('create_goal', { input }), invoke('link_session_goal', ...), etc.
   ▼
commands::goals   (src-tauri/src/commands/goals.rs, 19 comandos)
   │  — capa fina. Solo obtiene la conexión y delega.
   ▼
security::session::VaultSession::with_connection
   │  — vault bloqueado ⇒ Err antes de llegar a services/repositories.
   ▼
services::goals   (src-tauri/src/services/goals.rs)
   │  — reglas de negocio: validación, la regla de integridad paciente↔objetivo↔sesión,
   │    reglas de "no crear vínculos/objetivos nuevos para un paciente archivado".
   ▼
repositories::goals + repositories::goal_indicators + repositories::session_goals
   │  — SQL puro sobre las tres tablas.
   ▼
SQLite + SQLCipher (vault.db, sin migraciones nuevas)
```

## Objetivos terapéuticos: registros mutables, sin versionado

A diferencia de `session_notes` (Fase 4), un objetivo terapéutico es un **registro mutable
simple** — editarlo ejecuta un `UPDATE` directo sobre la misma fila, igual que editar un
paciente. No hay versión, no hay historial de cambios, no hay noción de "cerrado". Esta es una
decisión de producto explícita y deliberada de la aprobación de Fase 5, no una omisión: los
objetivos terapéuticos cambian de foco y de estado a lo largo del tratamiento de forma natural, y
el valor de negocio de un historial de ediciones no estaba pedido para esta fase.

`create_goal` siempre crea el objetivo en estado `activo` — el estado inicial no es
seleccionable en el formulario de creación, igual que una sesión siempre se crea en
`programada`. Cambiar el estado es una acción de edición posterior.

## Estados y transición no terminal

Los cuatro estados válidos (`activo`, `logrado`, `pausado`, `descartado`) se validan tanto en
Zod (frontend, para feedback inmediato) como en `services::goals::validate_status` (backend,
autoritativo) — un estado inválido enviado directamente por IPC, sin pasar por el formulario, se
rechaza igual.

**`logrado` no es un estado terminal.** El servicio permite cualquier transición entre los
cuatro estados sin restricción — incluida `logrado → activo`, `logrado → pausado` o
`logrado → descartado`. No hay ninguna regla de "una vez logrado, no se puede tocar": un
objetivo puede reabrirse si el criterio clínico cambia. Verificado con el test
`logrado_is_not_a_terminal_state` y confirmado manualmente en la aplicación real (ver más abajo).

## Indicadores: texto libre, sin cálculos

`goal_indicators` tiene exactamente tres campos editables: `description` (obligatorio),
`baseline_value` y `target_value` (ambos opcionales, texto libre). No hay porcentajes
automáticos, escalas, gráficos, fórmulas, semáforos ni ninguna métrica calculada — tal como
exigía la aprobación de Fase 5 ("no inventes... utiliza esos campos de manera clara y simple").
La UI muestra una explicación breve de qué significa cada campo, sin asumir que el nombre técnico
sea suficiente.

Un objetivo puede existir sin ningún indicador — no es un requisito de creación, y agregar
indicadores es una acción independiente y posterior. `create_indicator`/`update_indicator`/
`delete_indicator` no vuelven a comprobar si el paciente del objetivo está archivado: archivar no
bloquea la edición de datos hijos ya existentes, mismo criterio que
`services::sessions::autosave_note_draft` no revisa si la sesión está archivada.

## El vínculo sesión↔objetivo: la regla de integridad crítica

`session_goals` es la tabla puente N:M. La regla no negociable de esta fase — porque la FK por sí
sola no la garantiza — es:

> **`link_session_goal` verifica explícitamente que `session.patient_id == goal.patient_id`
> antes de crear cualquier vínculo.** Una sesión del paciente A jamás puede vincularse con un
> objetivo del paciente B.

`services::goals::link_session_goal` hace, en este orden: la sesión existe (`SessionNotFound` si
no), el objetivo existe (`NotFound` si no), ambos pertenecen al mismo paciente
(`PatientMismatch` si no), ese paciente no está archivado (`PatientArchived` — solo para
vínculos **nuevos**), y el vínculo no existe ya (`LinkAlreadyExists`). Cubierto explícitamente
por `rejects_linking_a_session_of_one_patient_with_a_goal_of_another`, con el escenario exacto
pedido en la aprobación: paciente A con sesión A y objetivo A, paciente B con sesión B y
objetivo B, intento de vincular sesión A + objetivo B rechazado.

Además de la prueba de backend, la interfaz **estructuralmente no puede ofrecer** un objetivo de
otro paciente: el selector "Agregar objetivo" de `SessionDetailScreen` consume
`list_available_goals_for_session`, que calcula en el backend los objetivos activos del paciente
de esa sesión que todavía no están vinculados — el frontend nunca filtra por su cuenta ni recibe
la lista completa de objetivos para filtrarla client-side.

**Duplicados:** la propia clave primaria compuesta `(session_id, goal_id)` de `SCHEMA_V1` los
impide a nivel de base de datos (`duplicate_link_violates_the_primary_key`, verificado a nivel de
repositorio). La capa de servicio verifica existencia antes de insertar
(`session_goals::exists`) para devolver `LinkAlreadyExists` — un error de dominio claro — en vez
de dejar que una violación de constraint SQL cruda llegue a la usuaria.

**Progreso del vínculo:** `session_goals.progress_note` es editable independientemente desde
`SessionDetailScreen`, vía `update_session_goal_progress` — no requiere que el paciente esté
activo (no es una operación de "crear algo nuevo").

## Archivado y restauración

Igual patrón que pacientes, citas y sesiones (soft delete real, sin `hard_delete` en ningún punto
del código para `therapeutic_goals`):

- `archive_goal` fija `deleted_at`; el objetivo desaparece del listado "Activos" pero sigue
  completo (con todos sus indicadores y vínculos con sesiones) en "Archivados".
- `restore_goal` revierte `deleted_at` a `NULL`.
- Archivar un objetivo **no** archiva ni elimina sus indicadores ni sus vínculos con sesiones —
  todo permanece intacto y consultable mientras está archivado, verificado con
  `archiving_hides_from_active_listing_but_keeps_indicators_and_links_intact` y confirmado
  manualmente.

## Pacientes archivados

- No se pueden crear objetivos nuevos para un paciente archivado (`create_goal` revisa
  `patient.deleted_at`).
- No se pueden crear vínculos nuevos sesión↔objetivo si el paciente está archivado
  (`link_session_goal` revisa lo mismo).
- Objetivos, indicadores y vínculos **existentes** de un paciente archivado siguen siendo
  consultables — archivar un paciente no oculta ni bloquea la lectura de sus objetivos, mismo
  criterio que con sus sesiones.

## Privacidad

- **IPC mínimo por construcción.** `GoalListItem` (lo que devuelven `list_goals`/
  `list_archived_goals`) no lleva `description` — solo `id`, `title`, `status`, `targetDate`,
  `indicatorCount` y `sessionCount`. El contenido completo del objetivo solo viaja por IPC
  cuando `GoalDetailScreen` lo pide explícitamente (`get_goal`). Mismo criterio que
  `SessionListItem` en Fase 4.
- **`SessionGoalRow`/`GoalSessionRow` minimizados.** Desde la sesión, solo se ve el título y
  estado del objetivo (nunca su descripción completa). Desde el objetivo, solo se ve fecha, hora
  y estado administrativo de la sesión (nunca el contenido de su nota clínica).
- **Sin objetivos en Google Calendar.** El módulo `calendar` no referencia `goals`,
  `goal_indicators` ni `session_goals` en ningún punto — verificado por inspección directa del
  código (`grep` sobre los cuatro archivos de `calendar/*.rs`: cero coincidencias) y por diseño
  (ningún import cruzado).
- **Sin contenido clínico en `location.state`, logs, ni título de ventana.** Mismas garantías
  estructurales que Fase 4 — la navegación entre pantallas de objetivos usa parámetros de ruta
  (`:patientId`, `:goalId`), nunca `location.state`.
- Auditoría manual realizada con una cadena marcador ficticia (`XYZFASE5TEST`) sembrada en
  título, descripción, indicadores y nota de progreso del vínculo: no aparece en `WebKitCache`,
  `CacheStorage`, `storage`, `hsts-storage.sqlite`, ni en el log propio de la aplicación — solo
  dentro del vault cifrado.

## Decisiones de negocio tomadas en esta fase

1. **Objetivos son registros mutables, sin versionado.** Ver sección dedicada arriba — decisión
   explícita de la aprobación de Fase 5, deliberadamente distinta de `session_notes`.
2. **Metadatos de objetivo inmutables tras la creación en cuanto a `patientId`.**
   `GoalUpdateInput` no incluye `patientId` — reasignar un objetivo a otro paciente no es una
   operación de este MVP, mismo criterio que `SessionMetadataInput` en Fase 4.
3. **Creación siempre en estado `activo`.** El estado no es seleccionable en el formulario de
   creación — se define solo al editar. Decisión interna de implementación sin impacto
   arquitectónico, análoga a como las sesiones siempre se crean en `programada`.
4. **`goal_interventions.technique_id` y `formulation_id` no se exponen ni se escriben.** Ambas
   columnas existen en `SCHEMA_V1` pero pertenecen a verticales explícitamente excluidos de esta
   fase (Herramientas, Formulación) — se dejan como el esquema las define, sin construir la
   funcionalidad que las usaría.
5. **El selector "Agregar objetivo" en la sesión solo ofrece objetivos activos.** Un objetivo
   archivado no aparece como opción para un vínculo nuevo — decisión de producto razonable (no
   tiene sentido empezar a trabajar un objetivo ya archivado) que no está prohibida ni exigida
   explícitamente por la aprobación, pero que se alinea con el principio general de que
   "archivado" en toda la aplicación oculta de los flujos de creación sin ocultar del todo el
   dato.
6. **La función `datetime.ts` de Sesiones se reutiliza sin duplicar.** `formatSessionDate`
   (formato AAAA-MM-DD → DD-MM-AAAA) sirve igual para `target_date` de un objetivo, que tiene el
   mismo formato — se importa desde `features/sessions/datetime.ts` en vez de crear una copia en
   `features/goals/`, evitando la duplicación que la aprobación de Fase 5 pide explícitamente
   evitar. Es la única desviación de la lista de archivos propuesta en la aprobación
   (`features/goals/datetime.ts` no se creó).

## Exclusiones explícitas de esta fase

Ninguno de estos puntos se tocó, tal como exigía la aprobación:

- `goal_interventions`, `clinical_techniques`, `technique_categories`, `technique_materials` —
  Herramientas no se implementa; la FK opcional de `goal_interventions.technique_id` queda sin
  usar.
- `case_formulations` y toda la vertical de Formulación — sin React Flow, sin canvas, sin nodos
  ni aristas. `therapeutic_goals.formulation_id` se mapea en el struct `Goal` porque la columna
  existe, pero siempre se inserta `NULL` y no hay UI para asignarlo.
- `docs/SCHEMA_V1.md` y `src-tauri/src/db/migrations.rs` — sin migraciones nuevas.
- `src-tauri/src/security/*`, `src-tauri/src/calendar/*`, `src-tauri/src/db/connection.rs` — sin
  tocar.
- `session_notes.rs` (repositorio ni servicio) — sin tocar; la integración con
  `SessionDetailScreen` es puramente aditiva (una sección nueva al final de la pantalla).
- `appointments.status`, `Google Calendar`, `sesiones.status`, autoguardado y versionado de
  notas — ningún ciclo de vida de Sesiones (Fase 4) se modificó. No existe ninguna
  automatización tipo "al vincular un objetivo → cambiar sesión a realizada".
- Dashboard, Ajustes generales, Recordatorios, búsqueda global, backup, export, modo privacidad,
  WAL — sin cambios.
- Ninguna dependencia nueva — todo el frontend reutiliza `Button`, `TextField`, `Select`,
  `Textarea`, Zod, `react-hook-form`, `react-router-dom`, ya presentes desde fases anteriores.

## Archivos creados o modificados

| Archivo | Rol |
|---|---|
| `src-tauri/src/repositories/goals.rs` (nuevo) | SQL puro sobre `therapeutic_goals`. |
| `src-tauri/src/repositories/goal_indicators.rs` (nuevo) | SQL puro sobre `goal_indicators` (única tabla del proyecto con borrado real, no soft delete). |
| `src-tauri/src/repositories/session_goals.rs` (nuevo) | SQL puro sobre la tabla puente `session_goals`, en ambas direcciones de consulta. |
| `src-tauri/src/services/goals.rs` (nuevo) | Validación, orquestación, y la regla de integridad paciente↔objetivo↔sesión. |
| `src-tauri/src/commands/goals.rs` (nuevo) | 19 comandos Tauri, todos mediados por `VaultSession::with_connection`. |
| `src-tauri/src/repositories/mod.rs`, `services/mod.rs`, `commands/mod.rs`, `lib.rs` | Registro de los nuevos módulos y comandos. |
| `src/features/goals/*` (nuevo) | `types.ts`, `api.ts`, `schema.ts`, `GoalsTab.tsx`, `GoalCreateScreen.tsx`, `GoalDetailScreen.tsx`. |
| `src/features/patients/PatientDetailScreen.tsx` | Pestaña "Objetivos" ahora renderiza `GoalsTab` en vez de "Próximamente". |
| `src/features/sessions/SessionDetailScreen.tsx` | Nueva sección "Objetivos trabajados en esta sesión" (`GoalsWorkedSection`) — puramente aditiva, no modifica ningún flujo existente de la nota clínica. |
| `src/App.tsx` | Rutas `/patients/:patientId/goals/new` y `/patients/:patientId/goals/:goalId`. |

## Tests ejecutados

`cargo test` en `src-tauri/`: **247/247 en verde** (200 previos sin cambios + 47 nuevos: 7 en
`repositories::goals`, 7 en `repositories::goal_indicators`, 7 en `repositories::session_goals`,
26 en `services::goals`). `cargo clippy --all-targets`: sin advertencias. `npm run build`: sin
errores. `npm run lint`: sin errores (los `warning` de `oxlint` sobre `set-state-in-effect` en
`GoalsTab.tsx`, `GoalDetailScreen.tsx` y la nueva sección de `SessionDetailScreen.tsx` siguen
exactamente el mismo patrón ya presente en `SessionsTab.tsx`/`PatientsListScreen.tsx`/
`AgendaScreen.tsx`/`SettingsScreen.tsx` desde fases anteriores — no es una regresión de esta
fase). `cargo build`: sin errores.

Tests representativos de la regla de integridad crítica y de las reglas de negocio:

| Requisito | Test |
|---|---|
| Sesión de paciente A no puede vincularse con objetivo de paciente B | `services::goals::rejects_linking_a_session_of_one_patient_with_a_goal_of_another` |
| Vínculo válido entre sesión y objetivo del mismo paciente | `services::goals::links_a_session_and_goal_of_the_same_patient` |
| No se pueden crear vínculos nuevos para un paciente archivado | `services::goals::rejects_creating_a_new_link_for_an_archived_patient` |
| Duplicado rechazado a nivel de servicio | `services::goals::rejects_a_duplicate_link` |
| Duplicado rechazado a nivel de base de datos (constraint real) | `repositories::session_goals::duplicate_link_violates_the_primary_key` |
| `logrado` no es terminal — todas las transiciones funcionan | `services::goals::logrado_is_not_a_terminal_state` |
| Un objetivo puede existir sin indicadores | `services::goals::a_goal_can_exist_without_any_indicator` |
| Archivar preserva indicadores y vínculos intactos | `services::goals::archiving_hides_from_active_listing_but_keeps_indicators_and_links_intact` |
| Restaurar recupera todo intacto | `services::goals::restoring_brings_it_back_to_the_active_listing_with_everything_intact` |
| Una sesión puede tener múltiples objetivos | `services::goals::a_session_can_have_multiple_goals_and_a_goal_can_have_multiple_sessions` |
| El selector de "agregar objetivo" excluye ya vinculados y otros pacientes | `services::goals::available_goals_for_session_excludes_already_linked_ones_and_other_patients` |
| El progreso del vínculo se puede editar | `services::goals::update_link_progress_note_changes_it` |

## Prueba manual realizada (aplicación real, no solo tests)

Compilada con `cargo build`, ejecutada bajo Xvfb con `xdotool` (clics y tecleo reales) sobre un
vault de prueba desechable (creado, usado y eliminado en esta sesión — nunca se tocó el vault
real), con capturas de pantalla en cada paso:

1. Crear vault de prueba → desbloquear → crear paciente ficticio ("Paciente de Prueba Fase 5").
2. Pestaña "Objetivos" deja de mostrar "Próximamente" — confirmado con el empty state real.
3. Crear objetivo con marcador ficticio `XYZFASE5TEST` en el título, **sin ningún indicador** →
   guardado correctamente, estado `Activo` por defecto.
4. Empty state de indicadores confirmado ("Este objetivo todavía no tiene indicadores").
5. Crear indicador con valores de partida/meta → editar el valor de partida → confirmado
   persistido.
6. Crear un segundo indicador sin valores de partida/meta (ambos opcionales) → confirmado que la
   línea de "Partida/Meta" no se muestra cuando ambos están vacíos.
7. Editar descripción del objetivo → ciclo completo de estados en la UI real:
   `activo → pausado → logrado → activo`, confirmando en cada paso que `logrado` **no** bloquea
   volver a `activo` ni a `pausado`.
8. Archivar objetivo (con diálogo de confirmación) → desaparece de "Activos" → aparece en
   "Archivados" con el formulario visible (mismo criterio que `SessionDetailScreen`, sin
   deshabilitar campos) → Restaurar → confirmado que los dos indicadores siguen exactamente
   intactos tras el ciclo completo de archivado/restauración.
9. Crear una sesión clínica para el mismo paciente (Fase 4, sin regresión) → sección "Objetivos
   trabajados en esta sesión" visible con empty state correcto.
10. "Agregar objetivo" → selector muestra únicamente el objetivo del paciente correcto (no hay
    otros pacientes en el vault de prueba en este punto) → vincular → objetivo aparece en la
    sesión con "Sin progreso registrado."
11. "Editar progreso" → escribir nota con marcador ficticio → "Guardar progreso" → confirmado
    persistido y visible.
12. Desde el objetivo, sección "Sesiones relacionadas" muestra la sesión con fecha, hora, estado
    administrativo (Programada) y la nota de progreso — clic en la fila navega al detalle de la
    sesión real (mismo `SessionDetailScreen` de Fase 4, sin pantalla duplicada).
13. Crear una segunda sesión para el mismo paciente → vincular el **mismo** objetivo → confirmado
    que un objetivo puede estar en múltiples sesiones.
14. Crear un segundo objetivo → volver a la primera sesión → el selector "Agregar objetivo"
    ofrece únicamente el objetivo todavía no vinculado a esa sesión (excluye correctamente el ya
    vinculado) → vincular → confirmado que una sesión puede tener múltiples objetivos, ambos
    visibles y ordenados alfabéticamente.
15. **Vínculo cruzado de pacientes**: no ejercitable desde la interfaz normal porque el selector
    de objetivos está estructuralmente limitado al paciente de la sesión (`list_available_goals_for_session`) —
    confirmado que la UI nunca ofrece un objetivo de otro paciente como opción. La regla de
    rechazo en sí está probada exhaustivamente por el test automatizado
    `rejects_linking_a_session_of_one_patient_with_a_goal_of_another` con el escenario exacto de
    dos pacientes/dos sesiones/dos objetivos pedido en la aprobación.
16. **Persistencia a través de bloqueo/desbloqueo del vault**: con los vínculos y notas de
    progreso ya creados, bloquear el vault (botón "Bloquear") → desbloquear con la misma
    contraseña → confirmado que ambos vínculos y la nota de progreso siguen exactamente iguales.
17. **Cierre completo del proceso de la aplicación y reapertura real** (no solo bloquear): matar
    el proceso, relanzar el binario → arranca en estado `Locked` → desbloquear → confirmado en
    la pestaña "Objetivos" que ambos objetivos persisten con sus conteos exactos de indicadores
    (0 y 2) y sesiones (1 y 2).
18. **Auditoría de privacidad**: búsqueda del marcador `XYZFASE5TEST` en `WebKitCache`,
    `CacheStorage`, `storage`, `hsts-storage.sqlite` y el log de la aplicación — cero
    coincidencias en todos ellos. Única coincidencia en el sistema completo: el log propio de la
    herramienta externa usada para automatizar el test (no un archivo de la aplicación).
19. **Regresión de Fases 1–4**: Dashboard (Inicio), Pacientes, Agenda, Ajustes, y el ciclo
    completo de creación de sesiones (Fase 4) revisados visualmente tras el reinicio completo —
    sin cambios de comportamiento respecto a fases anteriores.
20. Limpieza: proceso de la aplicación de prueba y servidor de desarrollo de Vite detenidos,
    vault de prueba eliminado, vault real restaurado exactamente como estaba antes de empezar
    (mismo tamaño y fecha de modificación verificados).

## Limitaciones y decisiones que quedan pendientes de aprobación

Ninguna. Todas las decisiones de esta fase estaban resueltas de forma definitiva en la
aprobación formal de Fase 5, o son decisiones internas de implementación sin impacto
arquitectónico (ver "Decisiones de negocio tomadas en esta fase" arriba). El vínculo cruzado de
pacientes no se pudo ejercitar manualmente desde la UI por diseño (la interfaz lo impide
estructuralmente) — esto se documenta explícitamente como una limitación de la prueba manual, no
como una brecha de cobertura: la regla está probada exhaustivamente a nivel de backend.
