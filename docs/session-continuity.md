# Continuidad entre sesiones (Fase 8)

Documento técnico de la Fase 8. Complementa `docs/ARCHITECTURE.md` (tabla de fases, sección 17).
Cubre dos entidades operativas nuevas — preparación para la próxima sesión y tareas terapéuticas
entre sesiones — pensadas para que la información relevante entre una sesión y la siguiente no
dependa de que la profesional recuerde reabrir la nota clínica anterior.

## Propósito

Antes de esta fase, la única forma de dejar constancia de "qué preparar para la próxima sesión" o
"qué tarea quedó pendiente para el paciente" eran los campos de texto libre `next_focus` y
`homework_tasks` de `session_notes` — texto histórico, dentro de una nota que queda inmutable en
cuanto se cierra (Fase 4). Nada mostraba ese contenido de forma proactiva al abrir la sesión
siguiente: había que recordar reabrir la nota anterior y leerla entera. Esta fase agrega dos
entidades **operativas**, con su propio ciclo de vida y visibles en la ficha del paciente y en
cada sesión, sin tocar ni reinterpretar los campos existentes de la nota clínica.

## Alcance

Dentro de esta fase: tablas `patient_prep_notes` y `therapy_tasks` (migración `V3`), su CRUD
completo con transiciones de estado, un panel de continuidad (`SessionContinuityPanel`) visible
tanto en la ficha del paciente (pestaña "Sesiones") como dentro de cada `SessionDetailScreen`,
vínculo opcional de una tarea con un objetivo terapéutico, y el conteo real de "Tareas clínicas
pendientes" en el Dashboard.

Fuera de alcance (deliberadamente, ver aprobación de Fase 8): Cierre/alta, `treatment_episodes`
(episodios/procesos terapéuticos como entidad propia — ver sección dedicada más abajo), Plan de
seguridad, Derivaciones, Red clínica, Consentimientos, Documentos, Formulación, Evaluaciones,
Herramientas/Técnicas, Biblioteca, `reminders` genéricos, Plantillas, Backup, modo WAL, Export,
búsqueda global, Modo Privacidad, Ajustes generales, biometría, FileVault/BitLocker, sync
multidispositivo, iOS/iPadOS, SII, firma electrónica, IA clínica o generación automática de
contenido clínico a partir de notas. Ninguno de estos puntos se tocó.

## Modelo de datos: migración `V3`

Primera migración desde `V2` (Fase 6.1) — `SCHEMA_V1` y `SCHEMA_V2` quedan intactos, verificado
por `v3_migration_preserves_all_existing_data`. Dos tablas nuevas, completamente aditivas:

```sql
CREATE TABLE patient_prep_notes (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  origin_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  content TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pendiente'
    CHECK (status IN ('pendiente','abordado','descartado')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_patient_prep_notes_patient_status ON patient_prep_notes(patient_id, status);

CREATE TABLE therapy_tasks (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  assigned_in_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  goal_id TEXT REFERENCES therapeutic_goals(id) ON DELETE SET NULL,
  description TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pendiente'
    CHECK (status IN ('pendiente','parcial','realizada','no_realizada','descartada')),
  assigned_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  review_due_at TEXT,
  reviewed_in_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  reviewed_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_therapy_tasks_patient_status ON therapy_tasks(patient_id, status);
CREATE INDEX idx_therapy_tasks_goal ON therapy_tasks(goal_id);
```

Ambas tablas llevan su propio trigger `trg_*_touch_updated_at`, mismo patrón que el resto del
esquema desde `SCHEMA_V1`.

## Por qué no bastaba con `next_focus`/`homework_tasks`

`session_notes.next_focus` y `session_notes.homework_tasks` **no se tocan, no se eliminan y su
semántica no se reinterpreta**. Siguen siendo exactamente lo que eran desde la Fase 4: texto
histórico dentro de una nota versionada, inmutable en cuanto se cierra (`is_locked = 1`). Esa
inmutabilidad es una garantía deliberada de la Fase 4 — y es precisamente lo que los vuelve
inadecuados para representar un ítem operativo:

