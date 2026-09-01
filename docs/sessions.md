# Sesiones clínicas y notas versionadas (Fase 4)

Documento técnico de la Fase 4. Complementa `docs/ARCHITECTURE.md` (secciones
2.4, 6 y 12.5) y `docs/patients-vertical.md` (Fase 1.5, cuyo patrón de capas
se reutiliza sin cambios). Cubre el segundo vertical funcional completo del
cuaderno clínico: registrar sesiones y llevar notas clínicas con historial
íntegro y a prueba de edición retroactiva.

## Propósito

Antes de esta fase, "Sesiones" era una pestaña de la ficha del paciente que
mostraba "Próximamente". Esta fase la reemplaza por contenido real: crear
sesiones (vinculadas o no a una cita de Agenda), escribir notas clínicas
sobre cada sesión, cerrarlas, y — si hace falta corregir algo después —
editarlas sin que la versión original desaparezca nunca.

## Arquitectura

Misma cadena de capas que Pacientes (Fase 1.5), duplicada para dos entidades
nuevas relacionadas entre sí:

```
React (features/sessions/*)
   │  invoke('create_session', { input }), invoke('close_current_note', ...), etc.
   ▼
commands::sessions   (src-tauri/src/commands/sessions.rs)
   │  — capa fina, 13 comandos. Solo obtiene la conexión y delega.
   │  state.with_connection(|conn| services::sessions::create_session(conn, input))
   ▼
security::session::VaultSession::with_connection
   │  — vault bloqueado ⇒ Err antes de llegar a services/repositories.
   ▼
services::sessions   (src-tauri/src/services/sessions.rs, 904 líneas)
   │  — reglas de negocio: validación de fecha/hora/modalidad/estado,
   │    coherencia con Agenda, reglas de versionado, transacciones atómicas.
   ▼
repositories::sessions + repositories::session_notes
   │  — SQL puro sobre dos tablas relacionadas 1-a-N.
   ▼
SQLite + SQLCipher (vault.db, tablas `sessions` y `session_notes`, ya
existentes desde la Fase 1.3 — sin migraciones nuevas en esta fase)
```

`sessions` y `session_notes` estaban definidas en el esquema desde la Fase
1.3 pero sin repositorio, servicio ni comando: esta fase es la primera que
las usa. No se agregó ninguna columna ni tabla nueva.

## Flujo de creación de una sesión

Dos caminos, ambos terminan en el mismo `create_session` del servicio:

- **Flujo A — desde la ficha del paciente.** Pestaña "Sesiones" → "Nueva
  sesión" → formulario en blanco (fecha, hora, duración, modalidad). No
  requiere una cita de Agenda.
- **Flujo B — desde una cita de Agenda.** `AppointmentDetailScreen` muestra
  una fila "Sesión clínica" cuando la cita tiene paciente asociado. Si no
  existe sesión para esa cita, el botón "Iniciar sesión" navega al mismo
  formulario de creación, mandando el `appointmentId` por query string
  (`?appointmentId=...`, nunca por `location.state`) y precargando fecha,
  hora, duración y modalidad desde la cita. Si ya existe una sesión, la fila
  muestra "Ver sesión" en su lugar — nunca ambos botones a la vez, y nunca
  se puede volver a crear una sesión para la misma cita.

`create_session` valida, en una única transacción (`unchecked_transaction`):

