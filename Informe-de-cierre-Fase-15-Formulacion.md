# Informe de cierre — Fase 15: Formulación clínica textual versionada

Sigue el formato obligatorio de `CLAUDE.md` sección 10, más las secciones específicas pedidas
para esta fase (Bloque A + Bloque B del encargo).

## 1. Baseline

Verificado antes de tocar código:

- `git rev-parse HEAD` → `b42d356` — un commit más allá del `c4fb227` que el informe de cierre de
  la Fase 14 declaró como su propio HEAD final. Investigado: `b42d356` es
  `docs: agregar auditoría de transición post-Fase 13 e informe de cierre de Fase 14`, un commit
  puramente documental (verificado con `git show --stat`: solo agrega los dos `.md` de esa
  transición, cero cambios de código/esquema/tests), forzado por el hook de fin de turno del
  proyecto tras el cierre de la Fase 14.
- `git branch --show-current` → `claude/cuaderno-clinico-desktop-udijjq` — correcto.
- `origin/claude/cuaderno-clinico-desktop-udijjq` → `b42d356` — sin divergencia.

## 2. Regularización Git (Bloque A)

Discrepancia presentada explícitamente antes de continuar (sin decidir unilateralmente si
`Auditoria-transicion-Fase-13-siguiente-fase.md` y los informes de cierre debían tratarse como
documentación versionada oficial o como artefactos externos). Resuelto con tu aprobación textual:
de aquí en adelante, informes de cierre, auditorías y planes pendientes de aprobación se
versionan en el repositorio como artefactos de trazabilidad del proyecto; `b42d356` no se revierte
ni se reorganiza retroactivamente; `docs/*.md` técnicos siguen siendo documentación funcional
normal, separada de los artefactos de proceso. Esta política queda registrada aquí y no autorizó
ningún cambio de código ni reorganización del repositorio.

## 3. Regresión inicial (con `b42d356` como baseline real)

| Comando | Resultado |
|---|---|
| `cargo test --release` | **670/670 en verde** |
| `cargo clippy --release --all-targets` | 0 warnings |
| `cargo build --release` | limpio |
| `npm run build` | limpio, sin errores TS |
| `npm run lint` | 23 warnings, 0 errors |
| `git diff --check` | limpio |

## 4. Auditoría de esquema previa (obligatoria antes de escribir código)

Inspeccionadas en `src-tauri/src/db/migrations.rs` (`SCHEMA_V1`, líneas 191-266), sin haber
existido nunca ningún repositorio/servicio/comando/componente de formulación en todo el código
(vertical 100% nueva sobre tablas presentes desde la Fase 1.3):

- `case_formulations`: solo `id, patient_id NOT NULL, title, model_type, created_at, updated_at,
  deleted_at` — sin ningún vínculo al proceso terapéutico.
- `formulation_versions`: `id, formulation_id, version_number CHECK >= 1, summary_text,
  created_at`, `UNIQUE(formulation_id, version_number)` — sin estado de borrador ni `updated_at`.
- `formulation_nodes`/`formulation_edges`: completamente construidas, con triggers de integridad
  cruzada entre versión y nodo/arista — nunca usadas por ningún código existente.

Se confirmó además (grep) que `docs/treatment-episodes.md` (línea 325-327) documenta una decisión
explícita de la Fase 9 de **no** agregar `episode_id` a `case_formulations` entonces, y que
`docs/goals.md` confirma que `therapeutic_goals.formulation_id` nunca se escribe desde ningún
comando actual.

## 5. Decisión patient vs. episode

Presentada en `Plan-Fase-15-pendiente-de-aprobacion.md` con tres opciones (A: mantener solo
`patient_id`; B: agregar `episode_id` opcional con migración; C: alternativas de inferencia
temporal o codificación dentro de `summary_text`, ambas rechazadas por frágiles/opacas). **Tú
aprobaste explícitamente la Opción B.**

## 6. Migración `SCHEMA_V8`

