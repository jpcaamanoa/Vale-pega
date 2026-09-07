# Plan de Seguridad Clínico Versionado (Fase 12)

Documento técnico de la Fase 12. Complementa `docs/ARCHITECTURE.md` (tabla de fases) y usa como
referencia conceptual (no como copia literal) el patrón de versionado ya probado en
`docs/episode-closure.md` (Fase 11) y en `session_notes` (Fase 4).

## 1. Objetivo y alcance

El Plan de Seguridad es una **herramienta de documentación clínica estructurada** para registrar,
junto con la persona, un plan de acción frente a momentos de crisis. Existe para que la
profesional y la persona puedan volver a un recurso ya trabajado — nada más.

## 2. Qué NO hace este sistema

Esto no es, y nunca debe convertirse en:

- un predictor de riesgo suicida ni una escala automática;
- un diagnóstico ni un sistema de triage;
- un sistema de vigilancia, alertas automáticas o notificación a terceros;
- un sistema de emergencia ni un sustituto de la atención de urgencia;
- un motor de recomendaciones clínicas ni una IA que interprete el contenido.

La aplicación **registra**; la profesional **decide**. No hay scoring, no hay colores
rojo/amarillo/verde derivados del contenido, no hay geolocalización, no hay contacto automático
(SMS, llamadas, WhatsApp, email) con nadie, y la ausencia de un plan nunca bloquea ningún otro
flujo de la aplicación (crear paciente, sesión, proceso, objetivo, pago, cita, cierre, reingreso).

## 3. Paciente, no proceso terapéutico

`safety_plans.patient_id` es obligatorio; no existe ninguna columna `episode_id`. Un plan de
seguridad tiene sentido clínico incluso con el proceso terapéutico cerrado o durante un reingreso
posterior — cerrar un proceso (`docs/episode-closure.md`) **nunca** elimina, archiva, marca como
obsoleto ni copia el plan. Un reingreso simplemente permite volver a consultar el plan vigente
existente; la profesional decide si conviene actualizarlo, nunca de forma automática.

## 4. Modelo de datos: migración `V6`

Aditiva sobre `SCHEMA_V1`–`V5`, que quedan intactos (verificado por
`fresh_database_is_created_from_migrations_alone_with_all_expected_tables` y por los tests de
migración de esta fase). Sin `DROP`, sin tocar ninguna columna existente.

```sql
CREATE TABLE safety_plans (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  version INTEGER NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('borrador','vigente','reemplazado')),
  warning_signs TEXT,
  internal_strategies TEXT,
  social_support_strategies TEXT,
  means_safety TEXT,
  crisis_steps TEXT,
  notes TEXT,
  reviewed_at TEXT,
  confirmed_at TEXT,
  superseded_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (patient_id, version),
  CHECK (
    (status = 'borrador'    AND confirmed_at IS NULL     AND superseded_at IS NULL)
    OR
    (status = 'vigente'     AND confirmed_at IS NOT NULL AND superseded_at IS NULL)
    OR
    (status = 'reemplazado' AND confirmed_at IS NOT NULL AND superseded_at IS NOT NULL)
  )
);
CREATE INDEX idx_safety_plans_patient ON safety_plans(patient_id);
CREATE UNIQUE INDEX idx_safety_plans_one_current_per_patient
  ON safety_plans(patient_id) WHERE status = 'vigente';
CREATE UNIQUE INDEX idx_safety_plans_one_draft_per_patient
  ON safety_plans(patient_id) WHERE status = 'borrador';

CREATE TABLE safety_plan_contacts (
  id TEXT PRIMARY KEY,
  safety_plan_id TEXT NOT NULL REFERENCES safety_plans(id) ON DELETE CASCADE,
  contact_type TEXT NOT NULL CHECK (contact_type IN ('support_person','professional','service')),
  name TEXT NOT NULL,
  relationship_or_role TEXT,
  phone TEXT,
  notes TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_safety_plan_contacts_plan ON safety_plan_contacts(safety_plan_id, sort_order);
```

### Por qué tabla hija para los contactos