- Un campo de una nota cerrada no tiene estado propio (no se puede marcar "ya lo aborde" sin
  reabrir/reversionar la nota completa, violando el versionado append-only).
- No hay forma de listar "todo lo pendiente de todos los pacientes" sin recorrer notas.
- No se puede vincular a un objetivo terapéutico ni fijar una fecha de revisión.
- Nada los muestra proactivamente al abrir la sesión siguiente.

`patient_prep_notes` y `therapy_tasks` son entidades operativas independientes, con su propio
ciclo de vida, que conviven con `next_focus`/`homework_tasks` sin sustituirlos. Una profesional
puede seguir usando el texto libre de la nota para el registro narrativo de la sesión y, además,
crear una tarea o una preparación puntual cuando corresponda — son complementarios, no
excluyentes.

## Arquitectura

Mismas capas que el resto de las verticales (Objetivos, Antecedentes, Pagos):

```
React (features/prep-notes/*, features/therapy-tasks/*, SessionContinuityPanel)
   │  invoke('create_prep_note', ...), invoke('create_therapy_task', ...), etc.
   ▼
commands::patient_prep_notes / commands::therapy_tasks   (15 comandos en total)
   │  — capa fina, mediada por VaultSession::with_connection.
   ▼
services::patient_prep_notes / services::therapy_tasks
   │  — validación, reglas de integridad sesión↔paciente y objetivo↔paciente,
   │    bloqueo de creación para paciente archivado.
   ▼
repositories::patient_prep_notes / repositories::therapy_tasks
   │  — SQL puro.
   ▼
SQLite + SQLCipher (vault.db, migración V3)
```

## `patient_prep_notes`: preparación para la próxima sesión

Tres estados: `pendiente` (recién creada o "vuelta a pendiente"), `abordado`, `descartado`. Una
preparación es editable (`update_prep_note`) **solo mientras está `pendiente`** —
`update_prep_note_rejects_editing_a_resolved_note` cubre que intentar editar una nota `abordado`/
`descartado` se rechaza con `PrepNoteError::NotEditable`, para que el contenido de una decisión ya
tomada no cambie retroactivamente. Marcar `abordado`/`descartado` (o volver a `pendiente`) es una
acción explícita separada (`set_prep_note_status`) — nunca implícita ni automática.

`origin_session_id` es **opcional a propósito** (regla explícita de la aprobación de Fase 8): una
preparación nunca depende de que exista una cita futura agendada — se puede crear sin sesión de
origen, y `a_prep_note_never_requires_a_future_appointment` lo verifica directamente. Cuando sí
existe, se valida que la sesión de origen pertenezca al mismo paciente
(`rejects_an_origin_session_belonging_to_a_different_patient`).

**Sin `deleted_at`.** A diferencia de `therapeutic_goals`/`payments`/`sessions`, el ciclo de vida
completo de esta entidad ya queda representado por su propio `status`: ninguno de los tres estados
oculta la fila permanentemente, y no hay ningún caso de uso de "ocultar esta fila sin perder el
dato" distinto de simplemente marcarla `descartado`. Agregar soft-delete encima habría sido un
segundo mecanismo redundante para el mismo propósito — decisión documentada en el comentario de
diseño de `SCHEMA_V3` en `migrations.rs`.

Una preparación resuelta (`abordado`/`descartado`) nunca se borra ni se pierde: sigue disponible
en el historial de la sesión/paciente, simplemente deja de contar como pendiente
(`set_prep_note_status_marks_as_abordado_and_preserves_history`).

## `therapy_tasks`: tareas entre sesiones

Cinco estados: `pendiente`, `parcial`, `realizada`, `no_realizada` — los cuatro pedidos
explícitamente por la aprobación — más **`descartada`**, agregada con justificación explícita
(la aprobación exigía no añadir estados sin argumentarlos):