```sql
ALTER TABLE case_formulations ADD COLUMN episode_id TEXT
  REFERENCES treatment_episodes(id) ON DELETE SET NULL;
CREATE INDEX idx_case_formulations_episode ON case_formulations(episode_id);
CREATE UNIQUE INDEX idx_case_formulations_one_per_episode
  ON case_formulations(episode_id) WHERE episode_id IS NOT NULL AND deleted_at IS NULL;
```

Puramente aditiva, `SCHEMA_V1`–`V7` intactos. Mismo patrón de índice único parcial ya usado 3 veces
en el proyecto (`idx_treatment_episodes_one_active_per_patient`/`idx_episode_closures_active`/
`idx_safety_plans_one_current_per_patient`). Se confirmó, antes de crearla, que no existía ninguna
fila real de formulación en ningún entorno — el momento más seguro posible para migrar.

## 7. Modelo de versionado

Una formulación principal por proceso (forzada por el índice único parcial de la sección 6), con
múltiples versiones históricas nunca sobrescritas ni eliminadas desde la UI normal. Sin estado de
"borrador": `formulation_versions` no tiene columna de estado — la versión vigente es siempre
`MAX(version_number)`. Se decidió explícitamente no replicar el modelo más complejo de
`safety_plans` (borrador/vigente/reemplazado): ningún requisito de esta fase exigía un estado
intermedio no confirmado.

`episode_id` es nulable en el esquema (necesario para `ON DELETE SET NULL`), pero obligatorio
(`String`, no `Option<String>`) en `services::formulations::FormulationInput` — decisión propia,
sin necesidad de aprobación adicional por no requerir migración: permitir una formulación "suelta"
del paciente sin proceso habría recreado la ambigüedad que esta fase resolvió.

## 8. Contenido estructurado sin migración

10 secciones conceptuales (síntesis, predisponentes, precipitantes, perpetuantes, protectores,
patrones, hipótesis, focos terapéuticos, plan de intervención, observaciones) definidas en el
**frontend** (`FORMULATION_SECTIONS`), serializadas como bloques Markdown (`## Sección\n\ncontenido`)
dentro de la única columna `summary_text` ya existente. El backend nunca interpreta esta
estructura — cero columnas SQL nuevas para esto. `parseSections` nunca descarta contenido que no
calce con el formato esperado (lo coloca en "Síntesis del caso").

## 9. `model_type`

Texto libre en el esquema (sin `CHECK`) y en el servicio. El frontend ofrece
`FORMULATION_MODEL_SUGGESTIONS` (TCC, 5P, Transdiagnóstico) solo como `<datalist>` de sugerencias —
nunca una lista cerrada; cualquier texto propio es válido.

## 10. Historial de versiones

`FormulationHistory.tsx` existe desde esta misma fase (no se repite la deuda de Historial de
cierres, Fase 11/14). Lista todas las versiones y permite abrir cualquiera en modo exclusivamente
de lectura — ningún botón de edición ni de eliminación en el historial.

## 11. Proceso cerrado

`create_new_version` revalida el proceso vinculado con `check_episode_assignable`
(`services::treatment_episodes`, reutilizado sin duplicar) usando el `episode_id` guardado en la
formulación. Un proceso cerrado rechaza la validación → no se puede crear una versión nueva.
Lectura (formulación actual + historial completo) permanece siempre disponible.

## 12. Reingreso

Un proceso nuevo del mismo paciente nunca hereda ni copia la formulación de un proceso anterior —
verificado con `reingreso_never_copies_the_previous_episodes_formulation`: crear un segundo
proceso y consultar `get_formulation_by_episode` para él devuelve `None` aunque el primer proceso
ya tenga contenido.

## 13. Paciente archivado

`create_formulation`/`create_new_version` rechazan explícitamente un paciente archivado
(`PatientArchived`) — mismo criterio que `require_editable_draft` de `safety_plans` (Fase 12):
crear una versión nueva es "crear contenido clínico nuevo", igual que la primera creación. La
lectura (formulación actual + historial) permanece siempre disponible. Verificado con
`rejects_a_new_version_for_an_archived_patient` y
`allows_a_new_version_again_after_the_patient_is_restored`.

## 14. Objetivos terapéuticos — decisión de no integrar