Se evaluaron tres opciones: columnas rígidas (`contact1`, `contact2`, ...), JSON estructurado, y
tabla hija. Se eligió tabla hija porque permite un número arbitrario de contactos, mantiene
estructura consultable, permite orden (`sort_order`) y versiona naturalmente al estar ligada a
`safety_plan_id` — cada versión del plan tiene sus propios contactos, congelados en el momento en
que esa versión se confirmó.

### `ON DELETE`

`safety_plans.patient_id → patients` usa `ON DELETE RESTRICT` (igual que `treatment_episodes` y
`episode_closures`): el historial clínico nunca desaparece como efecto colateral de borrar un
paciente — y, en la práctica, un paciente nunca se borra de verdad, solo se archiva
(`deleted_at`). `safety_plan_contacts.safety_plan_id → safety_plans` sí usa `ON DELETE CASCADE`:
los contactos son un detalle del plan al que pertenecen, sin valor propio fuera de él. Este
`CASCADE` solo puede dispararse cuando un plan se elimina en duro, y eso **solo ocurre para
borradores nunca confirmados** (ver §7) — nunca para un `vigente` o `reemplazado`, que no exponen
ninguna función de borrado.

### Índices de unicidad

Dos índices únicos parciales garantizan, a nivel de base de datos y no solo de servicio:

- como máximo **un** plan `vigente` por paciente (`idx_safety_plans_one_current_per_patient`);
- como máximo **un** `borrador` sin confirmar por paciente a la vez
  (`idx_safety_plans_one_draft_per_patient`), para que dos borradores nunca compitan por
  convertirse en el vigente.

Mismo patrón que `idx_treatment_episodes_one_active_per_patient` (Fase 9) y
`idx_episode_closures_active` (Fase 11).

## 5. Versionado: borrador → vigente → reemplazado

Vocabulario de tres estados (más simple que el `is_current`/`is_locked` de dos flags de
`session_notes`, porque aquí hace falta distinguir explícitamente "todavía no confirmado" de
"histórico"):

- **`borrador`**: editable, nunca es "el plan vigente". Puede guardarse incompleto — ninguna
  sección es obligatoria.
- **`vigente`**: confirmado. Inmutable — ninguna función de este dominio permite un `UPDATE` sobre
  su contenido (reforzado en SQL: `repositories::safety_plans::update_draft` incluye
  `WHERE status = 'borrador'` en el propio `UPDATE`).
- **`reemplazado`**: historia. Igual de inmutable que `vigente`; solo difiere en que ya no es la
  versión activa.

"Actualizar el plan" nunca es un `UPDATE` sobre el vigente: crea un **borrador nuevo**, precargado
con el contenido del vigente (copia explícita, hecha por el frontend al llamar
`create_safety_plan_draft` con esos valores) para facilitar la edición. Al confirmarlo
(`confirm_safety_plan_draft`), dentro de una única transacción:

1. el plan vigente anterior (si existe) se marca `reemplazado` (`superseded_at` = ahora);
2. el borrador se confirma como `vigente` (`confirmed_at` = ahora).

Nunca hay un instante persistente con dos vigentes simultáneos — el orden importa (superseder
antes de confirmar) y el índice único parcial es la garantía de última instancia si algo fallara.

## 6. Borrador

Un plan incompleto puede guardarse como borrador en cualquier momento (`update_safety_plan_draft`)
sin obligar a completar ninguna sección. La UI ofrece dos acciones explícitas y distintas:
"Guardar borrador" (persiste sin confirmar) y "Guardar como plan vigente" (persiste y confirma en
un solo paso).

## 7. Eliminación

Nunca hay eliminación (ni física ni lógica) de un plan `vigente` o `reemplazado` — ninguna función
del dominio lo permite. Un **borrador nunca confirmado** sí puede descartarse
(`discard_safety_plan_draft`): eliminación física real, la única permitida, porque ese contenido
nunca llegó a ser un registro clínico confirmado. Sus contactos se eliminan en cascada. El número
de versión de un borrador descartado queda libre para el siguiente borrador (a diferencia de un
plan reemplazado, cuya versión queda fija para siempre en el historial).

## 8. Campos clínicos