1. El paciente existe y no está archivado.
2. Si viene `appointment_id`: la cita existe, tiene paciente, y ese paciente
   coincide con `patient_id` — y no existe ya una sesión para esa cita
   (activa **o archivada**: archivar una sesión no libera el cupo de "una
   sesión por cita").
3. Fecha, hora, duración y modalidad tienen formato válido.
4. Inserta la fila en `sessions` **y** su primera nota (`session_notes`,
   versión 1, borrador) en la misma transacción — nunca queda una sesión
   sin nota, ni una nota huérfana.

## Ciclo de vida de una nota clínica

Cada nota pasa por dos ejes de estado, independientes entre sí:

- **Borrador ↔ Cerrada** (`is_locked`, `closed_at`): mientras es borrador se
  autoguarda en cada cambio (debounce ~1.5s en el frontend); al cerrarla
  queda inmutable.
- **Vigente ↔ Reemplazada** (`is_current`, `superseded_at`): solo la versión
  vigente se edita o se muestra por defecto; las reemplazadas solo se ven en
  "Ver historial", siempre de solo lectura.

```
crear sesión ──► v1 (borrador, vigente)
                    │ autoguardado continuo
                    ▼
                 "Cerrar nota" ──► v1 (cerrada, vigente)
                    │
                    │ "Editar" (con confirmación)
                    ▼
                 v2 (borrador, vigente)      v1 (cerrada, reemplazada) ◄── nunca cambia
                    │ ...mismo ciclo...
                    ▼
                 v3 (borrador, vigente)      v2 (cerrada, reemplazada)
                                             v1 (cerrada, reemplazada)
```

## El invariante central: append-only real, no solo por convención

La regla no negociable de la Fase 4 es que **una nota cerrada nunca se
modifica con un UPDATE de contenido** — editarla siempre crea una fila
nueva. Esto está garantizado en tres niveles independientes, no solo en la
capa de servicio:

1. **SQL estructural.** `update_draft_content` (repositories/session_notes.rs)
   incluye `WHERE is_locked = 0` en el propio UPDATE — un intento de escribir
   contenido sobre una nota cerrada actualiza cero filas, sin importar qué
   llame a la función. Probado explícitamente:
   `updating_a_locked_note_directly_changes_nothing`.
2. **Restricción de base de datos.** El índice único parcial
   `idx_session_notes_current ON session_notes(session_id) WHERE is_current = 1`
   (Fase 1.3) hace que sea imposible, a nivel de SQLite, tener dos versiones
   vigentes simultáneas para la misma sesión — probado con
   `the_database_itself_rejects_two_current_versions_for_the_same_session`.
3. **Orden de operaciones en el servicio.** `create_new_note_version` marca
   la versión vieja como `superseded` (con `mark_superseded`, que fija
   `is_current = 0` y `superseded_at`) **antes** de insertar la nueva fila
   con `is_current = 1` — así nunca hay un instante, ni siquiera dentro de
   la transacción, en que existan dos vigentes.

El test `three_consecutive_versions_are_all_preserved_correctly` (y su
confirmación manual en la UI real, ver más abajo) demuestra que v1, v2 y v3
conviven con sus contenidos originales intactos después de dos ediciones.

## Reglas de cierre

- Cerrar una nota vacía (los cuatro campos en blanco o solo espacios) se
  rechaza — un solo campo con contenido real es suficiente.
- Cerrar una nota ya cerrada es **idempotente** — no falla ni crea una
  versión nueva, solo confirma el estado actual.
- Editar una nota cerrada exige confirmación explícita en la UI ("Editar
  esta nota creará una nueva versión. La versión anterior permanecerá
  intacta y seguirá disponible en el historial.") antes de crear la versión
  siguiente.
- Pedir una versión nueva mientras la vigente sigue siendo un borrador no
  hace nada — no se acumulan versiones vacías
  (`requesting_a_new_version_while_the_current_one_is_still_a_draft_changes_nothing`).

## Relación con Agenda

- `sessions.appointment_id` es opcional y `ON DELETE SET NULL`: cancelar o
  archivar una cita nunca borra ni huérfana de forma destructiva la sesión
  ya creada a partir de ella.
- La coherencia paciente-cita-sesión se valida en la creación (ver arriba),
  no después — no puede existir una sesión cuyo paciente no coincida con el
  de su cita.
- Una cita de "bloqueo personal" (sin paciente) nunca ofrece crear una
  sesión: la fila "Sesión clínica" completa no se renderiza en
  `AppointmentDetailScreen` cuando `appointment.patientId` es nulo.

## Privacidad

- **Ningún contenido clínico via `location.state` ni en la URL.** El único
  dato que viaja por query string es `appointmentId` (un UUID de
  programación, no clínico). El contenido de la nota siempre se pide fresco
  por IPC tras montar la pantalla.
- **Google Calendar no se toca en esta fase.** La sincronización sigue
  minimizada exactamente como en Fases 2–3 (solo horario) — sesiones y
  notas no le agregan ningún campo nuevo, ni se lee su contenido para
  enviarlo a Google.
- **Sin contenido clínico en logs.** `SessionError` nunca interpola el
  contenido de una nota ni el error crudo de `rusqlite` en sus variantes
  visibles — mismo patrón que `PatientError` en la Fase 1.5.
- **Sin IA ni servicios externos.** Ninguna función de esta fase envía,
  procesa o resume contenido de notas fuera de SQLCipher local.
- Auditoría manual realizada con una cadena marcador ficticia
  (`XYZFASE4TEST`) sembrada en contenido de notas de prueba: no aparece en
  `WebKitCache`, `CacheStorage`, `storage`, `hsts-storage.sqlite`, ni en el
  log propio de la aplicación (`logs/Cuaderno Clínico.log`) — solo dentro
  del vault cifrado.

## Archivado y restauración

Igual patrón que pacientes y citas (soft delete real, sin `hard_delete` en
ningún punto del código):

- `archive_session` fija `deleted_at`; la sesión desaparece del listado
  "Activas" pero sigue completa (con toda su nota e historial) en
  "Archivadas".
- `restore_session` revierte `deleted_at` a `NULL`.
- Archivar una sesión **no** archiva ni bloquea su nota — el contenido y el
  historial de versiones permanecen accesibles y sin cambios mientras está
  archivada.

## Decisiones de negocio tomadas en esta fase

1. **Qué cuenta como "contenido no vacío" al cerrar.** No especificado de
   forma exhaustiva en la aprobación más allá de "texto no vacío después de
   trim" — se decidió que basta con que **uno solo** de los cuatro campos
   (contenido, intervenciones, tareas, foco de la próxima sesión) tenga
   contenido real. Test:
   `closing_a_note_with_content_in_any_single_field_succeeds`.
2. **Metadatos de sesión inmutables tras la creación.** `patient_id` y
   `appointment_id` no forman parte de `SessionMetadataInput` — una vez
   creada la sesión, esos dos campos no se pueden reasignar desde la UI.
   Decisión interna de implementación (regla 37 de la aprobación), no un
   cambio de arquitectura.
3. **"Una sesión por cita" se aplica también a sesiones archivadas.**
   Interpretación conservadora: archivar la sesión de una cita no libera esa
   cita para crear una segunda sesión. Test:
   `rejects_a_second_session_even_if_the_first_was_archived`.
4. **Flujo B con cita archivada.** "Ver sesión" se muestra siempre que exista
   la sesión (es solo un enlace de lectura); "Iniciar sesión" (creación) se
   oculta si la cita está archivada.

## Exclusiones explícitas de esta fase

Ninguno de estos puntos se tocó, tal como exigía la aprobación:

- `docs/SCHEMA_V1.md` y `src-tauri/src/db/migrations.rs` — sin migraciones
  nuevas; el esquema de `sessions`/`session_notes` ya existía desde 1.3.
- `src-tauri/src/security/*` — sin cambios al modelo de cifrado, vault ni
  sesión.
- `src-tauri/src/calendar/*` — Google Calendar sin cambios.
- `src-tauri/src/db/connection.rs` — sin tocar (no fue necesario: WAL y
  demás pragmas ya cubrían las necesidades de esta fase).
- Colores/tokens de diseño — ningún color nuevo; se reutilizan
  `bg-success-soft`/`text-success` (vigente/cerrada), `bg-warning-soft`
  (borrador) y el acento `#2D5128` ya existentes.
- Formulación clínica, objetivos, evaluaciones, documentos, pagos, línea
  temporal — siguen mostrando "Próximamente"; solo "Sesiones" pasó a tener
  contenido real (`SECTIONS_WITH_REAL_CONTENT` ahora es
  `['resumen', 'sesiones']`).

## Archivos creados o modificados

| Archivo | Rol |
|---|---|
| `src-tauri/src/repositories/sessions.rs` (nuevo) | SQL puro sobre `sessions`: `insert`, `find_by_id`, `find_by_appointment_id`, `list_active_by_patient`, `list_deleted_by_patient`, `update_metadata`, `soft_delete`, `restore`. |
| `src-tauri/src/repositories/session_notes.rs` (nuevo) | SQL puro sobre `session_notes`, con las tres garantías estructurales del invariante append-only descritas arriba. |
| `src-tauri/src/services/sessions.rs` (nuevo, 904 líneas) | Validación, orquestación, transacciones atómicas (`unchecked_transaction`) y las reglas de versionado. |
| `src-tauri/src/commands/sessions.rs` (nuevo) | 13 comandos Tauri, todos mediados por `VaultSession::with_connection`. |
| `src-tauri/src/repositories/mod.rs`, `services/mod.rs`, `commands/mod.rs`, `lib.rs` | Registro de los nuevos módulos y comandos. |
| `src/components/ui/Textarea.tsx` (nuevo) | Primitivo de formulario para campos multilínea, mismo patrón que `TextField`. |
| `src/features/sessions/*` (nuevo) | `types.ts`, `api.ts`, `schema.ts` (Zod), `datetime.ts`, `SessionsTab.tsx`, `SessionCreateScreen.tsx`, `SessionDetailScreen.tsx`. |
| `src/features/patients/PatientDetailScreen.tsx` | Pestaña "Sesiones" ahora renderiza `SessionsTab` en vez de "Próximamente". |
| `src/features/agenda/AppointmentDetailScreen.tsx` | Fila "Sesión clínica" con Flujo B ("Iniciar sesión" / "Ver sesión" / oculto en bloqueos personales). |
| `src/App.tsx` | Rutas `/patients/:patientId/sessions/new` y `/patients/:patientId/sessions/:sessionId`. |

## Tests ejecutados

`cargo test` en `src-tauri/`: **200/200 en verde** (172 de las Fases 1–3 sin
cambios + 28 nuevos en `services::sessions` + 6 nuevos en
`repositories::sessions` + 9 nuevos en `repositories::session_notes`, más
los ajustes de conteo entre repositorios/servicios). `cargo clippy
--all-targets`: sin advertencias. `npm run build`: sin errores. `npm run
lint`: sin errores (los `warning` de `oxlint` sobre `set-state-in-effect` en
`SessionsTab.tsx` siguen exactamente el mismo patrón ya presente en
`PatientsListScreen.tsx`, `AgendaScreen.tsx` y `SettingsScreen.tsx` desde
fases anteriores — no es una regresión de esta fase). `cargo build`: sin
errores.

Tests representativos del invariante append-only:

| Requisito | Test |
|---|---|
| Actualizar contenido de una nota cerrada no hace nada | `repositories::session_notes::updating_a_locked_note_directly_changes_nothing` |
| SQLite rechaza dos versiones vigentes para la misma sesión | `repositories::session_notes::the_database_itself_rejects_two_current_versions_for_the_same_session` |
| Editar una nota cerrada preserva la anterior intacta | `services::sessions::editing_a_closed_note_creates_a_new_version_and_leaves_the_previous_one_intact` |
| Tres versiones consecutivas se preservan correctamente | `services::sessions::three_consecutive_versions_are_all_preserved_correctly` |
| Cerrar nota vacía se rechaza | `services::sessions::closing_an_empty_note_is_rejected_and_changes_nothing` |
| Cerrar nota ya cerrada es idempotente | `services::sessions::closing_an_already_closed_note_is_idempotent` |
| Autoguardado nunca bloquea el borrador | `services::sessions::autosave_writes_to_the_draft_and_never_locks_it` |
| Autoguardar sobre una nota cerrada se rechaza | `services::sessions::autosaving_a_locked_note_is_rejected` |
| Sesión + primera nota se crean atómicamente | `services::sessions::creates_a_session_with_its_first_note_version_atomically` |
| Flujo B hereda el paciente de la cita | `services::sessions::creates_a_session_from_an_appointment_inheriting_its_patient` |
| Coherencia paciente-cita | `services::sessions::rejects_a_session_whose_patient_does_not_match_the_appointments_patient` |
| No se puede crear una segunda sesión para la misma cita | `services::sessions::rejects_a_second_session_for_the_same_appointment` |
| Tampoco si la primera está archivada | `services::sessions::rejects_a_second_session_even_if_the_first_was_archived` |
| Archivar preserva la nota intacta | `services::sessions::archiving_hides_from_active_listing_but_keeps_notes_intact` |
| Restaurar vuelve al listado activo | `services::sessions::restoring_brings_it_back_to_the_active_listing` |

## Prueba manual realizada (aplicación real, no solo tests)

Compilada con `cargo build`, ejecutada bajo Xvfb con `xdotool` (clics y
tecleo reales) sobre un vault de prueba desechable (creado, usado y
eliminado en esta sesión — nunca se tocó el vault real), con capturas de
pantalla en cada paso:

1. Crear vault de prueba → desbloquear → crear paciente ficticio ("Paciente
   de Prueba Fase 4").
2. **Flujo A**: pestaña "Sesiones" → "Nueva sesión" → formulario completo
   (fecha 09/01/2026, hora 03:00 PM, duración 50, modalidad presencial) →
   sesión creada con su primera nota en borrador.
3. Escribir contenido con el marcador ficticio `XYZFASE4TEST` → autoguardado
   confirmado ("Guardado") → "Cerrar nota" → nota pasa a "Cerrada (versión
   1)".
4. "Editar" (con diálogo de confirmación) → versión 2 precargada con el
   contenido exacto de la versión 1 → se edita (se agrega texto) → se
   autoguarda → se cierra → "Cerrada (versión 2)".
5. Repetido una vez más → versión 3 creada, editada, cerrada.
6. "Ver historial" → las tres versiones visibles: v3 "Vigente" con su
   contenido editado, v2 y v1 "Reemplazada el 01-09-2026" cada una con su
   contenido **original, sin ninguna de las ediciones posteriores** —
   confirma en la UI real, no solo en tests, que el historial es append-only
   de verdad.
7. Archivar la sesión → desaparece de "Activas", aparece en "Archivadas"
   con el formulario de metadatos deshabilitado y la nota intacta.
8. Restaurar → vuelve a "Activas" con todo intacto.
9. **Flujo B**: crear una cita en Agenda con el mismo paciente → fila
   "Sesión clínica" muestra "Iniciar sesión" → clic → formulario precargado
   con fecha/hora/duración/modalidad exactas de la cita → sesión creada →
   volver a la cita → la fila ahora muestra "Ver sesión" (no vuelve a
   ofrecer "Iniciar sesión" para la misma cita).
10. Crear una cita de "bloqueo personal" (sin paciente) → la fila "Sesión
    clínica" no aparece en absoluto.
11. **Persistencia a través de bloqueo/desbloqueo del vault**: con una nota
    en borrador y contenido sin cerrar, bloquear el vault (botón
    "Bloquear") → desbloquear con la misma contraseña → el borrador sigue
    exactamente igual, sin pérdida.
12. **Cierre completo del proceso de la aplicación y reapertura real** (no
    solo bloquear): matar el proceso, relanzar el binario → arranca en
    estado `Locked` → desbloquear → las dos sesiones, su historial completo
    de 3 versiones, y la cita de bloqueo personal, todo persistido
    correctamente.
13. **Auditoría de privacidad**: búsqueda del marcador `XYZFASE4TEST` en
    `WebKitCache`, `CacheStorage`, `storage`, `hsts-storage.sqlite` y el log
    de la aplicación — cero coincidencias en todos ellos. Única
    coincidencia en el sistema completo: el log propio de la herramienta
    externa usada para automatizar el test (no un archivo de la aplicación,
    no forma parte de su perímetro de datos).
14. **Regresión de Fases 1–3**: Dashboard (Inicio), listado de Pacientes,
    Agenda, y Ajustes revisados visualmente tras el reinicio completo — sin
    cambios de comportamiento respecto a fases anteriores.
15. Limpieza: proceso de la aplicación de prueba detenido, vault de prueba
    eliminado, vault real restaurado exactamente como estaba antes de
    empezar.

## Limitaciones y decisiones que quedan pendientes de aprobación

Ninguna. Todas las decisiones de esta fase estaban resueltas de forma
definitiva en la aprobación formal (seguridad de la sección 23 de la
auditoría previa) o son decisiones internas de implementación sin impacto
arquitectónico (ver "Decisiones de negocio tomadas en esta fase" arriba).