Auditado `docs/goals.md`: `therapeutic_goals.formulation_id` nunca se escribe desde ningún comando
actual. Un visor de "objetivos relacionados" habría mostrado siempre una lista vacía en la
práctica, sin valor real, y habría requerido tocar `goals.rs` — expresamente fuera de alcance salvo
detención explícita. **Decisión: no integrar Goals en absoluto en esta fase.**

## 15. `formulation_nodes`/`formulation_edges`

No se usan, no se leen, no se escriben ni se modifican en esta fase. Quedan explícitamente
reservadas para una futura fase de Formulación Visual — documentado en `docs/formulation.md`
sección 8. Ningún archivo de este vertical las referencia.

## 16. Repositorio (`src-tauri/src/repositories/formulations.rs`, nuevo)

`insert_formulation`, `find_formulation_by_id`, `find_formulation_by_episode`,
`list_formulations_by_patient` (con `current_version` vía subconsulta `MAX`, IPC-minimizado),
`insert_version`, `find_version_by_id`, `latest_version`, `list_versions`. 7 tests. SQL puro, sin
ninguna regla de negocio (regla de capas del proyecto).

## 17. Servicio (`src-tauri/src/services/formulations.rs`, nuevo)

`create_formulation` (valida paciente existe/no archivado, `check_episode_assignable`, unicidad de
formulación por proceso, título no vacío; inserta formulación + v1 en una transacción),
`create_new_version` (revalida paciente no archivado + proceso vía `check_episode_assignable` con
el `episode_id` ya guardado; calcula `version_number + 1`), `get_formulation`,
`get_formulation_by_episode`, `list_formulations`, `get_current_version`, `get_version`,
`list_versions`. 14 tests, incluyendo los 8 casos críticos de la aprobación (creación, paciente
inexistente/archivado, proceso inexistente/de otro paciente/cerrado, una formulación por proceso,
título vacío, versionado sin sobrescritura, archivado bloquea/desbloquea nueva versión, proceso
cerrado bloquea nueva versión, reingreso sin copia, listado multi-proceso).

## 18. Comandos Tauri (`src-tauri/src/commands/formulations.rs`, nuevo)

8 comandos delgados: `create_formulation`, `get_formulation`, `get_formulation_by_episode`,
`list_formulations`, `get_current_formulation_version`, `get_formulation_version`,
`list_formulation_versions`, `create_formulation_version`. Registrados en `lib.rs`
(`generate_handler!`). Cero import de `calendar::*`.

## 19. Minimización de IPC

`FormulationSummary` (usado por `list_formulations`) nunca lleva `summaryText` — el contenido
completo de una versión solo viaja al pedir explícitamente `get_current_formulation_version`/
`get_formulation_version`/`list_formulation_versions`. Mismo criterio que `SafetyPlanSummary`
(Fase 12) y `AssessmentAdministrationSummary` (Fase 13).

## 20. Frontend (`src/features/formulation/`, nuevo)

`types.ts`, `sections.ts` (serialización de secciones), `api.ts`, `schema.ts` (validación `zod`),
`FormulationHistory.tsx`, `FormulationTab.tsx` — pestaña "Formulación" de la ficha del paciente,
que distingue "Formulación del proceso actual" (proceso `status === 'activo'`, mismo criterio ya
usado por `ProcessesTab.tsx`) de "Formulaciones de otros procesos" sin duplicar la pestaña
"Procesos". Sin editor de texto enriquecido — secciones editadas con `<textarea>` simples
(componente `Textarea` ya existente). Integrada en `PatientDetailScreen.tsx`
(`SECTIONS_WITH_REAL_CONTENT` ahora incluye `'formulacion'`).

## 21. Privacidad

Verificado por grep en `src/features/formulation/`: cero referencias a `console.*`,
`localStorage`, `sessionStorage`, `navigator.clipboard`, `window.location`, `document.title`,
`analytics`, `telemetry`. Marcador ficticio reservado para la auditoría manual pendiente:
`XYZFASE15FORMULACION` (no pudo ejercitarse en vivo — ver sección 26).

## 22. Google Calendar