> `descartada` cubre una tarea que deja de ser relevante **antes** de llegar a revisarse en
> ninguna sesión (el foco terapéutico cambió, la tarea resultó duplicada, etc.) — un caso distinto
> de `no_realizada`, que en el flujo documentado va emparejada con un evento de revisión real
> (`reviewed_in_session_id`/`reviewed_at` completos). Verificado por
> `pendiente_to_descartada_without_ever_being_reviewed_in_a_session`, que confirma que descartar
> una tarea no exige haberla revisado en ninguna sesión.

Campos relevantes:

- `assigned_in_session_id` (opcional, `ON DELETE SET NULL`): la sesión en la que se asignó la
  tarea, si corresponde — igual patrón que `payments.session_id` (Fase 7).
- `goal_id` (opcional, `ON DELETE SET NULL`): vínculo con un objetivo terapéutico — ver sección
  dedicada abajo.
- `review_due_at` (opcional): fecha sugerida para revisar el resultado; puramente informativo, sin
  ningún job ni recordatorio automático asociado (`reminders` sigue sin implementarse).
- `reviewed_in_session_id`/`reviewed_at`: se completan al revisar la tarea **si** la revisión
  ocurre desde el contexto de una sesión concreta; `set_review` deja ambos campos intactos cuando
  no se provee una sesión de revisión (`set_review_without_session_leaves_review_fields_untouched`)
  — es exactamente el mecanismo que permite que `descartada` no dependa de una revisión.

**Con `deleted_at`.** A diferencia de `patient_prep_notes`, archivar una tarea es un acto
administrativo distinto de cualquiera de sus cinco estados clínicos — mismo criterio que separar
`archived`/`status` en Objetivos y Pagos. `archive_task`/`restore_task` siguen el mismo patrón de
soft-delete real que el resto del esquema.

`TherapyTaskListItem` (lo que devuelven los comandos de listado) incluye `goalTitle` vía `LEFT
JOIN` con `therapeutic_goals`, para mostrar el nombre del objetivo vinculado sin una segunda
consulta desde React — y sigue mostrando el título incluso si ese objetivo fue archivado después
(`a_task_linked_to_an_archived_goal_still_shows_its_title`), porque archivar un objetivo no rompe
vínculos ya creados.

## Reglas de integridad: sesión y objetivo siempre re-verificados contra el paciente

Mismo criterio no negociable que Objetivos (Fase 5) y Pagos (Fase 7): **nunca se confía en el
`patientId` que llega desde React sin validarlo contra la fila real relacionada**.

- `check_session_belongs_to_patient` se usa tanto en `patient_prep_notes` (para
  `origin_session_id`) como en `therapy_tasks` (para `assigned_in_session_id` y
  `reviewed_in_session_id`) — rechaza con `SessionNotFound`/`SessionPatientMismatch` cuando la
  sesión no existe o pertenece a otro paciente.
- `check_goal_belongs_to_patient` (nueva en esta fase, análoga a la de sesión) verifica que
  `goal_id` pertenezca al mismo paciente que la tarea, tanto al crear (`create_task`) como al
  actualizar (`update_task`, cuando `goal_id` cambia) —
  `rejects_a_goal_belonging_to_a_different_patient` y
  `update_task_rejects_a_goal_belonging_to_a_different_patient` lo cubren explícitamente. El
  selector de la UI ("Vincular a objetivo") solo lista los objetivos del paciente de esa sesión,
  pero esa es una restricción de conveniencia, nunca la única barrera — la autoridad está en el
  servicio.

## Vínculo con objetivo: siempre opcional

`goal_id` es `NULL` por defecto, y el flujo completo de crear/editar una tarea funciona igual sin
él (`creates_a_task_without_a_goal`). El vínculo existe para permitir, cuando así lo decide la
profesional, mostrar de forma explícita qué objetivo terapéutico avanza una tarea concreta — nunca
es obligatorio ni se infiere automáticamente.

