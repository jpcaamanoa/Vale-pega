# Antecedentes clínicos (Fase 6)

Documento técnico de la Fase 6. Complementa `docs/ARCHITECTURE.md` (secciones 4 y 13.D) y
`docs/goals.md` (Fase 5, cuyo patrón de capas se reutiliza sin cambios). Cubre el cuarto vertical
funcional completo del cuaderno clínico: registrar los antecedentes clínicos de un paciente
(motivo de consulta, diagnóstico, notas diagnósticas, factores de riesgo, notas médicas
relevantes).

## Propósito

Antes de esta fase, "Antecedentes" era una pestaña de la ficha del paciente que mostraba
"Próximamente". Esta fase la reemplaza por contenido real: crear, consultar y editar un único
registro de antecedentes clínicos por paciente.

## Alcance

Dentro de esta fase: `patient_clinical_profile` (obtener/crear/actualizar, un registro por
paciente), integración con la pestaña "Antecedentes" de `PatientDetailScreen` (Fase 1.5).

Fuera de alcance (deliberadamente, ver aprobación de Fase 6): versionado o historial de
antecedentes, catálogo o taxonomía clínica de factores de riesgo, cálculos o interpretación
automática de `risk_flags`, Formulación, Evaluaciones, Documentos, Pagos, Biblioteca,
Herramientas, Recordatorios, IA, búsqueda global, backup/export, modo privacidad, WAL, ajustes
generales, cambios a Dashboard, React Flow, Recharts.

## Modelo de datos usado

Exactamente el de `SCHEMA_V1` (Fase 1.3) — **sin migraciones nuevas**. Una tabla:

```sql
CREATE TABLE patient_clinical_profile (
  patient_id TEXT PRIMARY KEY REFERENCES patients(id) ON DELETE RESTRICT,
  presenting_problem TEXT,
  primary_diagnosis_code TEXT,
  diagnosis_notes TEXT,
  risk_flags TEXT,
  relevant_medical_notes TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TRIGGER trg_patient_clinical_profile_touch_updated_at
AFTER UPDATE ON patient_clinical_profile
WHEN NEW.updated_at = OLD.updated_at
BEGIN
  UPDATE patient_clinical_profile SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
    WHERE patient_id = NEW.patient_id;
END;
```

Dos particularidades del esquema que la implementación respeta tal cual, sin intentar
"corregirlas":

- **`patient_id` es la propia `PRIMARY KEY` de la tabla.** No hay `id` surrogate ni columna
  `created_at`. Esto, por sí solo, garantiza a nivel de base de datos que existe como máximo un
  registro por paciente — sin necesidad de un índice único adicional.
- **El trigger `AFTER UPDATE` solo toca `updated_at` cuando la propia sentencia no lo cambió.**
  El repositorio nunca necesita fijar `updated_at` manualmente en un `UPDATE`.

## Arquitectura

Misma capa que Sesiones (Fase 4) y Objetivos (Fase 5):

```
React (features/clinical-profile/*)
   │  invoke('get_clinical_profile', { patientId }), invoke('create_clinical_profile', ...), etc.
   ▼
commands::patient_clinical_profile   (src-tauri/src/commands/patient_clinical_profile.rs, 3 comandos)
   │  — capa fina. Solo obtiene la conexión y delega.
   ▼
security::session::VaultSession::with_connection
   │  — vault bloqueado ⇒ Err antes de llegar a services/repositories.
   ▼
services::patient_clinical_profile   (src-tauri/src/services/patient_clinical_profile.rs)
   │  — reglas de negocio: paciente existe, un perfil por paciente, paciente no archivado
   │    (solo para creación), validación de JSON sintáctico de `risk_flags`.
   ▼
repositories::patient_clinical_profile
   │  — SQL puro sobre `patient_clinical_profile`.
   ▼
SQLite + SQLCipher (vault.db, sin migraciones nuevas)
```

## Registro mutable simple, sin versionado