Cero referencias a `formulation`/`summary_text`/`hypothesis`/`predisponente`/`precipitante`/
`perpetuante`/`protector` dentro de `src-tauri/src/calendar/` ni de `src/features/agenda/`, y cero
importaciones de `calendar::*` desde ningún archivo de este vertical (verificado por grep).

## 23. Backup / Restore

Sin cambios de diseño (`SCHEMA_V8` es aditiva). Único cambio en `backup/service.rs`: actualización
mecánica de dos literales de test (`schema_version`/`supported_schema_version` de `7` a `8`) —
mismo patrón ya aplicado en la transición V6→V7. Ningún otro archivo de `backup/*` tocado.

## 24. Tests nuevos

25 tests nuevos: 4 en `db::migrations` (`SCHEMA_V8`: columnas presentes desde el arranque,
idempotencia y preservación de datos anteriores, vínculo a un proceso, regla de una formulación
por proceso a nivel de base de datos), 7 en `repositories::formulations`, 14 en
`services::formulations`.

## 25. Total de tests

**695/695 en verde** (670 previos sin cambios + 25 nuevos). Ningún test eliminado ni debilitado.

## 26. Build / Clippy / Lint

| Comando | Resultado |
|---|---|
| `cargo build --release` | limpio |
| `cargo test --release` | 695/695 |
| `cargo clippy --release --all-targets` | 0 warnings |
| `npm run build` | limpio, sin errores TS (primera verificación de los 6 archivos frontend nuevos) |
| `npm run lint` | 24 warnings (23 preexistentes + 1 nuevo en `FormulationTab.tsx`, misma categoría `react(set-state-in-effect)` ya presente en `GoalsTab`/`PaymentsTab`/`SessionsTab`/`AssessmentsTab`/`SafetyPlanTab` — sin categoría nueva), 0 errors |
| `git diff --check` | limpio |

## 27. Prueba manual GUI — bloqueada por el entorno, documentada como pendiente (no inventada)

Se reconstruyó el binario `release` (embebe el frontend recién compilado) y se intentó lanzar la
aplicación en un vault desechable bajo Xvfb, con capturas de pantalla vía `import` (ImageMagick),
siguiendo exactamente el mismo procedimiento que en fases anteriores (Fases 9/10/12, con capturas
reales conservadas en el scratchpad de la sesión).

La ventana se creó correctamente ("Cuaderno Clínico", proceso vivo), pero WebKitGTK no pudo cargar
el contenido: "Could not connect to localhost: Connection refused" — un error de la capa de
renderizado del protocolo personalizado de Tauri, no un error de la aplicación ni del código de
esta fase. Se intentó **tres veces** con configuraciones distintas (lanzamiento directo, con
sesión D-Bus vía `dbus-run-session`, y forzando renderizado por software con
`WEBKIT_DISABLE_COMPOSITING_MODE`/`LIBGL_ALWAYS_SOFTWARE`), con el mismo resultado en las tres.

Los Casos A–I del encargo (crear v1 → actualizar a v2 → verificar v1 intacta en historial → cerrar
proceso bloquea nueva versión → reingreso sin copia → archivar paciente → bloquear/desbloquear
vault → reiniciar la aplicación → marcador de privacidad `XYZFASE15FORMULACION`) **no pudieron
ejercitarse en vivo en este entorno en este momento**. Siguiendo la política explícita del
proyecto de nunca inventar resultados, queda documentado como **pendiente** en
`docs/formulation.md` sección 16, y se agrega al Pre-V1 Manual Acceptance Test acumulado junto con
las pruebas pendientes de fases anteriores documentadas de la misma forma. La cobertura funcional
equivalente a los 9 casos está cubierta por los 21 tests automatizados de repositorio/servicio
(sección 24).

## 28. Archivos nuevos

- `Plan-Fase-15-pendiente-de-aprobacion.md`
- `docs/formulation.md`
- `src-tauri/src/repositories/formulations.rs`
- `src-tauri/src/services/formulations.rs`
- `src-tauri/src/commands/formulations.rs`
- `src/features/formulation/types.ts`
- `src/features/formulation/sections.ts`
- `src/features/formulation/api.ts`
- `src/features/formulation/schema.ts`
- `src/features/formulation/FormulationHistory.tsx`
- `src/features/formulation/FormulationTab.tsx`