## Sin conversión automática de texto clínico

Ninguna nota clínica se analiza, resume ni convierte automáticamente en una tarea o una
preparación — crear cualquiera de las dos entidades es siempre una acción explícita de la
profesional desde el panel de continuidad, nunca una inferencia sobre el contenido de
`session_notes`. No hay IA, heurística de texto, ni job en background que lea notas cerradas para
generar filas nuevas.

## Consideración explícita: episodios/procesos terapéuticos (documentada, NO implementada)

La aprobación de Fase 8 pidió considerar explícitamente, sin implementarla, una futura separación
conceptual entre **paciente** y **episodio/proceso terapéutico** (un mismo paciente podría, en el
futuro, tener más de un proceso terapéutico distinto a lo largo del tiempo). La pregunta concreta
era si `patient_prep_notes`/`therapy_tasks` debían anclarse a un `episode_id` especulativo "por si
acaso" en vez de (o además de) `patient_id`.

**Decisión: no se agrega ningún `episode_id`, ni aquí ni en ninguna tabla existente.** Motivos:

1. Ninguna tabla del esquema actual —`sessions`, `therapeutic_goals`, `payments`,
   `patient_clinical_profile`, `case_formulations`, etc.— tiene hoy un `episode_id`. Agregarlo
   únicamente en las dos tablas nuevas de esta fase sería inconsistente con el resto del esquema,
   sin resolver el problema de fondo (todas las demás tablas seguirían ancladas solo a
   `patient_id`).
2. Una futura migración hacia episodios necesitará, de todos modos, tocar todas esas tablas a la
   vez de forma coordinada (probablemente insertando una fila de "episodio implícito" por paciente
   existente y re-vinculando cada tabla). Anticipar esa migración solo en dos tablas no la
   simplifica ni la vuelve más segura — solo introduce una columna sin uso real hoy.
3. No hay, en este momento, evidencia de que los episodios sean indispensables para que Fase 8
   funcione: `patient_prep_notes`/`therapy_tasks` ancladas a `patient_id`, exactamente igual que
   el resto del esquema, cubren el caso de uso pedido (continuidad entre sesiones de un mismo
   paciente) sin ambigüedad.

`patient_prep_notes.patient_id` y `therapy_tasks.patient_id` quedan, por lo tanto, en la misma
posición que cualquier otra tabla del esquema respecto a una futura migración de episodios: ni
mejor preparadas ni peor preparadas que `sessions`/`therapeutic_goals`/`payments`. Si en una fase
futura se aprueba formalmente introducir episodios, esa fase deberá diseñar la migración de forma
coordinada sobre todo el esquema — no es una decisión que esta fase deba ni pueda anticipar
unilateralmente. Ninguna evidencia recogida durante esta fase sugirió que fuera necesario detenerse
y plantear ese cambio arquitectónico ahora.

## Paciente archivado

- No se pueden crear preparaciones ni tareas nuevas para un paciente archivado (`create_prep_note`
  y `create_task` revisan `patient.deleted_at`, con la autoridad en el backend — la UI solo oculta
  los botones "Agregar"/"Agregar tarea" como refuerzo).
- Las preparaciones y tareas **existentes** de un paciente archivado siguen siendo consultables y
  editables (contenido, estado, revisión) — archivar un paciente no oculta ni bloquea la corrección
  de datos ya registrados, mismo criterio que sesiones, objetivos y pagos. Verificado por
  `editing_a_historical_task_of_an_archived_patient_is_allowed` (y su equivalente para
  preparaciones).
- Ninguna función de actualización (`update_prep_note`, `set_prep_note_status`, `update_task`,
  `review_task`) vuelve a comprobar el estado archivado del paciente — solo la creación lo hace.