A diferencia de `session_notes` (Fase 4), el perfil clínico es un **registro mutable simple** —
editarlo ejecuta un `UPDATE` directo sobre la misma fila, igual que editar un paciente o un
objetivo terapéutico (Fase 5). No hay `version`, `version_number`, `is_current`, `is_locked`,
`closed_at`, `superseded_at`, snapshots, tabla de historial, ni ninguna forma de auditoría
histórica del contenido. Esta es una decisión de producto explícita y deliberada de la aprobación
de Fase 6, no una omisión: mantiene consistencia con el patrón de registros mutables ya usado en
el proyecto (pacientes, objetivos) y evita introducir en esta fase una arquitectura de versionado
que `SCHEMA_V1` no define.

`create_clinical_profile` y `update_clinical_profile` son operaciones separadas, no un upsert
implícito: crear sobre un paciente que ya tiene perfil devuelve `AlreadyExists`; editar un perfil
que todavía no existe devuelve `NotFound`. Esto refleja directamente los tres estados de la UI
(sin registro / crear / editar) sin ambigüedad sobre cuál operación corresponde a cada uno.

## `risk_flags`: JSON sin taxonomía clínica

`risk_flags` es, en el esquema, `TEXT` que contiene JSON — sin cambios respecto a Fase 1.3. Esta
fase **no** introduce un catálogo clínico de factores de riesgo, no interpreta el contenido, y no
deriva ningún diagnóstico o alerta automática a partir de él. La única validación, tanto en
frontend (Zod) como en backend (`services::patient_clinical_profile::validate_risk_flags`,
autoritativo), es que el campo esté vacío o contenga JSON sintácticamente válido — nunca se
valida ni se asume una forma específica (objeto, lista, etc.). La UI lo presenta como un campo de
texto libre con la instrucción explícita de qué formato se espera, sin ningún selector,
categoría, ni semántica clínica predefinida — exactamente lo que pedía la aprobación de Fase 6
("no inventes una taxonomía médica... la prioridad es no inventar semántica clínica").

## Un registro por paciente y aislamiento entre pacientes

Toda operación recibe `patient_id` explícitamente y se valida en la capa de servicio:

- `get_clinical_profile`/`create_clinical_profile`/`update_clinical_profile` verifican primero
  que el paciente existe (`PatientNotFound` si no) — antes de tocar `patient_clinical_profile`.
- No existe ninguna operación que permita leer o modificar el perfil de un paciente usando el
  `patient_id` de otro: la cláusula `WHERE patient_id = ?` en cada consulta/actualización, más la
  verificación explícita de existencia del paciente, hacen estructuralmente imposible el acceso
  cruzado. Cubierto por
  `a_patients_profile_cannot_be_read_through_another_patients_id` y
  `updating_with_another_patients_id_never_touches_the_first_patients_profile`.
- `patient_clinical_profile.patient_id` es a la vez clave primaria y `FOREIGN KEY ... ON DELETE
  RESTRICT` — la propia base de datos garantiza que nunca puede existir un perfil sin paciente y
  que nunca puede haber dos perfiles para el mismo paciente.

## Pacientes archivados

- No se pueden crear antecedentes nuevos para un paciente archivado (`create_clinical_profile`
  revisa `patient.deleted_at`), mismo criterio que `create_goal` (Fase 5) y `create_session`
  (Fase 4).
- Sí se pueden editar antecedentes **ya existentes** de un paciente archivado
  (`update_clinical_profile` no revisa el estado del paciente) — mismo criterio que editar
  indicadores u objetivos ya existentes: archivar no bloquea la edición de datos ya registrados,
  solo la creación de datos nuevos.
- En la interfaz, el botón "Agregar antecedentes" del estado vacío se oculta para un paciente
  archivado sin perfil todavía (`canCreate = !patientArchived` en `ClinicalProfileTab`) — la
  creación queda bloqueada tanto en frontend como en backend.

## Privacidad

- **IPC mínimo por construcción.** El único dato que viaja es exactamente lo necesario para
  mostrar/editar el perfil (`ClinicalProfile`, cinco campos de contenido más `patientId` y
  `updatedAt`) — no hay ningún listado ni vista resumida que exponga contenido clínico de forma
  incidental, porque no existe listado: es un único registro por paciente, consultado siempre
  por `patient_id` explícito.
- **Sin antecedentes en Google Calendar.** El módulo `calendar` no referencia
  `patient_clinical_profile` en ningún punto — verificado por inspección directa del código
  (`grep -rni "clinical_profile\|antecedentes\|presenting_problem\|risk_flags"
  src-tauri/src/calendar/`: cero coincidencias).
