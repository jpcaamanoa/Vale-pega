# Evaluaciones clínicas / psicométricas (Fase 13)

Documento técnico de la Fase 13. Complementa `docs/ARCHITECTURE.md` (sección 4, "Evaluaciones") y
`Plan-Fase-13-pendiente-de-aprobacion.md` (auditoría de esquema y decisión de migración que
precedió esta fase).

## 1. Objetivo y alcance

Primer vertical productivo sobre `assessment_instruments`/`assessment_administrations`, presentes
sin usar desde `SCHEMA_V1` (Fase 1.3). Permite registrar administraciones de instrumentos
psicométricos — qué se administró, cuándo, un vínculo opcional al proceso terapéutico, puntaje
total, subescalas y una interpretación redactada por la profesional — y consultar la evolución
longitudinal de un mismo instrumento para un paciente a lo largo del tiempo.

## 2. Qué NO hace este sistema (regla de copyright, no negociable)

Cuaderno Clínico **nunca almacena ni reproduce el contenido real de un instrumento**: ni sus
ítems/preguntas, ni sus opciones de respuesta, ni tablas de normas o baremos propietarios. Esto no
es una limitación técnica temporal — es una decisión permanente de diseño, motivada por que la
mayoría de los instrumentos psicométricos de uso clínico están protegidos por derechos de autor y
licenciados por sus editoriales; reproducir su contenido dentro de la aplicación sería una
infracción, independientemente de que el uso sea clínico y no comercial.

En consecuencia:

- El catálogo de instrumentos (`assessment_instruments`) solo guarda **metadatos**: nombre,
  abreviatura, descripción y categoría — todo texto libre escrito por la propia usuaria, nunca el
  contenido protegido del instrumento.
- Una administración (`assessment_administrations`) solo guarda **resultados**: puntaje total,
  subescalas (JSON libre, ej. `{"cognitivo": 10, "somatico": 8}`) e interpretación — siempre
  redactados por la profesional, nunca generados a partir de ítem por ítem.
- Ninguna función de este dominio calcula, normaliza (percentiles, T-scores, Z-scores) ni
  interpreta automáticamente nada. `total_score`, `subscale_scores` e `interpretation_text` son
  siempre lo que la profesional escribió.
- Ninguna vista mezcla instrumentos distintos en una misma evolución: la evolución longitudinal
  siempre es de **un** instrumento a la vez (comparar puntajes de instrumentos distintos como si
  fueran una sola serie sería clínicamente incorrecto, con o sin normalización).

### `raw_responses` — columna legado, deliberadamente sin uso

La tabla `assessment_administrations` tiene, desde `SCHEMA_V1`, una columna `raw_responses TEXT`
pensada originalmente para "JSON opcional, ítem por ítem" (ver `docs/ARCHITECTURE.md` línea 378).
Esta fase **decidió explícitamente no usarla**: ni `repositories::assessments` ni
`services::assessments` la leen o la escriben nunca, y no aparece en ningún tipo de la API ni en
ningún componente de React. Queda como columna legado sin uso, documentada aquí:

- Guardar las respuestas ítem por ítem de un instrumento protegido equivaldría, en la práctica, a
  reconstruir el instrumento completo dentro de la base de datos — exactamente lo que la regla de
  copyright de esta fase prohíbe.
- No se elimina la columna (eso sí requeriría una migración destructiva sobre una tabla existente,
  contra la regla permanente de la Fase 12+ de nunca aplicar cambios destructivos sin advertencia
  explícita) — simplemente se ignora en todo el código nuevo.
- Si una fase futura decide usarla, es una decisión de producto que debe evaluarse explícitamente
  contra la regla de copyright, nunca una reactivación silenciosa.

## 3. Auditoría de esquema previa y migración `V7`

Antes de escribir cualquier código de esta fase se auditó el esquema existente
(`assessment_instruments`/`assessment_administrations`, sin cambios desde `SCHEMA_V1`) contra los
requisitos de este vertical — ver `Plan-Fase-13-pendiente-de-aprobacion.md` para el análisis
completo. Se encontraron dos huecos reales, aprobados explícitamente antes de codificarse:

- `assessment_instruments` no tenía `abbreviation` ni `category` (solo `name`/`description`/
  `is_custom`).
- `assessment_administrations` no tenía ningún vínculo opcional a un proceso terapéutico
  (`episode_id`) — a diferencia de `sessions`/`therapeutic_goals`, que sí lo recibieron quirúrgicamente
  en `SCHEMA_V4` cuando se introdujo `treatment_episodes`. Esta tabla es anterior a
  `treatment_episodes` (nació en `SCHEMA_V1`) y nunca se revisó retroactivamente.

`SCHEMA_V7` (puramente aditiva, sin tocar ninguna columna existente) agrega:

```sql
ALTER TABLE assessment_instruments ADD COLUMN abbreviation TEXT;
ALTER TABLE assessment_instruments ADD COLUMN category TEXT;

ALTER TABLE assessment_administrations ADD COLUMN episode_id TEXT
  REFERENCES treatment_episodes(id) ON DELETE SET NULL;
CREATE INDEX idx_assessment_administrations_episode ON assessment_administrations(episode_id);
```

