# Fase 13 — Evaluaciones Clínicas / Psicométricas — auditoría de esquema y punto de STOP

Este documento es el resultado del paso 5 pedido explícitamente antes de escribir ningún código
de Fase 13: **"auditoría real del schema de Evaluaciones"**. La conclusión de la auditoría es que
el esquema existente **no cubre** dos necesidades de V1 descritas en el encargo de Fase 13, así
que — siguiendo tanto la instrucción explícita de Fase 13 ("si una migración ES necesaria,
DETENTE y explica qué falta, por qué, y alternativas, antes de crearla") como la regla permanente
§11 de `CLAUDE.md` ("cambio del modelo de base de datos" es uno de los casos que exige detenerse
y esperar aprobación) — **no se ha escrito ninguna migración ni código de Fase 13**. Este archivo
es esa explicación; nada más se hizo hasta tener tu aprobación.

## 0. Contexto: baseline y hardening ya cerrados

Antes de esta auditoría se completaron y ya están en el commit estable `684f048`
(`fix: reforzar versionado y archivado del plan de seguridad`), pusheado a
`claude/cuaderno-clinico-desktop-udijjq`:

- Verificación de baseline (build/test/clippy/lint limpios antes de tocar nada).
- El micro-hardening completo de Fase 12 (Parte I del encargo): re-chequeo autoritativo de
  paciente archivado en `update_draft`/`confirm_draft`/`add_contact`/`update_contact`/
  `delete_contact`, exención confirmada de `discard_draft`, y la nueva
  `create_draft_from_current` que copia atómicamente en el backend contenido narrativo +
  `reviewed_at` + todos los contactos (con IDs nuevos) al iniciar una actualización del plan.
- 15 tests nuevos (631 tests totales, todos pasando), `cargo clippy --release --all-targets`
  limpio, `cargo build --release` limpio, `npm run build`/`npm run lint` limpios, `git diff
  --check` limpio, árbol de trabajo limpio y `HEAD` = `origin/claude/cuaderno-clinico-desktop-udijjq`.
- `docs/safety-plan.md` actualizado (§5, §12, §18, §19) para reflejar el comportamiento nuevo y
  quitar las dos limitaciones ya resueltas.

**Limitación que dejo explícita:** en este entorno remoto no pude ejecutar la prueba manual de GUI
con Xvfb+xdotool que se usó en fases anteriores (el clasificador de permisos de este entorno de
ejecución en la nube bloqueó tanto mover el vault existente como lanzar el binario en segundo
plano). La verificación de este hardening se apoya en los 15 tests automatizados nuevos (que cubren
exactamente los escenarios pedidos: archivado bloquea, restaurar desbloquea, `discard` no se
bloquea, copia de contactos con IDs nuevos y aislamiento entre versiones) más el build/clippy/lint
limpios — no en una verificación visual en la interfaz. Si quieres, puedo dejar instrucciones
puntuales para que verifiques esos casos tú misma en un vault desechable local.

## 1. El problema

`assessment_instruments`/`assessment_administrations` existen desde `SCHEMA_V1` (Fase 1.3) —
nunca se les construyó una capa de servicio ni de interfaz ("vertical productivo"), tal como decía
la auditoría previa. Pero al comparar su forma actual, campo por campo, contra lo que pide el
encargo de Fase 13, encuentro **tres huecos concretos** que el esquema actual no cubre:

### 1.1 `assessment_instruments` no tiene `abbreviation` ni `category`

Esquema actual (idéntico al de `docs/ARCHITECTURE.md` §4 desde el día uno, sin cambios):

```sql
CREATE TABLE assessment_instruments (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,     -- ej. "BDI-II", "PHQ-9"
  description TEXT,
  is_custom INTEGER NOT NULL DEFAULT 0 CHECK (is_custom IN (0,1))
);
```

El encargo de Fase 13 pide que el catálogo de instrumentos tenga **nombre / abreviatura /
descripción / categoría** (texto libre preferido sobre enum). Hoy solo existen `name` y
`description`; no hay ninguna columna para abreviatura ni para categoría. `is_custom` no sirve
para ninguna de las dos cosas — es un flag booleano sin relación con "sigla" ni "tipo de
instrumento".

### 1.2 `assessment_administrations` no tiene ningún campo de "vínculo opcional a proceso"

```sql
CREATE TABLE assessment_administrations (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  instrument_id TEXT NOT NULL REFERENCES assessment_instruments(id) ON DELETE RESTRICT,
  administered_at TEXT NOT NULL,
  context TEXT CHECK (context IN ('ingreso','seguimiento','alta')),
  raw_responses TEXT,
  total_score REAL,
  subscale_scores TEXT,
  interpretation_text TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
```

El encargo pide "vínculo opcional a un proceso" (mismo concepto que ya existe para sesiones y
objetivos: `sessions.episode_id`/`therapeutic_goals.episode_id`, agregados en `SCHEMA_V4` cuando
se introdujo `treatment_episodes`). Esta tabla es anterior a `treatment_episodes` (nació en
`SCHEMA_V1`, `treatment_episodes` nació en `SCHEMA_V4`) y nunca se revisó retroactivamente como sí
se hizo con `sessions`/`therapeutic_goals` en ese momento — a diferencia de `payments`,
`patient_prep_notes` y `therapy_tasks`, que quedaron **deliberadamente** sin `episode_id` (ver
comentario explícito en `migrations.rs` línea 658), aquí no hay ninguna decisión documentada de
excluirlo a propósito: simplemente nadie lo tocó porque no existía vertical de evaluaciones.

`context` (`ingreso`/`seguimiento`/`alta`) **no sustituye** este vínculo: es un enum sobre *cuándo*
se administró en términos generales, no una referencia a *qué proceso concreto* — dos evaluaciones
de "seguimiento" del mismo paciente pueden pertenecer a procesos terapéuticos distintos si hubo
reingreso, y el modelo actual no puede distinguirlas.

### 1.3 Lo que SÍ cubre el esquema actual (nada de esto necesita migración)

- Registrar qué se administró, cuándo, puntaje total, subescalas (JSON libre,
  `subscale_scores`), interpretación redactada por la profesional (`interpretation_text`, texto
  libre) — todo ya existe y ya es compatible con "nunca auto-calcular/auto-interpretar": ninguna
  columna impone estructura que obligue a un cálculo automático.
- `raw_responses` (columna ya presente, nunca poblada por ninguna vertical existente) — cumple
  exactamente el rol que pide Fase 13: queda como columna legado sin uso, documentada, nunca
  expuesta en la UI. No hace falta ni agregarla ni quitarla.
- Evolución longitudinal dentro de un mismo instrumento: el índice
  `idx_assessments_patient_instrument (patient_id, instrument_id, administered_at)` ya está
  pensado exactamente para esa consulta (filtrar por paciente+instrumento, ordenar por fecha).
- Borrado blando (`deleted_at`), `created_at`/`updated_at` con trigger automático: mismo patrón
  que el resto del dominio, sin cambios necesarios.
- La restricción de integridad referencial (`instrument_id` con `ON DELETE RESTRICT`) ya impide
  borrar un instrumento del catálogo si tiene administraciones asociadas — coherente con "nunca
  perder datos clínicos".

## 2. Por qué la arquitectura actual no alcanza

No es que el diseño esté mal — es que el encargo de Fase 13 introduce dos requisitos de producto
que **no estaban en el esquema original de Fase 1.3** (que a su vez venía literal de
`ARCHITECTURE.md` §4, escrito antes de que existiera el concepto de "catálogo propio de la
usuaria con categoría" o el de "vínculo opcional a proceso" para evaluaciones específicamente).
Ninguna combinación de las columnas existentes puede representar de forma honesta ni "abreviatura
separada del nombre completo" ni "a qué proceso terapéutico concreto perteneció esta
administración" — sería forzar semántica nueva dentro de columnas (`description`, `context`) que
ya tienen un significado distinto y ya declarado.

## 3. Qué se propone (pendiente de tu aprobación — nada de esto está escrito todavía)

Una migración `SCHEMA_V7`, puramente aditiva, seleccionada por replicar el patrón ya usado en
`SCHEMA_V4` (línea 703-707, cuando se agregó `episode_id` a `sessions`/`therapeutic_goals`) sin
inventar nada nuevo:

```sql
ALTER TABLE assessment_instruments ADD COLUMN abbreviation TEXT;
ALTER TABLE assessment_instruments ADD COLUMN category TEXT;

ALTER TABLE assessment_administrations ADD COLUMN episode_id TEXT
  REFERENCES treatment_episodes(id) ON DELETE SET NULL;
CREATE INDEX idx_assessment_administrations_episode ON assessment_administrations(episode_id);
```

- `abbreviation`/`category`: `TEXT` nulable, sin `CHECK`, texto libre — exactamente como pediste
  ("preferir texto libre sobre enum"), sin lista cerrada de categorías que tendría que mantenerse
  a mano cada vez que registres un instrumento nuevo.
- `episode_id`: nulable, `ON DELETE SET NULL` (nunca `RESTRICT`, para no impedir nunca borrar —
  bueno, los episodios no se borran, se archivan/cierran; pero mantiene el mismo comportamiento no
  bloqueante que `sessions.episode_id`/`therapeutic_goals.episode_id`), no `NOT NULL` porque el
  vínculo es explícitamente opcional (una evaluación puede registrarse sin proceso formal
  asociado, igual que una sesión).
- Índice nuevo por paralelismo con `idx_sessions_episode`/`idx_therapeutic_goals_episode`.

Filas existentes (si las hubiera — hoy no hay ninguna, ver §5) recibirían `NULL` en las tres
columnas nuevas sin ninguna pérdida de datos ni necesidad de backfill, porque son genuinamente
opcionales y no había ningún dato previo del que derivarlas.

### Alternativas que también considero válidas, si prefieres no tocar el esquema ahora

1. **Diferir `episode_id` para una fase posterior** (como se hizo deliberadamente con `payments`/
   `patient_prep_notes`/`therapy_tasks`): Fase 13 registraría evaluaciones sin vínculo a proceso
   en absoluto, y el vínculo opcional se agregaría en una fase de "Fase 13.1" cuando haya un caso
   de uso real que lo necesite. Reduce el alcance de V1, pero evita tocar el esquema ahora.
2. **Diferir `abbreviation`/`category` también**, y que el catálogo V1 sea solo nombre +
   descripción (como ya está hoy) — la abreviatura se escribiría como parte del nombre libre (ej.
   `name = "BDI-II — Inventario de Depresión de Beck"`) en vez de en un campo separado. Más simple,
   pero pierde la posibilidad de mostrar "BDI-II" como columna corta en una lista/tabla sin
   parsear texto.
3. **La migración completa como se propone arriba** (mi recomendación): es puramente aditiva, no
   afecta ninguna fila existente (no hay ninguna hoy), sigue exactamente el patrón ya usado y
   aprobado en `SCHEMA_V4`, y evita que Fase 13 quede con una limitación de producto documentada
   desde el día uno que probablemente haya que revertir pronto de todas formas.

**Si eliges la opción 3, la migración se implementaría dentro de Fase 13 (no como un commit
separado)**, ya que es parte integral de construir ese vertical — a diferencia del hardening de
Plan de Seguridad, que sí tenía sentido como commit independiente por corregir una fase ya cerrada.

## 4. Qué archivos/tablas se verían afectados

- `src-tauri/src/db/migrations.rs`: nueva constante `SCHEMA_V7`, agregada a la lista de
  `Migrations::new(vec![...])`, más los tests de migración existentes que enumeran columnas por
  tabla (ej. el test que verifica las columnas de cada tabla tras `to_latest`).
- Ninguna tabla existente pierde ninguna columna; ninguna columna existente cambia de tipo ni de
  restricción.
- `docs/db-schema.md` y `docs/ARCHITECTURE.md` §4 (la sección "Evaluaciones") necesitarían
  actualizarse para reflejar las columnas nuevas — y, por separado, `ARCHITECTURE.md` línea 389
  ("Recharts consulta directamente sobre ella") queda **contradicha por tu propia instrucción de
  esta fase** (evitar Recharts, preferir tabla/SVG-CSS) — lo marco aquí porque es exactamente el
  tipo de "decisión anterior que cambia" que la regla §1 de `CLAUDE.md` pide señalar
  explícitamente; como el cambio viene de tu propia instrucción explícita en este mismo encargo,
  lo tomo como ya aprobado, y simplemente actualizaría esa línea de `ARCHITECTURE.md` como parte
  de la documentación de Fase 13.

## 5. Riesgos que introduce

Mínimos, y acotados:

- Es una migración `ALTER TABLE ADD COLUMN` pura — SQLite la ejecuta sin reescribir la tabla,
  reversible en el sentido de que las columnas nuevas simplemente no se usarían si algo saliera
  mal (no hay forma de que rompa ninguna fila existente).
- No hay ninguna fila real en `assessment_administrations` hoy en ningún entorno de desarrollo
  conocido en este repositorio (nunca se construyó una vertical que escribiera en ella) — es,
  estructuralmente, el momento más seguro posible para tocar este esquema.
- El único riesgo real es de alcance/tiempo: agregar `episode_id` implica que el servicio de Fase
  13 debe validar ese vínculo igual que ya validan `services::sessions`/`services::therapeutic_goals`
  (episodio debe pertenecer al mismo paciente, etc. — patrón ya existente y probado, se reutiliza,
  no se inventa).

## 6. Cómo se preservarían los datos y funcionalidades existentes

- Ninguna migración anterior (`V1`–`V6`) se modifica — la nueva se agrega al final de la lista,
  igual que todas las anteriores.
- Los tests de "una base ya existente se actualiza preservando sus datos" (patrón ya establecido
  en este proyecto, ver `applying_a_new_migration_preserves_existing_data`) se extenderían para
  cubrir también `V6 → V7`.
- Ninguna funcionalidad de Fases 1–12 toca `assessment_instruments`/`assessment_administrations`
  hoy, así que el riesgo de regresión sobre código existente es cero.

## 7. Decisión que necesito de ti antes de continuar

1. ¿Apruebas la migración `SCHEMA_V7` propuesta en la sección 3 (columnas `abbreviation`/
   `category` en `assessment_instruments`, `episode_id` en `assessment_administrations`), o
   prefieres alguna de las alternativas 1/2, o alguna combinación (por ejemplo: agregar
   `abbreviation`/`category` ahora pero diferir `episode_id`)?
2. ¿Confirmas que `raw_responses` se mantiene como columna legado sin exponer en la UI, documentada
   en `docs/assessments.md` como se especificó, sin ningún otro cambio?
3. ¿Quieres que deje instrucciones puntuales de verificación manual en un vault desechable local
   (dado que este entorno remoto no me permitió ejecutar Xvfb para la prueba de GUI), o prefieres
   que la prueba manual de Fase 13 quede pendiente hasta que la ejecutes tú misma?

No he escrito ninguna línea de código ni de migración de Fase 13. Quedo a la espera de tu
aprobación sobre estos puntos antes de continuar.