## UX: panel de continuidad, sin monolito nuevo

`SessionContinuityPanel` es un componente delgado que compone `PrepNotesSection` y
`TherapyTasksSection` (ambos en sus propias carpetas de *feature*,
`features/prep-notes`/`features/therapy-tasks`), y se renderiza en dos lugares:

- **`SessionsTab.tsx`** (pestaña "Sesiones" de la ficha del paciente): sin `sessionId`, muestra
  todo lo pendiente del paciente independientemente de la sesión.
- **`SessionDetailScreen.tsx`**: con el `sessionId` de esa sesión concreta, para que crear una
  tarea o revisar una desde ahí quede vinculada a esa sesión (`assignedInSessionId`/
  `reviewedInSessionId`) sin un paso adicional.

`SessionDetailScreen.tsx` (ya el archivo más grande del frontend desde fases anteriores) recibió
únicamente un `import` y una línea de render — toda la lógica nueva vive en los componentes
nuevos, evitando agrandar ese archivo. Cada sección tiene su propio "Ver historial" que expande la
lista de ítems ya resueltos (`abordado`/`descartado`/`parcial`/`realizada`/`no_realizada`/
`descartada`) sin mezclarlos con la lista de pendientes — confirmado manualmente: resolver un ítem
lo saca de "pendientes" y lo deja disponible en "historial" en el mismo momento, sin recargar la
página.

## Dashboard: conteo real, sin inventar "Documentos"

`therapyTasksApi.pendingCount()` (`get_pending_therapy_task_count`, conteo global vía `COUNT(*)`
en el backend, nunca una lista completa descargada para contarla en el cliente) reemplaza el
placeholder fijo de la tarjeta "Pendientes", que ahora muestra "Tareas clínicas pendientes: N" con
una nota explícita de que notas sin cerrar y documentos pendientes se sumarán "cuando existan esas
funcionalidades" — sin fingir que Documentos ya existe.

Como agregado pequeño y trivial (mismo patrón de `strftime('%Y-%m', ...) = strftime('%Y-%m',
'now')` ya usado en `payments::dashboard_summary`), se incluyó también `sessionsApi.thisMonthCount()`
(`get_sessions_this_month_count`), reemplazando el placeholder de "Sesiones del mes" que quedaba
pendiente desde la Fase 7. Excluye sesiones canceladas y de pacientes archivados.

## Privacidad

- **Sin logging de contenido clínico.** Ninguno de los seis archivos nuevos de backend
  (`repositories`/`services`/`commands` × 2 entidades) contiene una sola llamada a
  `log::`/`println!`/`dbg!`/`eprintln!` — verificado por inspección directa (`grep` sin
  coincidencias).
- **IPC mínimo por construcción.** `TherapyTaskListItem` no lleva `patientId` (el listado ya está
  scoped a un paciente) y solo agrega `goalTitle` — nunca notas del objetivo ni ningún otro campo
  clínico adicional.
- **`location.state`/props solo llevan identificadores opacos.** `SessionContinuityPanel` recibe
  `patientId`/`sessionId` como identificadores, nunca contenido clínico pasado como prop.
- **Sin envío externo.** Ninguna de las dos entidades nuevas se referencia desde `calendar/*` ni
  desde ningún otro módulo que hable con servicios externos.
- **Auditoría manual con marcador ficticio (`XYZFASE8CONTINUIDAD`)**, sembrado en el contenido de
  una preparación y de una tarea de prueba reales: no aparece en `WebKitCache`, `HSTS storage`, ni
  en el log propio de la aplicación (`~/.local/share/com.jpcaamano.cuadernoclinico/logs/Cuaderno
  Clínico.log`, que solo registra el evento genérico de migración de base de datos) — cero
  coincidencias fuera de `vault.db`, cifrado con SQLCipher.

## Decisiones de negocio tomadas en esta fase