`abbreviation`/`category` son texto libre, sin `CHECK`, a propósito — la usuaria mantiene su propio
catálogo, sin una taxonomía cerrada que mantener. `episode_id` sigue exactamente el mismo patrón ya
usado por `sessions.episode_id`/`therapeutic_goals.episode_id`: nulable, `ON DELETE SET NULL`,
vínculo opcional. `context` (`ingreso`/`seguimiento`/`alta`, ya existente desde `SCHEMA_V1`) no
sustituye este vínculo: es un enum sobre *cuándo* se administró en términos generales, no una
referencia al proceso concreto — dos evaluaciones de "seguimiento" del mismo paciente pueden
pertenecer a procesos distintos si hubo un reingreso.

Verificado con `fresh_database_has_v7_columns` (una base nueva tiene las tres columnas desde el
arranque) y `v7_migration_is_idempotent_and_preserves_v1_assessment_data` (una fila insertada antes
de `V7` recibe `NULL` en las columnas nuevas sin perder ningún dato existente).

## 4. Modelo de datos

```sql
CREATE TABLE assessment_instruments (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  is_custom INTEGER NOT NULL DEFAULT 0 CHECK (is_custom IN (0,1)),
  abbreviation TEXT,   -- V7
  category TEXT        -- V7
);

CREATE TABLE assessment_administrations (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  instrument_id TEXT NOT NULL REFERENCES assessment_instruments(id) ON DELETE RESTRICT,
  administered_at TEXT NOT NULL,
  context TEXT CHECK (context IN ('ingreso','seguimiento','alta')),
  raw_responses TEXT,          -- legado, nunca usado (ver sección 2)
  total_score REAL,
  subscale_scores TEXT,
  interpretation_text TEXT,
  created_at TEXT NOT NULL DEFAULT (...),
  updated_at TEXT NOT NULL DEFAULT (...),
  deleted_at TEXT,
  episode_id TEXT REFERENCES treatment_episodes(id) ON DELETE SET NULL  -- V7
);
```

`is_custom` siempre se inserta en `1`: en esta fase no existe ningún catálogo precargado de
instrumentos conocidos — todo instrumento nace del catálogo que la propia usuaria mantiene. La
columna se conserva tal como la definió `SCHEMA_V1`, por si una fase futura decide sembrar nombres
de instrumentos comunes (nunca su contenido protegido).

`instrument_id`/`patient_id` usan `ON DELETE RESTRICT`: un instrumento con administraciones
asociadas no puede borrarse (no existe ningún comando de borrado de instrumentos, solo edición de
metadatos — ver sección 6), y un paciente con evaluaciones asociadas tampoco, mismo criterio que el
resto del dominio.

## 5. `subscale_scores` — JSON de texto libre, validado solo sintácticamente

Igual criterio que `patient_clinical_profile.risk_flags` (Fase 6): el servicio valida que sea JSON
sintácticamente correcto (`serde_json::from_str`), pero **nunca interpreta su estructura** —
ninguna clave es obligatoria, ninguna forma se impone. La profesional decide cómo estructurar las
subescalas de cada instrumento.

## 6. Catálogo de instrumentos

- `create_instrument`/`update_instrument`: nombre (obligatorio, único en todo el catálogo — un
  nombre duplicado se rechaza con un error de dominio claro, no con la violación cruda de `UNIQUE`
  de SQLite), abreviatura/descripción/categoría (opcionales, texto libre).
- Sin eliminación de instrumentos: `ON DELETE RESTRICT` ya impediría borrar uno con
  administraciones asociadas, y no hay ningún caso de uso pedido para borrar uno que nunca se usó.
- El catálogo es **compartido entre todos los pacientes** — nunca se filtra por paciente.

## 7. Administraciones

- `create_administration`: rechaza un paciente inexistente o archivado (mismo criterio que
  `services::therapy_tasks::create_task`), un instrumento inexistente, una fecha con formato
  inválido, un `context` fuera de las tres opciones válidas, o `subscale_scores` con JSON inválido.
  Si se informa `episode_id`, se revalida con `treatment_episodes::check_episode_assignable` (el
  proceso debe existir, pertenecer al mismo paciente, y estar en un estado que acepte asignaciones
  nuevas) — mismo mecanismo ya usado por `sessions`/`therapeutic_goals`.
- `update_administration`: no vuelve a comprobar si el paciente está archivado (corregir un
  registro histórico de un paciente archivado sigue permitido, mismo criterio que
  `services::therapy_tasks::update_task` / `services::payments::update_payment`), pero si
  `episode_id` cambia, sí se revalida.
- `total_score` acepta valores negativos: algunos instrumentos usan puntajes estandarizados (ej.
  Z-scores) que pueden serlo — mismo criterio ya documentado en `docs/db-schema.md` para esta misma
  columna.