- **Sin contenido clínico en `location.state`, logs, `localStorage`/`sessionStorage`, ni título
  de ventana.** Mismas garantías estructurales que Fases 4 y 5 — la pestaña "Antecedentes" no usa
  navegación con estado ni almacenamiento del navegador; los mensajes de error mostrados en la UI
  son siempre los mensajes de dominio de `ClinicalProfileError` (nunca el `Debug` crudo de un
  error de `SQLite`).
- Auditoría manual realizada con una cadena marcador ficticia (`XYZFASE6ANTECEDENTES`) sembrada
  en los cinco campos de contenido de dos pacientes de prueba: no aparece en `WebKitCache`,
  `CacheStorage`, `storage`, `hsts-storage.sqlite`, `mediakeys`, ni en el log propio de la
  aplicación — solo dentro del `vault.db` cifrado (confirmado que tampoco aparece en texto plano
  dentro del propio archivo `vault.db`, como corresponde a SQLCipher).

## Decisiones de negocio tomadas en esta fase

1. **Antecedentes clínicos son un registro mutable simple, sin versionado.** Ver sección
   dedicada arriba — decisión explícita de la aprobación de Fase 6, deliberadamente distinta de
   `session_notes`.
2. **Creación y edición son operaciones separadas, no un upsert.** Crear sobre un perfil
   existente falla con `AlreadyExists`; editar un perfil inexistente falla con `NotFound`. Reflejo
   directo de los estados de la UI (vacío/crear vs. existente/editar).
3. **`risk_flags` se trata únicamente como JSON de sintaxis válida, sin taxonomía ni selector.**
   Ver sección dedicada arriba — decisión explícita para no inventar semántica clínica no
   definida por el proyecto.
4. **La creación se bloquea para un paciente archivado; la edición de un perfil existente no.**
   Mismo criterio ya establecido para objetivos e indicadores en Fase 5.
5. **Campos de texto siempre opcionales, sin validaciones clínicas artificiales.** Ni el
   frontend ni el backend exigen ningún campo — un perfil completamente vacío es válido y se
   puede guardar (verificado manualmente y con
   `creates_a_profile_with_all_fields_empty`/`inserts_a_profile_with_all_fields_empty`).

## Exclusiones explícitas de esta fase

Ninguno de estos puntos se tocó, tal como exigía la aprobación:

- Versionado, historial, snapshots, o cualquier forma de auditoría del contenido de
  `patient_clinical_profile` — el registro es mutable sin excepción.
- Ningún catálogo clínico de factores de riesgo, ninguna taxonomía diagnóstica, ningún cálculo o
  interpretación automática a partir de `risk_flags`.
- `docs/SCHEMA_V1.md` y `src-tauri/src/db/migrations.rs` — sin migraciones nuevas, sin tocar
  `SCHEMA_V1`.
- `src-tauri/src/security/*`, `src-tauri/src/calendar/*`, `src-tauri/src/db/connection.rs`,
  `src-tauri/src/repositories/session_notes.rs`, `src-tauri/src/services/sessions.rs` — sin
  tocar.
- `therapeutic_goals`, `goal_indicators`, `session_goals` (Fase 5) — sin modificar.
- Formulación, Evaluaciones, Documentos, Pagos, Biblioteca, Herramientas, Recordatorios,
  Ajustes generales, Backup, Export, Privacy Mode, búsqueda global, WAL, Biometría,
  FileVault/BitLocker — sin implementar.
- Ninguna dependencia nueva — todo el frontend reutiliza `Button`, `TextField`, `Textarea`, Zod,
  `react-hook-form`, ya presentes desde fases anteriores. Ni React Flow ni Recharts se instalaron.
- Ni `package.json` ni `Cargo.toml`/`Cargo.lock` se modificaron.

## Archivos creados o modificados

