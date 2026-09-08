# Informe de cierre — Fase 13: Evaluaciones Clínicas / Psicométricas

Sigue el formato obligatorio de `CLAUDE.md` sección 10, con las secciones adicionales pedidas para
esta fase (auditoría de esquema previa, prueba manual, evaluación de próximos verticales). Cubre
también, como Parte I independiente ya cerrada, el micro-hardening de Fase 12 que sirvió de
baseline (commit `684f048`) antes de empezar Fase 13.

---

## Parte I (ya cerrada) — Micro-hardening de Plan de Seguridad

Resumen breve; ver el commit `684f048` para el detalle completo.

1. **Qué se implementó**: `update_draft`/`confirm_draft`/`add_contact`/`update_contact`/
   `delete_contact` ahora re-comprueban el archivado del paciente en cada llamada
   (`require_editable_draft`), no solo al crear el borrador. `discard_draft` queda deliberadamente
   exceptuado (decisión de producto confirmada vía `AskUserQuestion`). "Actualizar plan" pasa a
   copiar, atómicamente en el backend (`create_draft_from_current`), contenido narrativo +
   `reviewed_at` + todos los contactos (IDs nuevos, sin compartir fila con la versión anterior).
2. **Archivos modificados**: `src-tauri/src/repositories/safety_plans.rs`,
   `src-tauri/src/services/safety_plans.rs`, `src-tauri/src/commands/safety_plans.rs`,
   `src-tauri/src/lib.rs`, `src/features/safety-plan/api.ts`, `src/features/safety-plan/SafetyPlanTab.tsx`,
   `docs/safety-plan.md`.
3. **Archivos nuevos**: ninguno.
4. **Migraciones**: ninguna — confirmado que no hacía falta antes de empezar (era la instrucción
   explícita: si hiciera falta, detenerse).
5. **Funcionalidad anterior afectada**: ninguna fuera del dominio de Plan de Seguridad; el resto de
   verticales no se tocó.