- Archivar/restaurar (`archive_administration`/`restore_administration`) es soft delete puro, mismo
  patrón que el resto del dominio — nunca hay borrado físico alcanzable desde un comando normal.

## 8. Minimización de IPC

`AssessmentAdministrationSummary` (usado por `list_assessment_administrations`,
`list_archived_assessment_administrations` y `list_assessment_administrations_for_instrument`)
incluye el nombre/abreviatura del instrumento (vía `JOIN`, sin una segunda consulta desde React) y
el puntaje total, pero **nunca** `subscale_scores` ni `interpretation_text` — ese contenido más
extenso solo viaja al abrir una administración concreta con `get_assessment_administration`. Mismo
criterio de minimización ya usado por `SafetyPlanSummary` (Fase 12).

## 9. Evolución longitudinal

`list_administrations_for_instrument(patient_id, instrument_id)` devuelve únicamente las
administraciones de **ese instrumento** para ese paciente, ordenadas de la más antigua a la más
reciente — reutiliza el índice `idx_assessments_patient_instrument` ya presente desde `SCHEMA_V1`.
Nunca mezcla instrumentos distintos: no existe ninguna consulta ni vista de este dominio que
combine puntajes de instrumentos diferentes en una misma serie.

## 10. Decisión conservadora de dependencias — sin librería de gráficos

Se evaluó explícitamente instalar una librería de gráficos (Recharts, mencionada como plan original
en `docs/ARCHITECTURE.md` línea 389, "Recharts consulta directamente sobre ella" — línea ahora
desactualizada por esta misma decisión) y se decidió **no hacerlo** para el V1 de este vertical:
la evolución se muestra como una tabla simple (fecha, contexto, puntaje total), mismo criterio
conservador ya aplicado en Fase 6.1 (`docs/geographic-stats.md`) de preferir tabla/SVG-CSS antes que
sumar una dependencia nueva. Si en el futuro se necesita una visualización gráfica, es una decisión
a evaluar explícitamente entonces, no una dependencia agregada por adelantado "por si acaso".

## 11. Frontend

`src/features/assessments/`: `types.ts`, `api.ts`, `schema.ts` (validación con `zod`, mismo patrón
que `safety-plan`) y `AssessmentsTab.tsx` (pestaña "Evaluaciones" de la ficha del paciente):

- Lista de administraciones activas/archivadas del paciente, con acciones Ver evolución / Editar /
  Archivar / Restaurar.
- Modal "Registrar evaluación" / "Editar evaluación" — permite crear un instrumento nuevo del
  catálogo sin salir del formulario ("Nuevo" junto al selector de instrumento).
- Modal "Catálogo de instrumentos" — lista y edición de metadatos, independiente de cualquier
  paciente concreto.
- Vista "Evolución" — tabla simple por instrumento (sección 10).
- Si el paciente está archivado, "Registrar evaluación" queda deshabilitado (con un aviso
  explicativo), pero "Editar"/"Ver evolución" de evaluaciones ya existentes siguen disponibles —
  autoridad real en `services::assessments`, nunca solo ocultamiento en React.

## 12. Procesos cerrados y reingreso

Igual que el resto del dominio (ver `docs/episode-closure.md` sección de "Procesos cerrados y
reingreso" de otras verticales): el vínculo opcional a un proceso terapéutico (`episode_id`) no
impide consultar ni editar una administración cuyo proceso vinculado ya esté cerrado o pausado —
`check_episode_assignable` solo se ejecuta al **crear** el vínculo o al **cambiarlo**, nunca al
simple hecho de leer o editar otros campos de una administración ya existente que ya tenía ese
vínculo.

## 13. Google Calendar

Ninguna evaluación se sincroniza jamás con Google Calendar — ningún archivo de este dominio
(`repositories::assessments`, `services::assessments`, `commands::assessments`,
`src/features/assessments/*`) importa nada de `calendar::*`.

## 14. Backup / Restore

Sin cambios en el mecanismo de Backup/Restore (Fase 10): al ser una migración aditiva más
(`SCHEMA_V7`), un respaldo creado después de esta fase incluye las columnas nuevas como cualquier
otra columna del esquema. Verificado manualmente con un ciclo completo (crear evaluación → backup →
modificar/eliminar → restaurar → el estado vuelve exactamente al respaldado).

## 15. Tests

668 tests de backend (37 nuevos respecto al baseline de micro-hardening de Plan de Seguridad):
`repositories::assessments` (13), `services::assessments` (21) y 3 en `db::migrations` para
`SCHEMA_V7` (columnas presentes desde el arranque, idempotencia y preservación de datos anteriores
a `V7`, vínculo a un proceso terapéutico de punta a punta).

## 16. Limitaciones conocidas

- No hay atajo para reutilizar un instrumento reciente sin abrir el selector completo.
- No hay exportación/PDF/impresión de una evaluación ni de su evolución.
- No hay eliminación de instrumentos del catálogo (ver sección 6) — un instrumento creado por
  error permanece, editable, pero no puede quitarse de la lista.
- La evolución longitudinal es una tabla, no un gráfico (decisión conservadora, sección 10).