## 29. Archivos modificados

- `src-tauri/src/db/migrations.rs` (`SCHEMA_V8` + 4 tests)
- `src-tauri/src/backup/service.rs` (2 literales de test, mecánico)
- `src-tauri/src/repositories/mod.rs` (registro del módulo)
- `src-tauri/src/services/mod.rs` (registro del módulo)
- `src-tauri/src/commands/mod.rs` (registro del módulo)
- `src-tauri/src/lib.rs` (8 comandos en `generate_handler!`)
- `src/features/patients/PatientDetailScreen.tsx` (import, `SECTIONS_WITH_REAL_CONTENT`, render de
  `FormulationTab`, comentario actualizado)
- `docs/ARCHITECTURE.md` (fila de la Fase 15; se aprovechó para agregar también la fila de la
  Fase 14, ausente desde su propio cierre — ver sección 33)

Ningún archivo prohibido (`security/*`, `calendar/*`, `backup/*` más allá de los dos literales
mecánicos, `db/connection.rs`) fue tocado — verificado con `git status --porcelain` sobre esas
rutas antes del commit (sin salida más allá de lo declarado). Ninguno de los archivos con "no
tocar productivamente" (`session_notes`, `payments`, `safety_plans`, `assessments`,
`episode_closures`) fue modificado.

## 30. Tablas / migraciones

`SCHEMA_V8` (sección 6) — la única migración de esta fase. `SCHEMA_V1`–`V7` sin cambios.

## 31. Funcionalidades anteriores afectadas

Ninguna. La vertical es enteramente nueva (repositorio/servicio/comando/frontend inexistentes
antes de esta fase); la única modificación fuera de archivos nuevos y `migrations.rs`/
`backup/service.rs` es la integración de una pestaña que antes mostraba "Próximamente" en
`PatientDetailScreen.tsx`.

## 32. Dependencias nuevas

Cero. Ni en `Cargo.toml`/`Cargo.lock` ni en `package.json`/`package-lock.json` (sin editor de
texto enriquecido — se usa el componente `Textarea` ya existente).

## 33. Riesgos y limitaciones que permanecen

- Prueba manual GUI de esta fase pendiente por una limitación del entorno (sección 27), no por
  falta de intento — se suma a las pendientes acumuladas de fases anteriores.
- Sin integración con Objetivos terapéuticos (sección 14) ni con Formulación Visual (sección 15) —
  ambas explícitamente fuera de alcance.
- Sin archivado de formulaciones individuales — `case_formulations.deleted_at` permanece sin usar,
  documentado en `docs/formulation.md` sección 9, mismo criterio que `raw_responses` en
  `assessment_administrations` (Fase 13).
- Sin exportación/PDF/impresión de una formulación ni de su historial.
- **Deuda documental corregida en este cierre**: `docs/ARCHITECTURE.md` no tenía fila para la
  Fase 14 pese a que su informe de cierre (`Informe-de-cierre-Fase-14-Historial-de-Cierres.md`) ya
  estaba versionado — se agregó junto con la de la Fase 15 (ver sección 29), decisión propia por
  ser puramente aditiva y de bajo riesgo (no reorganiza nada existente, solo completa una fila
  faltante), consistente con la política de la sección 2 de mantener la documentación de proceso
  confiable. Se señala aquí explícitamente en vez de en silencio.

## 34. Decisiones nuevas que requieren tu aprobación

Ninguna decisión de arquitectura, seguridad o modelo de datos quedó pendiente de aprobación en
este cierre — la única decisión estructural de la fase (patient vs. episode, sección 5) ya fue
aprobada antes de escribir código. La corrección documental de la sección 33 se informa, no se
pide aprobación retroactiva para ella por ser puramente aditiva.

## 35. Commit

`8524f31` — `Fase 15: formulación clínica textual versionada`.

## 36. Push

Realizado a `claude/cuaderno-clinico-desktop-udijjq`. Sin force push.

## 37. Git final

- HEAD tras el commit de código: `8524f31` — coincide con
  `origin/claude/cuaderno-clinico-desktop-udijjq`.