| Archivo | Rol |
|---|---|
| `src-tauri/src/repositories/patient_clinical_profile.rs` (nuevo) | SQL puro sobre `patient_clinical_profile`: `find_by_patient_id`, `insert`, `update`. |
| `src-tauri/src/services/patient_clinical_profile.rs` (nuevo) | Validación (paciente existe, un perfil por paciente, paciente no archivado para creación, JSON válido en `risk_flags`) y orquestación. |
| `src-tauri/src/commands/patient_clinical_profile.rs` (nuevo) | 3 comandos Tauri, mediados por `VaultSession::with_connection`. |
| `src-tauri/src/repositories/mod.rs`, `services/mod.rs`, `commands/mod.rs`, `lib.rs` | Registro de los nuevos módulos y comandos. |
| `src/features/clinical-profile/*` (nuevo) | `types.ts`, `api.ts`, `schema.ts`, `ClinicalProfileTab.tsx`. |
| `src/features/patients/PatientDetailScreen.tsx` | Pestaña "Antecedentes" ahora renderiza `ClinicalProfileTab` en vez de "Próximamente". |

## Tests ejecutados

`cargo test` en `src-tauri/`: **271/271 en verde** (247 previos sin cambios + 24 nuevos: 8 en
`repositories::patient_clinical_profile`, 16 en `services::patient_clinical_profile`).
`cargo clippy --all-targets`: sin advertencias. `npm run build`: sin errores. `npm run lint`:
exit 0, 14 warnings (13 preexistentes de Fase 5 — verificados empíricamente contra el baseline
real vía un worktree del commit `1eb5e31` — más 1 nuevo en `ClinicalProfileTab.tsx`, de la misma
categoría preexistente `set-state-in-effect` que ya afecta a `GoalsTab.tsx`, `SessionsTab.tsx`,
`PatientsListScreen.tsx`, etc. — no se introduce ninguna categoría nueva). `cargo build`: sin
errores.

Tests representativos de las reglas de negocio:

| Requisito | Test |
|---|---|
| Un paciente sin perfil devuelve `None`, no error | `services::patient_clinical_profile::getting_profile_of_a_patient_without_one_returns_none` |
| Paciente inexistente rechazado en obtener/crear/editar | `getting_profile_of_a_nonexistent_patient_is_rejected`, `rejects_creation_for_a_nonexistent_patient`, `updating_for_a_nonexistent_patient_is_rejected` |
| No se puede crear un segundo perfil para el mismo paciente | `rejects_creating_a_second_profile_for_the_same_patient` |
| Creación bloqueada para paciente archivado | `rejects_creation_for_an_archived_patient` |
| Edición permitida para paciente archivado (perfil ya existente) | `updating_an_archived_patients_profile_is_allowed` |
| `risk_flags` con JSON inválido rechazado (crear y editar) | `rejects_invalid_json_in_risk_flags_on_create`, `rejects_invalid_json_in_risk_flags_on_update` |
| `risk_flags` vacío/blanco tratado como ausente | `accepts_blank_risk_flags_as_absent` |
| Todos los campos pueden quedar vacíos | `creates_a_profile_with_all_fields_empty` |
| Editar un perfil inexistente falla con `NotFound` | `updating_a_profile_that_does_not_exist_yet_is_rejected` |
| Aislamiento: el perfil de un paciente no se puede leer con el `patient_id` de otro | `a_patients_profile_cannot_be_read_through_another_patients_id` |
| Aislamiento: "editar" con el `patient_id` de otro nunca toca el perfil real | `updating_with_another_patients_id_never_touches_the_first_patients_profile` |
| `PRIMARY KEY` de la tabla rechaza un segundo `INSERT` para el mismo paciente | `repositories::patient_clinical_profile::a_second_insert_for_the_same_patient_violates_the_primary_key` |
| `UPDATE` reemplaza todos los campos, incluido vaciar uno ya definido | `repositories::patient_clinical_profile::update_replaces_all_fields`, `update_can_clear_a_previously_set_field` |

## Prueba manual realizada (aplicación real, no solo tests)

Compilada con `cargo build`, ejecutada bajo Xvfb con `xdotool` (clics y tecleo reales) sobre **dos
vaults de prueba desechables** (creados, usados y eliminados en esta sesión — nunca se tocó el
vault real; restauración verificada byte a byte por tamaño y fecha de modificación exactos), con
capturas de pantalla en cada paso:

**Primer vault — enfocado en la vertical nueva:**

1. Crear vault de prueba → desbloquear → crear "Paciente Prueba A XYZFASE6ANTECEDENTES".
2. Pestaña "Antecedentes" deja de mostrar "Próximamente" — confirmado con el empty state real
   ("No hay antecedentes clínicos registrados." + botón "Agregar antecedentes").