1. **`patient_prep_notes` sin `deleted_at`; `therapy_tasks` con `deleted_at`.** Evaluado
   explícitamente para cada entidad por separado, no copiado mecánicamente del resto del esquema —
   ver justificación en las secciones dedicadas arriba.
2. **`descartada` agregada a `therapy_tasks`, con justificación explícita.** Cubre una tarea
   descartada sin haber sido revisada nunca — caso no cubierto por `no_realizada`.
3. **`origin_session_id` y `assigned_in_session_id`/`reviewed_in_session_id` siempre opcionales.**
   Ninguna preparación ni tarea depende de que exista una cita o sesión futura agendada.
4. **`goal_id` siempre opcional**, con la misma regla de integridad sesión↔paciente ya establecida
   para objetivos y pagos, extendida aquí a objetivo↔paciente.
5. **Sin `episode_id` especulativo.** Decisión explícita, con motivos documentados en la sección
   dedicada — ambas tablas quedan en la misma posición que el resto del esquema respecto a una
   futura migración de episodios, nunca mejor ni peor preparadas.
6. **`next_focus`/`homework_tasks` no se tocan.** Sin migración de datos, sin reinterpretación de
   semántica, sin deprecación — siguen siendo el registro narrativo de la nota clínica.
7. **Sin conversión automática de notas a tareas/preparaciones.** Siempre una acción explícita de
   la profesional.
8. **"Sesiones del mes" del Dashboard incluido como agregado pequeño y trivial**, cerrando el
   placeholder que había quedado pendiente desde la Fase 7 — no se trató como fuera de alcance
   porque la propia aprobación lo permitía si resultaba trivial, y reutiliza exactamente el mismo
   patrón SQL que "Ingresos del mes".

## Exclusiones explícitas de esta fase

Ninguno de estos puntos se tocó, tal como exigía la aprobación: Cierre/alta, `treatment_episodes`
como entidad implementada, Plan de seguridad, Derivaciones, Red clínica, Consentimientos,
Documentos, Formulación, Evaluaciones, Herramientas/Técnicas, Biblioteca, `reminders` genéricos,
Plantillas, Backup, modo WAL, Export, búsqueda global, Modo Privacidad, Ajustes generales,
biometría, FileVault/BitLocker, sync multidispositivo, iOS/iPadOS, SII, firma electrónica, IA
clínica, generación automática de contenido clínico. Ninguna dependencia nueva — todo el frontend
reutiliza los mismos componentes (`Button`, `Textarea`, `Select`) y patrones (`useEffect` + estado
local) ya presentes desde fases anteriores; el backend reutiliza exactamente el mismo patrón de
capas, sin repositorios genéricos, sin bus de eventos, sin máquina de estados genérica.

## Archivos creados o modificados