6. **Tests**: 631/631 en verde (616 previos + 15 nuevos).
7. **Pruebas manuales**: no pudieron ejecutarse en este entorno remoto (ver sección "Limitación de
   entorno" más abajo) — verificación apoyada en los 15 tests automatizados nuevos.
8. **Riesgos/limitaciones**: la verificación visual de la UI (que "Actualizar plan" copia contactos
   en vivo, que los botones se deshabilitan correctamente al archivar) queda pendiente de
   confirmación manual — instrucciones puntuales en la sección correspondiente más abajo.
9. **Decisiones que requirieron aprobación**: la única — descartar un borrador con el paciente
   archivado — se resolvió vía `AskUserQuestion` antes de codificar (se eligió permitirlo).
10. **Commit estable**: `684f048` — `fix: reforzar versionado y archivado del plan de seguridad`.

---

## Parte II — Fase 13: Evaluaciones Clínicas / Psicométricas

### 1. Qué se implementó

Primer vertical productivo sobre `assessment_instruments`/`assessment_administrations` (presentes
sin usar desde `SCHEMA_V1`, Fase 1.3):

- **Catálogo de instrumentos** que la propia usuaria mantiene: nombre, abreviatura, descripción,
  categoría — todo texto libre, sin ninguna lista cerrada. Sin eliminación (solo edición de
  metadatos): un instrumento con administraciones asociadas no puede borrarse (`ON DELETE
  RESTRICT`), y no había caso de uso pedido para borrar uno sin usar.
- **Registro de administraciones**: instrumento, fecha, vínculo opcional a un proceso terapéutico
  (`episode_id`, nuevo en esta fase), contexto (`ingreso`/`seguimiento`/`alta`, ya existente),
  puntaje total (acepta negativos, para escalas estandarizadas), subescalas (JSON de texto libre,
  validado solo sintácticamente, nunca interpretado), interpretación redactada por la profesional.
- **Evolución longitudinal** de un mismo instrumento para un paciente — nunca mezcla instrumentos
  distintos — mostrada como **tabla simple**, no como gráfico (ver punto 9).
- **Regla de copyright no negociable**, aplicada en todo el dominio: nunca se almacena ni reproduce
  el contenido real de un instrumento. `raw_responses` (columna legado de `SCHEMA_V1`) queda
  explícitamente sin usar y documentada.
- Archivar/restaurar administraciones (soft delete, mismo patrón que el resto del dominio).
- Auditoría de esquema previa, obligatoria antes de escribir código — ver punto 2.

### 2. Auditoría de esquema previa y migración `SCHEMA_V7`

Antes de escribir cualquier código de Fase 13 se auditó el esquema real de
`assessment_instruments`/`assessment_administrations` (sin cambios desde `SCHEMA_V1`) contra los
requisitos de esta fase. Se documentó el resultado completo en
`Plan-Fase-13-pendiente-de-aprobacion.md` **antes** de tocar código, y se esperó aprobación
explícita antes de continuar (instrucción "si una migración ES necesaria, DETENTE y explica...").

Se encontraron dos huecos reales:

- `assessment_instruments` no tenía `abbreviation` ni `category`.
- `assessment_administrations` no tenía ningún vínculo opcional a un proceso terapéutico — a
  diferencia de `sessions`/`therapeutic_goals`, que sí lo recibieron en `SCHEMA_V4`.

Se presentaron tres opciones (migración completa / solo catálogo / ninguna) con riesgos y
alternativas explícitas. La usuaria eligió la migración completa. `SCHEMA_V7` (puramente aditiva,
`SCHEMA_V1`–`V6` intactos) agrega exactamente esas tres columnas más un índice, siguiendo el mismo
patrón ya usado en `SCHEMA_V4`. Ver `docs/assessments.md` sección 3 para el detalle SQL completo.

### 3. Qué archivos se modificaron

- `src-tauri/src/db/migrations.rs` (nueva `SCHEMA_V7` + 3 tests de migración).
- `src-tauri/src/repositories/mod.rs` / `src-tauri/src/services/mod.rs` / `src-tauri/src/commands/mod.rs` /
  `src-tauri/src/lib.rs` (registro del módulo/comandos nuevos).
- `src-tauri/src/backup/service.rs` (dos literales de test actualizados de `6` a `7` — mismo patrón
  mecánico que en la transición V4→V5→V6; `current_app_schema_version()` calcula la versión
  dinámicamente, así que no hubo cambio de diseño).
- `src/features/patients/PatientDetailScreen.tsx` (pestaña "Evaluaciones" deja de mostrar
  "Próximamente").
- `docs/ARCHITECTURE.md` (fila de la Fase 13 en la tabla de fases, fila del micro-hardening,
  corrección de la línea desactualizada sobre Recharts en la sección "Evaluaciones").

### 4. Qué archivos nuevos se crearon

- `src-tauri/src/repositories/assessments.rs` (13 tests).
- `src-tauri/src/services/assessments.rs` (21 tests).
- `src-tauri/src/commands/assessments.rs`.
- `src/features/assessments/types.ts`, `api.ts`, `schema.ts`, `AssessmentsTab.tsx`.
- `docs/assessments.md`.
- `Plan-Fase-13-pendiente-de-aprobacion.md` (documento de la auditoría de esquema, conservado como
  registro histórico de la decisión).

### 5. Qué tablas/migraciones se modificaron

`SCHEMA_V7`: `ALTER TABLE assessment_instruments ADD COLUMN abbreviation TEXT` / `ADD COLUMN
category TEXT`; `ALTER TABLE assessment_administrations ADD COLUMN episode_id TEXT REFERENCES
treatment_episodes(id) ON DELETE SET NULL` + `CREATE INDEX
idx_assessment_administrations_episode`. Ninguna tabla existente pierde ninguna columna; ninguna
columna existente cambia de tipo ni de restricción. Ninguna migración anterior (`V1`–`V6`) se
modificó.

### 6. Qué funcionalidades anteriores fueron afectadas

Ninguna. `assessment_instruments`/`assessment_administrations` no tenían ninguna vertical
construida encima — cero riesgo de regresión sobre código existente. Verificado explícitamente que
`src-tauri/src/calendar/*` no se tocó (`grep` sin resultados) y que ningún archivo nuevo importa
`calendar::*`.

### 7. Qué tests se ejecutaron y sus resultados

- `cargo test --lib` / `cargo test --release`: **668/668 en verde** (631 previos del
  micro-hardening + 37 nuevos: 13 en `repositories::assessments`, 21 en `services::assessments`, 3
  en `db::migrations` para `V7`).
- `cargo clippy --all-targets` / `cargo clippy --release --all-targets`: sin advertencias.
- `cargo build` / `cargo build --release`: limpios.
- `npm run build`: limpio, sin errores de TypeScript.
- `npm run lint`: 23 warnings (22 preexistentes de fases anteriores + 1 nuevo en
  `AssessmentsTab.tsx`, de la misma categoría `react(set-state-in-effect)` ya presente en todos los
  demás componentes de pestaña — sin categoría nueva). Cero errores.
- `git diff --check`: limpio, sin problemas de espacios en blanco.

### 8. Qué pruebas manuales se realizaron

**Ninguna pudo ejecutarse en este entorno.** Limitación de entorno explícita, no oculta: este
entorno remoto en la nube bloqueó, por su clasificador de permisos, tanto mover el vault existente
a un lado como lanzar el binario compilado en segundo plano bajo Xvfb — el mecanismo usado en fases
anteriores (Fase 6.1 en adelante) para las pruebas manuales de GUI. Se intentó dos veces con
enfoques distintos (mover el directorio de datos existente; usar `XDG_DATA_HOME` para un vault
aislado) y ambos fueron denegados por el clasificador de la sesión.

La verificación de esta fase se apoya exclusivamente en: 37 tests automatizados nuevos que cubren
exactamente los escenarios de negocio (creación/edición de instrumentos y administraciones,
duplicados, paciente archivado, vínculo a proceso terapéutico válido/inválido, JSON de subescalas
válido/inválido, aislamiento entre instrumentos en la evolución longitudinal, idempotencia y
preservación de datos de la migración `V7`); build/clippy/lint limpios en modo debug y release.

**Instrucciones de verificación manual para ejecutar en tu máquina** (vault desechable, nunca el
vault real):

1. Renombra o mueve el directorio de datos de la app a un lado (macOS:
   `~/Library/Application Support/com.jpcaamano.cuadernoclinico`; Linux:
   `~/.local/share/com.jpcaamano.cuadernoclinico`) para partir de un vault limpio, o usa una copia
   de la app con un identificador de bundle distinto.
2. `npm run tauri dev` (o `npm run tauri build` y ejecutar el binario) para levantar la aplicación
   real.
3. Crear un vault nuevo con contraseña de prueba y un paciente ficticio (marcador de privacidad
   sugerido: **`XYZFASE13EVALUACIONES`** en el campo de notas de una administración, para luego
   confirmar que no aparece fuera del vault cifrado).
4. **Caso A — Catálogo**: abrir "Evaluaciones" → "Catálogo de instrumentos" → crear un instrumento
   (ej. "BDI-II", abreviatura "BDI-II", categoría "Depresión") → confirmar que aparece en la lista →
   editarlo (cambiar la categoría) → confirmar que el cambio persiste.
5. **Caso B — Registrar evaluación**: "Registrar evaluación" → seleccionar el instrumento → fecha →
   contexto "Ingreso" → puntaje total `18` → subescalas `{"cognitivo": 10, "somatico": 8}` →
   interpretación con el marcador `XYZFASE13EVALUACIONES` → guardar → confirmar que aparece en el
   listado con el puntaje visible pero sin mostrar la interpretación completa en la fila (verificar
   que la minimización de IPC se refleja también visualmente: abrir la evaluación para ver la
   interpretación completa).
6. **Caso C — Instrumento nuevo desde el formulario**: al registrar una segunda evaluación, usar el
   botón "Nuevo" junto al selector de instrumento sin cerrar el formulario, crear un instrumento
   distinto, confirmar que queda seleccionado automáticamente.
7. **Caso D — Vínculo a proceso terapéutico**: con un Proceso activo ya creado para el paciente,
   registrar una evaluación vinculándola a ese proceso; confirmar que se guarda correctamente.
8. **Caso E — Evolución longitudinal**: registrar una segunda administración del mismo instrumento
   en una fecha distinta con un puntaje distinto → "Ver evolución" → confirmar que la tabla muestra
   ambas fechas ordenadas de la más antigua a la más reciente, y que un instrumento distinto no
   aparece mezclado.
9. **Caso F — Editar**: editar la primera administración (cambiar puntaje/interpretación) →
   confirmar que se refleja en el listado y en la evolución.
10. **Caso G — Archivar/Restaurar**: archivar una administración → confirmar que desaparece de
    "Activas" y aparece en "Archivadas" → restaurar → confirmar que vuelve.
11. **Caso H — Paciente archivado**: archivar el paciente → confirmar que "Registrar evaluación"
    queda deshabilitado con el aviso explicativo, pero que "Editar"/"Ver evolución" de evaluaciones
    ya existentes siguen funcionando → restaurar al paciente → confirmar que "Registrar evaluación"
    vuelve a estar disponible.
12. **Caso I — Nombre de instrumento duplicado**: intentar crear un instrumento con un nombre ya
    usado → confirmar el mensaje de error claro ("ya existe un instrumento llamado...").
13. **Caso J — Backup/Restore**: Ajustes → Respaldo → crear un backup → modificar/archivar una
    evaluación → Restaurar ese backup → confirmar que el estado vuelve exactamente al momento del
    backup (incluyendo el catálogo de instrumentos y las administraciones).
14. **Caso K — Auditoría de privacidad**: con la app cerrada, buscar el marcador
    `XYZFASE13EVALUACIONES` en todo el disco fuera del archivo del vault (`grep -r
    XYZFASE13EVALUACIONES` sobre el directorio de datos de la app, excluyendo `vault.db`) —
    confirmar cero coincidencias fuera del archivo cifrado. Revisar también que ningún log de la
    aplicación (consola de `tauri dev`) haya impreso el marcador ni ningún otro contenido clínico.
15. Cerrar la app, reabrirla, desbloquear, confirmar que todo lo anterior persiste.
16. Al terminar, conservar el vault de prueba bajo un nombre de respaldo (nunca eliminarlo por si
    hace falta revisar algo después) — no reutilizar el vault real para estas pruebas.

### 9. Qué riesgos o limitaciones permanecen

- **Prueba manual de GUI no ejecutada en este entorno** (sección 8) — es la limitación más
  relevante de este cierre; las instrucciones de arriba permiten cerrarla del lado de la usuaria.
- No hay eliminación de instrumentos del catálogo (por diseño — ver `docs/assessments.md` sección
  6); uno creado por error queda editable pero no se puede quitar de la lista.
- No hay exportación/PDF/impresión de una evaluación ni de su evolución.
- La evolución longitudinal es una tabla, no un gráfico — decisión conservadora explícita, no una
  limitación técnica (ver punto 10 de este informe / sección 10 de `docs/assessments.md`).
- `raw_responses` permanece en el esquema como columna legado sin uso — si una fase futura quisiera
  reactivarla, debe evaluarse explícitamente contra la regla de copyright, nunca en silencio.

### 10. Qué decisiones nuevas requieren aprobación

Ninguna pendiente en este cierre — la única decisión de esquema (`SCHEMA_V7`) ya se resolvió
explícitamente antes de escribir código (sección 2), y la decisión de no instalar una librería de
gráficos fue la aplicación directa de tu instrucción explícita para esta fase, no una decisión
nueva que requiera aprobación adicional.

### 11. El commit que representa el estado estable de la fase

`bd6d9b1` — `Fase 13: evaluaciones clínicas y seguimiento psicométrico`, pusheado a
`claude/cuaderno-clinico-desktop-udijjq`. Árbol de trabajo limpio, `HEAD` = `origin/...` al momento
de este informe.

---

## Evaluación breve de próximos verticales (sin implementar nada)

Ocho candidatos evaluados brevemente, con una recomendación de orden. Ninguno se implementa en este
cierre.

1. **Formulación clínica** (`case_formulations`/`formulation_versions`/`formulation_nodes`/
   `formulation_edges`) — ya en el esquema desde `SCHEMA_V1`, sin vertical construida. Complejidad
   alta (editor visual de nodos/conexiones), pero es la única sección de la ficha del paciente que
   sigue en "Próximamente" con datos clínicos estructurales importantes detrás.
2. **Documentos** (`documents`) — cifrado de documentos individuales ya es un principio de
   seguridad no negociable del proyecto (regla 4 de `CLAUDE.md`) pero no implementado; probablemente
   el vertical de mayor impacto percibido para el uso diario (adjuntar informes, consentimientos).
3. **Biblioteca profesional** (`library_resources`/`library_tags`) — utilidad clara pero menor
   urgencia clínica; depende de que Documentos exista primero (`file_document_id`).
4. **Recordatorios** (`reminders`) — referencia polimórfica ya con limitación documentada
   (`docs/db-schema.md` punto 9); complejidad media, valor operativo alto (avisos de revisión de
   plan de seguridad, tareas vencidas, etc.).
5. **Historial de cierres de proceso en la UI** — limitación ya documentada explícitamente desde la
   Fase 11 (`episodeClosuresApi.listHistory` existe y tiene tests, pero sin pantalla). Esfuerzo
   bajo, cierra una deuda ya conocida en vez de abrir código nuevo.
6. **Exportación** — mencionada como "futura" en varios documentos (Plan de Seguridad, Evaluaciones,
   Backup). Depende de decidir un formato (PDF/HTML) y no tiene tabla propia — es una capa sobre
   datos ya existentes.
7. **Línea temporal del paciente** (sección de navegación ya reservada, sin tabla propia) —
   agregación de lectura sobre entidades ya existentes (sesiones, pagos, evaluaciones, plan de
   seguridad, cierres), sin necesidad de esquema nuevo.
8. **Sincronización multi-dispositivo (Sync)** — explícitamente fuera de alcance de cualquier fase
   actual por regla permanente (`CLAUDE.md` regla 6): requiere su propia fase dedicada con
   E2EE/gestión de dispositivos/resolución de conflictos, nunca puede improvisarse dentro de otra
   fase.

**Orden recomendado**: Documentos (2) → Formulación clínica (1) → Recordatorios (4) → Historial de
cierres en la UI (5, esfuerzo bajo) → Línea temporal (7) → Biblioteca profesional (3) →
Exportación (6). Sync (8) queda fuera de este orden, como su propia fase futura dedicada cuando
corresponda.

## Estimación actualizada de completitud de V1

Verticales de datos clínicos con tabla propia en `SCHEMA_V1` ya construidos de punta a punta:
Pacientes, Agenda/Google Calendar, Sesiones/Notas, Objetivos, Antecedentes, Pagos, Continuidad
(preparación + tareas), Procesos terapéuticos + Cierre/Alta, Plan de Seguridad, Evaluaciones (esta
fase) — 10 de 13 verticales de dominio clínico identificadas en `ARCHITECTURE.md` sección 4, más
Backup/Restore como capacidad transversal ya completa. Quedan sin construir: Formulación clínica,
Documentos, Biblioteca profesional, Recordatorios (4 de 13). Estimación aproximada de avance de V1:
**~75-80%** del alcance de dominio clínico descrito en `ARCHITECTURE.md`, sin contar la
multiplataforma (iOS/iPadOS, Sync), que es explícitamente una fase futura separada, no parte del
V1 de escritorio.