3. Crear antecedentes completando los cinco campos (incluido `risk_flags` con JSON válido,
   marcador ficticio incluido en todos los campos) → "Guardado." → todos los valores mostrados
   correctamente en la vista de lectura.
4. Navegar a otra pestaña y volver → persistencia confirmada sin recarga completa.
5. "Editar" → formulario prellenado con los valores existentes → editar el motivo de consulta →
   "Guardar cambios" → cambio reflejado inmediatamente.
6. **Lock/unlock**: bloquear el vault → desbloquear con la misma contraseña → antecedentes
   confirmados exactamente iguales.
7. **Cierre completo del proceso y reapertura real** (no solo bloquear): matar el proceso,
   relanzar el binario → arranca pidiendo contraseña → desbloquear → antecedentes del paciente A
   confirmados intactos.
8. Crear "Paciente Prueba B XYZFASE6ANTECEDENTES" → su pestaña "Antecedentes" muestra el empty
   state correcto (sin ningún dato del paciente A) — aislamiento confirmado visualmente además
   de por los tests automatizados.
9. Validación de `risk_flags`: introducir JSON inválido (`{esto no es json valido`) → mensaje de
   error claro sin exponer detalles técnicos, envío bloqueado → corregir a vacío → guardado
   exitoso de un perfil con **todos los campos vacíos**.
10. Crear "Paciente Prueba C XYZFASE6ANTECEDENTES" → archivarlo inmediatamente (sin antecedentes
    todavía) → pestaña "Antecedentes" muestra el empty state **sin** el botón "Agregar
    antecedentes" — la creación queda bloqueada en la UI para un paciente archivado, tal como
    exige la regla de negocio.
11. **Auditoría de privacidad**: búsqueda del marcador `XYZFASE6ANTECEDENTES` en `WebKitCache`,
    `CacheStorage`, `storage`, `hsts-storage.sqlite`, `mediakeys`, el log de la aplicación, y
    dentro del propio `vault.db` en texto plano — cero coincidencias en todos. Búsquedas
    adicionales de `localStorage`/`sessionStorage`, `location.state`, `println!`/`dbg!`/
    `eprintln!` y macros `log::*` en el código nuevo — sin hallazgos.

**Segundo vault — regresión funcional de Fases 1–5** (además de la suite automatizada, que ya
cubre las 247 pruebas previas sin cambios):

12. Dashboard ("Inicio") carga sin errores, sin datos inventados (0 pacientes activos al
    empezar).
13. Crear paciente, sesión (Fase 4) y su nota clínica — versión 1 creada, cerrada ("Cerrada
    (versión 1)"), editada — versión 2 creada ("Borrador (versión 2)", prellenada con el
    contenido anterior) → "Ver historial" confirma **ambas versiones intactas** ("Versión 2 ·
    Vigente" y "Versión 1 · Reemplazada el [fecha]") — modelo append-only de Fase 4 sin
    regresión.
14. Crear un objetivo terapéutico (Fase 5) para el mismo paciente → estado `Activo` por defecto,
    sección de indicadores presente.
15. Desde la sesión, "Agregar objetivo" → selector ofrece correctamente el objetivo del paciente
    → vincular → objetivo visible en "Objetivos trabajados en esta sesión" con "Sin progreso
    registrado." — vínculo sesión↔objetivo de Fase 5 sin regresión.
16. Agenda carga sin errores ("No hay citas en este rango.").
17. Auditoría de privacidad del marcador de esta segunda prueba — sin coincidencias fuera del
    vault cifrado.
18. Limpieza: procesos de la aplicación de prueba y servidor de desarrollo de Vite detenidos,
    ambos vaults de prueba eliminados, vault real restaurado exactamente como estaba antes de
    empezar (mismo tamaño y fecha de modificación verificados byte a byte en ambas ocasiones).

## Limitaciones y decisiones que quedan pendientes de aprobación

Ninguna. Todas las decisiones de esta fase estaban resueltas de forma definitiva en la aprobación
formal de Fase 6, o son decisiones internas de implementación sin impacto arquitectónico (ver
"Decisiones de negocio tomadas en esta fase" arriba).