| Archivo | Rol |
|---|---|
| `src-tauri/src/db/migrations.rs` | `SCHEMA_V3` (dos tablas nuevas), registro en `migrations()`, 5 tests nuevos de migración. |
| `src-tauri/src/repositories/patient_prep_notes.rs` (nuevo) | SQL puro sobre `patient_prep_notes`. |
| `src-tauri/src/services/patient_prep_notes.rs` (nuevo) | Validación, integridad sesión↔paciente, bloqueo de creación para paciente archivado, regla de edición solo si `pendiente`. |
| `src-tauri/src/commands/patient_prep_notes.rs` (nuevo) | 6 comandos Tauri. |
| `src-tauri/src/repositories/therapy_tasks.rs` (nuevo) | SQL puro sobre `therapy_tasks`, incluido el `LEFT JOIN` con objetivos. |
| `src-tauri/src/services/therapy_tasks.rs` (nuevo) | Validación, integridad sesión↔paciente y objetivo↔paciente, revisión con/sin sesión, archivar/restaurar. |
| `src-tauri/src/commands/therapy_tasks.rs` (nuevo) | 9 comandos Tauri. |
| `src-tauri/src/repositories/sessions.rs` | Nueva función `count_this_month()` para el Dashboard. |
| `src-tauri/src/services/sessions.rs` | Nueva función `sessions_this_month_count()`. |
| `src-tauri/src/commands/sessions.rs`, `lib.rs`, `repositories/mod.rs`, `services/mod.rs`, `commands/mod.rs` | Registro de los módulos y comandos nuevos. |
| `src/features/prep-notes/*` (nuevo) | `types.ts`, `api.ts`, `PrepNotesSection.tsx`. |
| `src/features/therapy-tasks/*` (nuevo) | `types.ts`, `api.ts`, `TherapyTasksSection.tsx`. |
| `src/features/sessions/SessionContinuityPanel.tsx` (nuevo) | Compone ambas secciones. |
| `src/features/sessions/SessionDetailScreen.tsx` | Import + una línea de render del panel — sin lógica nueva en el archivo. |
| `src/features/sessions/SessionsTab.tsx` | Panel de continuidad renderizado sobre la tabla de sesiones. |
| `src/features/sessions/api.ts` | `thisMonthCount()`. |
| `src/features/dashboard/DashboardScreen.tsx` | "Pendientes" con conteo real de tareas; "Sesiones del mes" con valor real. |
| `src/features/dashboard/ComingSoonCard.tsx` (eliminado) | Quedó sin ningún uso tras el cambio anterior — verificado por `grep` antes de eliminarlo. |

## Tests ejecutados

`cargo test` en `src-tauri/`: **423/423 en verde** (355 previos sin cambios + 68 nuevos: 16 en
`services::patient_prep_notes`, 7 en `repositories::patient_prep_notes`, 23 en
`services::therapy_tasks`, 15 en `repositories::therapy_tasks`, 5 en `db::migrations` para `V3`, 2
en `repositories::sessions` para `count_this_month`). `cargo clippy --all-targets`: sin
advertencias. `npm run build`: sin errores. `npm run lint`: 19 warnings, exactamente las mismas dos
categorías ya presentes desde fases anteriores (`react(incompatible-library)`,
`react(set-state-in-effect)`) — verificado que ninguno de los 19 se origina en un archivo nuevo de
esta fase. `cargo build`: sin errores.

Tests representativos de las reglas de negocio centrales:

| Requisito | Test |
|---|---|
| `V3` preserva datos de `V1`+`V2` | `db::migrations::v3_migration_preserves_all_existing_data` |
| Una preparación nunca requiere sesión futura agendada | `services::patient_prep_notes::a_prep_note_never_requires_a_future_appointment` |
| No se puede editar una preparación ya resuelta | `services::patient_prep_notes::update_prep_note_rejects_editing_a_resolved_note` |
| Resolver una preparación preserva su historial | `services::patient_prep_notes::set_prep_note_status_marks_as_abordado_and_preserves_history` |
| Sesión de origen de otro paciente rechazada | `services::patient_prep_notes::rejects_an_origin_session_belonging_to_a_different_patient` |
| `descartada` sin haber sido revisada nunca | `services::therapy_tasks::pendiente_to_descartada_without_ever_being_reviewed_in_a_session` |
| Revisar sin sesión no toca los campos de revisión | `repositories::therapy_tasks::set_review_without_session_leaves_review_fields_untouched` |
| Objetivo de otro paciente rechazado al crear | `services::therapy_tasks::rejects_a_goal_belonging_to_a_different_patient` |
| Objetivo de otro paciente rechazado al editar | `services::therapy_tasks::update_task_rejects_a_goal_belonging_to_a_different_patient` |
| Tarea vinculada a objetivo archivado sigue mostrando su título | `repositories::therapy_tasks::a_task_linked_to_an_archived_goal_still_shows_its_title` |
| No se pueden crear tareas/preparaciones para un paciente archivado | `services::therapy_tasks::rejects_creation_for_an_archived_patient` (y equivalente en `patient_prep_notes`) |
| Tareas/preparaciones históricas de un paciente archivado siguen editables | `services::therapy_tasks::editing_a_historical_task_of_an_archived_patient_is_allowed` (y equivalente) |