| Campo (DB / API)                              | Pregunta conceptual                                                      |
| ---------------------------------------------- | ------------------------------------------------------------------------ |
| `warning_signs` / `warningSigns`               | ¿Cómo noto que estoy empezando a estar peor?                              |
| `internal_strategies` / `internalStrategies`   | Estrategias personales que puedo intentar por mí misma/o                 |
| `social_support_strategies` / `socialSupportStrategies` | Personas o lugares que me ayudan a acompañarme o distraerme     |
| `means_safety` / `meansSafety`                 | Acciones para aumentar la seguridad del entorno / reducir acceso a medios |
| `crisis_steps` / `crisisSteps`                 | Pasos acordados frente a una crisis                                      |
| `notes` / `notes`                              | Observaciones adicionales (opcional)                                     |
| `reviewed_at` / `reviewedAt`                   | Fecha de la última revisión del plan con la persona                      |

Todos los campos narrativos son texto libre y opcionales; ninguno se interpreta, puntúa ni analiza
automáticamente.

## 9. Contactos

`safety_plan_contacts` permite un número arbitrario de personas de apoyo, profesionales o
servicios (`contact_type`: `support_person` / `professional` / `service`), cada uno con nombre
(obligatorio), relación o rol, teléfono y notas — solo gestionables mientras el plan es
`borrador`; una vez confirmado, el contacto queda congelado como parte de esa versión.

### Relación con el contacto de emergencia del paciente

`patients.emergency_contact_*` (Fase 1) **no se duplica automáticamente**. Fase 12 no implementó
ningún atajo de "usar el contacto de emergencia de la ficha" — se evaluó como mejora opcional
(§19 de la aprobación) y se decidió no construirlo todavía, para no ampliar el alcance más allá de
lo pedido; queda como posible mejora de UX en una fase futura. Si se agrega, debe copiarse como
snapshot de texto en el momento de crearse el contacto del plan, nunca como referencia viva: un
cambio posterior en el contacto de emergencia del paciente no debe alterar retroactivamente ningún
plan ya confirmado. Esta regla ya se cumple estructuralmente hoy: `safety_plan_contacts` no lleva
ninguna clave foránea hacia los campos de contacto de emergencia de `patients`.

## 10. `risk_flags` (antecedentes clínicos)

`patient_clinical_profile.risk_flags` (Fase 6, JSON libre validado solo sintácticamente) y el Plan
de Seguridad son conceptos distintos, sin ninguna relación de código entre sí:

- `risk_flags` = información clínica relevante registrada en antecedentes.
- Plan de Seguridad = recurso de acción trabajado clínicamente con la persona.

Ninguna función crea una `risk_flag` porque exista un plan, ni crea un plan porque exista una
`risk_flag`. `services::safety_plans` no importa nada de `services::patient_clinical_profile`, y
viceversa.

## 11. Procesos cerrados y reingreso

Cerrar un proceso terapéutico (`services::episode_closures::close_episode`) no toca
`safety_plans` en absoluto — no hay ninguna llamada cruzada entre ambos módulos. El plan vigente
sigue siendo consultable durante todo el ciclo de vida del paciente, con el proceso abierto,
pausado, cerrado, o durante un reingreso posterior.

## 12. Paciente archivado

Un paciente archivado (`patients.deleted_at IS NOT NULL`) puede seguir **consultando** su plan
vigente y su historial completo, pero:

- no puede **crear** un plan nuevo (`create_draft` rechaza con `PatientArchived`);
- no puede **confirmar** ni **actualizar** un borrador mientras el paciente permanezca archivado
  (el borrador solo puede crearse para un paciente activo, así que este caso solo podría surgir si
  el paciente se archiva con un borrador ya en curso — ver siguiente párrafo).

Restaurar al paciente (`restore_patient`) vuelve a permitir crear planes nuevos de inmediato — la
autoridad vive en `services::safety_plans` (verificada por
`rejects_creating_a_draft_for_an_archived_patient`), nunca solo en React.

Nota de diseño: a diferencia de la creación, `update_draft`/`confirm_draft`/`discard_draft` no
vuelven a comprobar el estado de archivado del paciente en esta fase (mismo criterio ya usado por
`services::patient_clinical_profile::update_clinical_profile`: editar contenido ya iniciado no se
bloquea retroactivamente). Si en la práctica clínica esto resulta indeseable, es una decisión de
producto a revisar explícitamente, no una laguna a corregir en silencio.

