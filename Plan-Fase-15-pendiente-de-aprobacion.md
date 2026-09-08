# Fase 15 — Formulación Clínica Textual Versionada — auditoría de esquema y punto de STOP

Este documento cierra el paso obligatorio "auditar antes de escribir código" pedido explícitamente
para Fase 15. La conclusión de la auditoría es que **el esquema actual no puede representar
correctamente la asociación paciente/proceso que tú mismo describiste como preferencia clínica**
sin una migración — así que, siguiendo tu instrucción explícita ("Si implementar asociación
correcta a treatment_episode requiere migración: DETENTE antes de crearla"), **no se ha escrito
ninguna migración ni código de Fase 15**. Este archivo es esa auditoría y esa explicación.

## 0. Bloque A — regularización del estado Git heredado (ya resuelta)

Antes de esta auditoría: `HEAD` real confirmado en `b42d356` (no `c4fb227` como declaraba el cierre
de Fase 14 — la diferencia es exactamente el commit documental `docs: agregar auditoría de
transición post-Fase 13 e informe de cierre de Fase 14`, que yo mismo creé al final del turno
anterior en respuesta al stop-hook del repositorio, ya visible en esta conversación). Presenté la
discrepancia y la pregunta de política de versionado de reportes de proceso; confirmaste
explícitamente:

- Los informes de cierre, auditorías y planes pendientes de aprobación **se versionan** en el
  repositorio de aquí en adelante, como artefactos de trazabilidad.
- `b42d356` permanece tal cual — sin revertir, sin reorganización retroactiva de informes
  históricos.
- `docs/*.md` técnicos siguen siendo documentación funcional normal, separada de estos artefactos
  de proceso.

Working tree confirmado limpio (`git status --porcelain=2` sin salida) antes de la regresión
baseline. Regla registrada; no se requiere ninguna acción de código para esto.

## 1. Regresión baseline (con working tree limpio)

| Comando | Resultado |
|---|---|
| `cargo test --release` | **670/670 en verde** |
| `cargo clippy --release --all-targets` | 0 warnings |
| `cargo build --release` | limpio |
| `npm run build` | limpio |
| `npm run lint` | 23 warnings, 0 errors |
| `git diff --check` | limpio |

Coincide exactamente con lo declarado en el cierre de Fase 14.

## 2. Auditoría real del esquema (verificada línea a línea, no asumida)

```sql
CREATE TABLE case_formulations (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  title TEXT NOT NULL,
  model_type TEXT,
  created_at TEXT NOT NULL DEFAULT (...),
  updated_at TEXT NOT NULL DEFAULT (...),
  deleted_at TEXT
);
-- trigger touch updated_at

CREATE TABLE formulation_versions (
  id TEXT PRIMARY KEY,
  formulation_id TEXT NOT NULL REFERENCES case_formulations(id) ON DELETE CASCADE,
  version_number INTEGER NOT NULL CHECK (version_number >= 1),
  summary_text TEXT,
  created_at TEXT NOT NULL DEFAULT (...),
  UNIQUE (formulation_id, version_number)
);

CREATE TABLE formulation_nodes (
  id TEXT PRIMARY KEY,
  formulation_version_id TEXT NOT NULL REFERENCES formulation_versions(id) ON DELETE CASCADE,
  node_type TEXT NOT NULL,
  label TEXT NOT NULL,
  description TEXT,
  position_x REAL NOT NULL,
  position_y REAL NOT NULL
);

CREATE TABLE formulation_edges (
  id TEXT PRIMARY KEY,
  formulation_version_id TEXT NOT NULL REFERENCES formulation_versions(id) ON DELETE CASCADE,
  source_node_id TEXT NOT NULL REFERENCES formulation_nodes(id) ON DELETE CASCADE,
  target_node_id TEXT NOT NULL REFERENCES formulation_nodes(id) ON DELETE CASCADE,
  relation_label TEXT,
  CHECK (source_node_id <> target_node_id)
);
-- + dos triggers BEFORE INSERT/UPDATE que fuerzan que ambos nodos de un edge
--   pertenezcan a la misma formulation_version_id

-- terapeutic_goals:
formulation_id TEXT REFERENCES case_formulations(id) ON DELETE SET NULL  -- nullable, opcional
```

Hallazgos concretos:

- **`case_formulations` no tiene `episode_id`** — su único ancla relacional es `patient_id`. No
  existe ninguna columna, índice ni trigger que vincule una formulación a un proceso terapéutico
  concreto.
- **`formulation_versions` no tiene `status`/`is_current`/`confirmed_at`/`superseded_at`** —
  únicamente `version_number` (entero, `CHECK >= 1`) y `summary_text` (texto libre, nullable).
  `UNIQUE(formulation_id, version_number)` impide números de versión duplicados. **No tiene
  `updated_at`** (a diferencia de `case_formulations`) — coherente con ser inmutable una vez creada
  (nunca se espera un `UPDATE` de contenido sobre una fila ya existente).
- **`formulation_nodes`/`formulation_edges`** están completos y ya reforzados con triggers de
  integridad cruzada (mismo patrón que otras tablas versionadas del proyecto) — listos para una
  futura Formulación Visual, **cero uso productivo hoy, cero fila real en ningún ambiente**.
- **`therapeutic_goals.formulation_id`** es nullable, `ON DELETE SET NULL` — un objetivo puede
  vincularse opcionalmente a una formulación, sin que borrar la formulación fuerce nada destructivo
  sobre el objetivo. Ya mapeado en el struct `Goal` (`repositories::goals`) pero — según
  `docs/goals.md` línea 229 — **nunca expuesto ni escribible desde ningún comando actual**: es un
  campo de solo lectura estructural, sin ningún flujo de UI que lo setee.
- **Cero vertical productiva existe hoy** (confirmado con `find`: ningún archivo en
  `repositories/`, `services/`, `commands/` ni `src/features/` menciona formulación) — así que
  **cero filas reales** existen en ningún ambiente. Es, estructuralmente, el momento más seguro
  posible para tocar este esquema si hiciera falta.
- **Precedente histórico directo, ya documentado**: `docs/treatment-episodes.md` (Fase 9, línea
  325-327) registra explícitamente la decisión original de **no** agregar `episode_id` a
  `case_formulations` (junto con `payments`/`patient_prep_notes`/`therapy_tasks`/`documents`/
  `assessment_administrations`/`reminders`) porque *"ninguna necesidad concreta lo exigió durante
  la implementación real"* — es decir, no había todavía ningún vertical productivo de Formulación
  que lo necesitara. Ese mismo razonamiento se revisó explícitamente en Fase 13 para
  `assessment_administrations` (`SCHEMA_V7`, aprobada por ti) en el momento exacto en que se
  construyó su primer vertical productivo. Fase 15 es exactamente ese mismo momento para
  Formulación.

## 3. Decisión pendiente: patient_id vs. episode_id — presentada, no decidida unilateralmente

### Opción A — mantener `patient_id` solamente (sin migración)

- **Ventajas**: cero migración, cero riesgo técnico, la más rápida de implementar.
- **Desventajas**: produce exactamente el modelo incorrecto que tú mismo señalaste como no
  deseado. Con `patient_id` como único ancla, "la formulación del paciente" es una sola serie
  longitudinal — la versión más reciente aparecería como "la vigente" sin importar qué proceso
  terapéutico esté activo. Un reingreso (Proceso A cerrado → Proceso B nuevo) no tendría ninguna
  forma estructural de evitar que la última formulación de A siga apareciendo como si fuera la de
  B. Esto contradice directamente tu propia regla del §22 ("no copiar automáticamente la
  formulación A a B") — sin `episode_id`, no hay nada que copiar *ni que distinguir*: solo existe
  una serie por paciente, nunca por proceso.

### Opción B — agregar `episode_id` opcional a `case_formulations` (requiere migración) — recomendada

```sql
ALTER TABLE case_formulations ADD COLUMN episode_id TEXT
  REFERENCES treatment_episodes(id) ON DELETE SET NULL;
CREATE INDEX idx_case_formulations_episode ON case_formulations(episode_id);
```

- **Nullability**: nullable — una formulación puede seguir existiendo sin proceso formal asociado,
  mismo criterio ya usado por `sessions.episode_id`/`therapeutic_goals.episode_id` (Fase 4) y
  `assessment_administrations.episode_id` (Fase 13, `SCHEMA_V7`).
- **FK / `ON DELETE`**: `REFERENCES treatment_episodes(id) ON DELETE SET NULL` — nunca `RESTRICT`
  (los procesos no se borran físicamente, solo se archivan/cierran; `SET NULL` es el mismo patrón
  ya usado en los dos precedentes citados).
- **Legacy / backfill**: **ninguno necesario**. A diferencia de la migración original de Fase 9
  (que sí tuvo que hacer backfill real sobre `sessions`/`therapeutic_goals`, verticales ya en uso
  activo con datos reales), aquí no existe ninguna fila de `case_formulations` en ningún ambiente
  — la columna nueva simplemente no tiene nada que retro-completar.
- **Impacto en pacientes con formulaciones futuras**: ninguno negativo — es exactamente la
  asociación correcta que permite que un reingreso empiece sin ninguna formulación heredada
  (`episode_id` de la nueva sería `NULL` hasta que se cree una formulación nueva explícitamente
  para ese proceso).
- **Además habilita, sin código adicional de validación cruzada**, la regla "una formulación
  principal por proceso" que pediste en el §10, con el mismo patrón de índice único parcial ya
  usado tres veces en el proyecto (`idx_treatment_episodes_one_active_per_patient`,
  `idx_episode_closures_active`, `idx_safety_plans_one_current_per_patient`):
  ```sql
  CREATE UNIQUE INDEX idx_case_formulations_one_per_episode
    ON case_formulations(episode_id) WHERE episode_id IS NOT NULL AND deleted_at IS NULL;
  ```

### Opción C — alternativas consideradas y descartadas

- **C1 — inferir el proceso por proximidad temporal** (sin columna, deducir "el proceso activo en
  el momento de creación"): frágil y no auditable — dos reingresos sucesivos harían imposible
  reconstruir con certeza a qué proceso perteneció una formulación antigua. Descartada por
  sacrificar integridad estructural, exactamente lo que tu instrucción pide no hacer.
- **C2 — codificar la asociación dentro del propio `summary_text`** (texto libre mencionando el
  proceso): sin ninguna garantía de integridad referencial, se rompe con un simple error de tipeo.
  Descartada por el mismo motivo.

**Recomendación de esta auditoría: Opción B.** Es el mismo patrón ya aprobado dos veces en este
proyecto (Fase 4, Fase 13), sin backfill, sin riesgo (cero datos reales existentes), y es la única
opción que representa correctamente tu propia preferencia clínica ya declarada explícitamente.

## 4. El resto de las decisiones del encargo — aplicando tus preferencias ya declaradas

Estas no requieren detenerse (ya diste una preferencia explícita en el propio encargo); las aplico
directamente una vez resuelto el punto 3, y las dejo registradas aquí para que las revises junto
con la decisión principal:

- **§10 (cuántas formulaciones)**: una formulación **principal** por proceso (si se aprueba la
  Opción B), con múltiples versiones históricas vía `formulation_versions`. Sin `episode_id`
  (Opción A), esta regla tendría que ser "una formulación principal por paciente" — un modelo
  distinto y más pobre, otra razón a favor de B.
- **§12 (borrador vs. confirmada)**: sin estado de borrador — cada "Actualizar formulación" crea
  directamente una versión nueva confirmada (`formulation_versions` no tiene columnas para
  soportar un borrador persistente, y agregar migración solo para eso no se justifica). La versión
  actual es `MAX(version_number)` para esa formulación. El editor precarga el contenido de la
  versión anterior en el formulario (edición en memoria, en el frontend) antes de guardar como
  versión nueva — nunca hay un `UPDATE` sobre `summary_text` de una versión ya creada.
- **§13-14 (contenido y modelo teórico)**: `summary_text` sigue siendo `TEXT` libre en el esquema;
  el frontend lo estructura como un documento con secciones editables (síntesis, predisponentes,
  precipitantes, perpetuantes, protectores, patrones, hipótesis, focos terapéuticos, plan de
  intervención, observaciones) que se serializan a un único bloque de texto legible al guardar —
  sin ninguna columna nueva, sin imponer que todas las secciones sean obligatorias. `model_type`
  quirda como texto libre con una lista corta de sugerencias comunes (TCC, 5P, transdiagnóstico,
  "Otro") en un `<select>` con opción de texto libre — nunca una taxonomía cerrada.

## 5. Archivos que se crearían (una vez aprobada la sección 3)

Backend: `repositories/formulations.rs`, `services/formulations.rs`, `commands/formulations.rs` +
registro en los `mod.rs`/`lib.rs` correspondientes. Frontend:
`src/features/formulation/{types.ts,api.ts,schema.ts,FormulationTab.tsx,FormulationHistory.tsx}` +
activación de la pestaña "Formulación" ya reservada en `PatientDetailScreen.tsx`. Documentación:
`docs/formulation.md` + fila nueva en la tabla de fases de `docs/ARCHITECTURE.md`.

`formulation_nodes`/`formulation_edges` no se tocan en ningún archivo — permanecen reservados,
documentados explícitamente en `docs/formulation.md` como "para una futura Formulación Visual".

## 6. Tests previstos (una vez aprobada la sección 3)

Repositorio: crear formulación, obtener, listar por paciente/proceso, crear v1/v2, versión
actual, historial ordenado, no-sobrescritura. Servicio: paciente inexistente/archivado, proceso
inexistente/de otro paciente/cerrado, incremento de versión, solo-lectura de versiones históricas,
reingreso sin copia automática, regla de una formulación por proceso. Privacidad: el DTO resumen
(listado) nunca lleva `summaryText` completo.

## 7. Estado Git

`git status --porcelain=2` vacío antes de este documento (Bloque A ya resuelto). Este archivo es la
única adición pendiente de esta auditoría — nada de código, nada de esquema, cero commits, cero
push, hasta tu aprobación.

## 8. Decisión que necesito que confirmes antes de continuar

¿Apruebas la **Opción B** (agregar `episode_id` opcional a `case_formulations` vía una nueva
migración aditiva `SCHEMA_V8`, sin backfill, sin tocar ninguna tabla existente) para que
Formulación quede correctamente asociada al proceso terapéutico, tal como describiste como tu
preferencia clínica? ¿O prefieres la Opción A (solo `patient_id`, sin migración, aceptando que una
formulación no podrá distinguirse por proceso) u otra alternativa?

No he escrito ninguna línea de código ni de migración de Fase 15. Quedo a la espera de tu decisión
sobre este punto antes de continuar con el resto de la fase.