## Prueba manual realizada (aplicación real, no solo tests)

Compilada con `cargo build`, ejecutada bajo Xvfb con `xdotool` (clics y tecleo reales) sobre un
vault de prueba desechable (el vault real se guardó aparte antes de empezar y se restauró
exactamente al terminar), con capturas de pantalla en cada paso:

1. Crear vault de prueba → Dashboard confirmado en vacío ("Tareas clínicas pendientes: 0",
   "Sesiones del mes: 0", ambos reales, no placeholders).
2. Crear paciente ficticio ("Camila Torres Vidal") y un objetivo terapéutico real para probar el
   vínculo opcional.
3. Desde la pestaña "Sesiones" del paciente: crear una preparación para la próxima sesión y una
   tarea entre sesiones (sin sesión ni objetivo asociados) → confirmadas visibles en el panel de
   continuidad de la ficha.
4. Crear una sesión clínica real → confirmado que el mismo panel de continuidad, con los mismos
   ítems pendientes, aparece también dentro de `SessionDetailScreen`.
5. Marcar la tarea como "Parcial" desde dentro de la sesión → desaparece de "pendientes" →
   "Ver historial" confirma que sigue disponible con la etiqueta "Parcial" y la opción "Volver a
   pendiente".
6. Marcar la preparación como "Abordado" desde dentro de la sesión → mismo comportamiento: sale de
   pendientes, queda en historial sin perderse.
7. Crear una segunda tarea vinculada al objetivo creado en el paso 2, desde dentro de la sesión →
   confirmado que el selector solo ofrece objetivos de ese paciente, y que la tarea guardada
   muestra el título del objetivo vinculado.
8. Archivar el paciente → confirmado que los botones "Agregar"/"Agregar tarea" desaparecen, pero
   el ítem pendiente existente sigue siendo revisable ("Marcar revisión" funciona con normalidad)
   → Restaurar paciente → botones de creación disponibles de nuevo.
9. **Persistencia a través de un cierre completo del proceso y reapertura real** (no solo
   bloquear): matar el proceso, relanzar el binario → arranca bloqueado → desbloquear → Dashboard
   confirmado con "Tareas clínicas pendientes: 1" y "Sesiones del mes: 1", coincidiendo
   exactamente con lo creado antes del reinicio.
10. **Auditoría de privacidad**: búsqueda del marcador `XYZFASE8CONTINUIDAD` (sembrado en el
    contenido de una preparación y una tarea de un paciente de prueba dedicado) en todo el
    directorio del vault (`WebKitCache`, `hsts-storage.sqlite`, logs) — cero coincidencias fuera de
    `vault.db`.
11. **Regresión funcional**: Pacientes (activos/archivados), Objetivos, Agenda y Dashboard
    revisados en el mismo flujo, sin cambios de comportamiento respecto a fases anteriores.
12. Limpieza: proceso de la aplicación de prueba y Xvfb detenidos, vaults de prueba conservados
    bajo nombres de respaldo identificados (nunca eliminados, por la regla permanente del
    proyecto), vault real restaurado exactamente como estaba antes de empezar.

## Limitaciones y decisiones que quedan pendientes de aprobación

- Ninguna. Todas las decisiones de esta fase estaban resueltas de forma definitiva en la
  aprobación formal de Fase 8, o son decisiones internas de implementación sin impacto
  arquitectónico (ver "Decisiones de negocio tomadas en esta fase" arriba). La consideración sobre
  episodios/procesos terapéuticos queda documentada explícitamente como **no implementada** — una
  futura fase que decida introducirlos deberá presentarse como cambio arquitectónico propio, con
  su propio análisis de impacto sobre todo el esquema, no como una extensión de esta fase.