- Este informe (y `Plan-Fase-15-pendiente-de-aprobacion.md`, ya incluido en `8524f31`) se
  versionan siguiendo la política de la sección 2 — este archivo se agrega en un commit
  documental separado inmediatamente después, mismo patrón ya usado en la transición Fase
  13→14.
- Migraciones: 1 (`SCHEMA_V8`).
- Dependencias nuevas: 0.

## 38. Estimación de completitud V1 (actualizada)

- **Núcleo clínico** (pacientes, sesiones, objetivos, antecedentes, procesos, cierre, evaluaciones,
  plan de seguridad, formulación): con Formulación completa, el núcleo clínico textual queda en
  ~90% — solo Documentos cifrados permanece como brecha estructural clínica importante.
- **V1 clínica** (núcleo + continuidad + pagos + agenda): ~85%.
- **V1 operacional** (V1 clínica + backup/restore + validación multiplataforma real): sin cambios
  respecto al informe de la Fase 14 en la dimensión que no depende de código — la validación física
  en macOS/Windows/iOS/iPadOS sigue en 0%, la brecha más ancha del proyecto. ~55-60%.
- **Visión multiplataforma completa** (V1 operacional + iOS/iPadOS + sincronización E2EE): sin
  cambios, ~25-30% — ninguna fase reciente toca esta dimensión, correctamente, porque son fases
  específicas futuras (regla 7 de `CLAUDE.md`).

## 39. Recomendación de próxima fase

Con Formulación cerrada, el núcleo de documentación clínica textual del V1 está prácticamente
completo (falta solo Documentos cifrados, bloqueada por una decisión criptográfica previa de
diseño — subclave derivada por HKDF del DEK). Se recomienda **evaluar explícitamente contigo**
antes de decidir la siguiente fase entre: (a) Documentos cifrados (requiere la decisión
criptográfica primero, nunca improvisada), (b) cerrar el ciclo de funcionalidad clínica nueva y
pasar a **cierre operacional de V1** — validación física macOS/Windows (la brecha más ancha y
menos dependiente de código nuevo), auditoría de seguridad end-to-end, y consolidación de la deuda
manual acumulada (todas las pruebas GUI pendientes documentadas a través de las fases, incluyendo
la de esta misma fase). Ver evaluación breve post-fase a continuación.

---

## Auditoría breve post-fase (sin implementar nada)

- **Documentos cifrados**: sigue bloqueada por la decisión criptográfica de diseño (subclave
  derivada por HKDF del DEK, nunca reutilizar el DEK directamente) — no debe empezar sin esa
  decisión aprobada explícitamente primero.
- **Recordatorios internos**: candidata razonable de complejidad baja, sin notificaciones del
  sistema operativo en su V1.
- **Línea temporal**: no necesita tabla nueva (vista agregada de lectura combinando sesiones,
  cierres, formulaciones y evaluaciones por fecha) — de menor prioridad que el cierre operacional.
- **Export**: sin diseño previo en ningún documento del proyecto — requeriría una fase de diseño
  explícita antes de empezar (qué formato, qué alcance, si incluye documentos).
- **Validación macOS/Windows**: sigue siendo la brecha más ancha hacia una "V1 operacional" — nunca
  ejecutada en hardware real en toda la historia del proyecto. Con el núcleo clínico textual ya
  prácticamente completo tras esta fase, esta brecha pesa proporcionalmente más que antes.

**¿Seguir agregando clínica o pasar a cierre operacional de V1?** Recomendación: con Formulación
cerrada, el V1 clínico textual (sin Documentos cifrados, que depende de una decisión previa
distinta) está funcionalmente completo. Se recomienda priorizar el **cierre operacional de V1**
(validación macOS/Windows real, consolidación de las pruebas manuales pendientes acumuladas,
auditoría de seguridad end-to-end) antes de sumar clínica nueva — pero esta es una recomendación,
no una decisión tomada: la fase siguiente se define contigo explícitamente, como en cada cierre de
fase anterior.

Ninguna de las líneas anteriores se implementa en este cierre — a la espera de tu aprobación de
este informe, según la regla permanente del proyecto.