## 13. Advertencia clínica

La pestaña "Plan de seguridad" siempre muestra, tanto en la vista principal como en el editor:

> Este plan es un recurso de apoyo elaborado en el contexto de la atención clínica. Cuaderno
> Clínico no es un servicio de emergencia ni sustituye la atención de urgencia.

No se agregan teléfonos automáticamente, no se asume ubicación actual, y `patients.region`/
`commune` (residencia registrada) nunca se usa para inferir servicios de emergencia — eso
requeriría conocer la ubicación real durante una crisis, que la aplicación no tiene ni debe tener.

## 14. Privacidad e IPC

Todo el contenido vive exclusivamente en el vault SQLCipher. Minimización de IPC (§33/§49 de la
aprobación):

- `get_current_safety_plan` / `get_safety_plan_draft`: contenido completo — se piden solo cuando
  la usuaria abre la pestaña "Plan de seguridad" de un paciente concreto.
- `list_safety_plan_history`: devuelve `SafetyPlanSummary` (id, versión, estado, fechas) — **nunca**
  contenido narrativo ni contactos, verificado estructuralmente por el propio tipo Rust (no existe
  ningún campo narrativo en el struct, así que no puede serializarse por error).
- `get_safety_plan_by_id`: contenido completo de una versión histórica concreta — solo se llama
  cuando la usuaria la abre explícitamente desde el historial.

Nada de este dominio se registra en logs, `localStorage`/`sessionStorage`, `document.title`,
nombres de archivo, ni telemetría — no hay ninguna llamada a `console.*` ni a almacenamiento del
navegador en `src/features/safety-plan/`.

## 15. Google Calendar

`src-tauri/src/calendar/*` no se modificó en esta fase. `event_payload(starts_at, ends_at)` sigue
aceptando exclusivamente dos marcas de tiempo y produciendo `{ summary, start, end }` — verificado
por búsqueda estructural (`grep` de `safety_plan`/`warning_signs`/`crisis_steps`/`means_safety`
sobre `src/calendar/`: cero coincidencias) y por el test ya existente
`event_payload_never_contains_anything_beyond_the_generic_summary_and_the_two_timestamps`. El Plan
de Seguridad nunca se sincroniza con Google Calendar.

## 16. Backup / Restore

`src-tauri/src/backup/*` no cambió su lógica de producción. `current_app_schema_version()` calcula
la versión soportada migrando una base en memoria, así que `SCHEMA_V6` se integró automáticamente
sin ningún cambio de código de backup/restore — dos tests de `backup::service` que **hardcodeaban**
el número de esquema anterior (`5`) se actualizaron a `6`, la misma actualización mecánica que ya
ocurrió al pasar de V4 a V5. Verificado con una prueba manual completa: crear paciente ficticio →
crear y confirmar plan v1 → backup → crear y confirmar plan v2 → restaurar → el estado vuelve
exactamente al v1 vigente (v2 deja de existir), con sus contactos y `reviewedAt` intactos.

## 17. Exportación futura

Fase 12 no implementa exportación, PDF, impresión ni compartir. El modelo de datos no impide una
fase futura de exportación: cada versión confirmada es un registro completo y autocontenido.

## 18. Tests

577 → 616 tests de backend (39 nuevos): repositorio (`repositories::safety_plans`, incluye el
struct de resumen `SafetyPlanSummary`), servicio (`services::safety_plans`, incluye reglas de
paciente archivado, versionado, contactos y las dos funciones de minimización IPC) y las
migraciones (`fresh_database_is_created_from_migrations_alone_with_all_expected_tables` ampliado
con las dos tablas nuevas). Ver `Informe-de-cierre-Fase-12-Plan-de-Seguridad.md` para el detalle
completo de resultados.

## 19. Limitaciones conocidas

- No hay atajo de "usar contacto de emergencia de la ficha" (ver §9) — mejora opcional no
  implementada.
- No hay recordatorios automáticos de revisión del plan (`reviewed_at` es puramente informativo).
- No hay exportación/PDF/impresión.
- `update_draft`/`confirm_draft`/`discard_draft` no reevalúan el estado de archivado del paciente
  en cada llamada (ver nota de diseño en §12).
