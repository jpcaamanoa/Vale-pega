# Informe de cierre — Fase de continuación post-Fase 19

Continuación del desarrollo de Cuaderno Clínico a partir de la validación real en Windows del
19/09/2026. Estructura: Fase A (auditoría, ya cerrada en una sesión anterior) → **Fase B**
(6 ítems confirmados, implementados) → **Fase C** (4 ítems de investigación, sin implementar) →
**Fase D** (regresión completa) → **Fase E** (este informe).

---

## 1. Opinión crítica del estado actual

Cuaderno Clínico sigue siendo una aplicación de escritorio local-first técnicamente sólida:
cifrado en capas (SQLCipher + envelope AES-256-GCM por documento, Argon2id para la contraseña
maestra), migraciones versionadas disciplinadas, y una arquitectura repository→service→command→
IPC→React consistente en 20 verticales funcionales. Esta fase agregó dos piezas de peso
(rediseño completo del Plan de Seguridad, Biblioteca global nueva) sin tocar ninguna de las
decisiones de seguridad ya aprobadas.

Dicho esto, con honestidad: **esta build sigue sin estar lista para datos clínicos reales**, por
tres razones concretas, no genéricas:

1. **La validación GUI de esta fase es incompleta.** El entorno de desarrollo Linux disponible no
   tiene compositor real (WebKitGTK bajo Xvfb sin `X11 EGL`/`DRI3` funcional) — los eventos
   sintéticos de scroll y navegación por teclado (Tab) no llegan de forma fiable al WebView. Se
   verificó real y visualmente (capturas de pantalla) el arranque, la creación de un vault
   desechable, el desbloqueo, y la navegación básica entre Inicio/Pacientes — pero **no** se
   validó interactivamente el flujo completo de los seis pasos del Plan de Seguridad rediseñado
   ni el flujo completo de Biblioteca (asociar/desasociar, abrir/previsualizar/exportar). Esto ya
   ocurrió en las Fases 15/16 previas por la misma limitación de entorno — no es nuevo, pero
   tampoco deja de ser una brecha real que solo se puede cerrar en Windows o macOS reales.
2. **El "No responde" reportado en Windows no fue medido, solo investigado por lectura de
   código** (instrucción explícita: sin mediciones reales en Windows disponibles desde este
   entorno). La hipótesis más plausible identificada (§7) requiere confirmación empírica.
3. Quedan cuatro decisiones de producto genuinas sin resolver, todas señaladas explícitamente en
   este informe (§9) para que las decida la usuaria — Línea temporal, borrado permanente de
   pacientes, UX de Google Calendar, y la causa raíz confirmada del "No responde".

Lo que sí se puede afirmar con evidencia real: 841/841 tests de backend en verde, `cargo clippy`
limpio, `npm run build`/`npm run lint` limpios, y una arquitectura de Biblioteca que reutiliza al
100% la infraestructura de cifrado ya auditada de Documentos — sin ninguna superficie de ataque
nueva.

## 2. Hallazgos de la auditoría (Fase A, resumen)

(Detalle completo ya entregado en una sesión anterior — resumen aquí para que este informe sea
autocontenido.)

- El modal de Formulación cortaba contenido fuera de la ventana visible por el antipatrón CSS
  `items-center` + `overflow-y-auto` en el contenedor sin `max-height` en el hijo.
- El Plan de Seguridad (Fase 12) cubría los conceptos de Stanley & Brown en cuatro campos de texto
  libre más una tabla de contactos genérica — sin distinguir "personas de distracción" (Paso 3) de
  "personas a las que pedir ayuda explícitamente" (Paso 4), y sin campos propios para
  instituciones de crisis (Paso 5).
- Estadísticas agrupaba cualquier región/comuna con menos de 3 pacientes como "Otras" también en
  la pantalla privada interna — mezclando una protección de reidentificación pensada para un
  reporte compartido futuro con la vista de trabajo diario de la propia profesional.
- "Factores de riesgo" en Antecedentes clínicos mostraba y pedía editar JSON crudo directamente en
  un `<textarea>`.
- "Objetivos terapéuticos" combinaba "Volver a la ficha" y "Guardar cambios" de forma ambigua, sin
  aviso de cambios sin guardar.
- No existía ninguna Biblioteca global — solo Documentos por paciente (Fase 16); `library_resources`
  llevaba declarada sin usar desde `SCHEMA_V1`.
- Línea temporal, borrado permanente de pacientes, y la UX de Google Calendar quedaron
  identificados como decisiones de producto pendientes, no como defectos a corregir.

## 3. Cambios exactos realizados

### Backend (Rust)

- **`src-tauri/src/db/migrations.rs`**: `SCHEMA_V11` (rediseño Plan de Seguridad) y `SCHEMA_V12`
  (Biblioteca) — ver §4.
- **`src-tauri/src/repositories/safety_plans.rs`**: `SafetyPlanContact` ampliado (`address`,
  `service_phone`, `is_emergency_contact`, `is_crisis_service`); struct nuevo
  `SafetyPlanListItem` + CRUD (`insert_list_item`/`find_list_item_by_id`/`list_items_by_plan`/
  `update_list_item`/`delete_list_item`).
- **`src-tauri/src/services/safety_plans.rs`**: `CREATABLE_CONTACT_TYPES` (excluye
  `support_person`); `validate_contact_input` ahora recibe el conjunto de tipos permitido;
  `create_draft_from_current` copia también los ítems de lista; nuevas funciones
  `list_items`/`add_item`/`update_item`/`delete_item`.
- **`src-tauri/src/commands/safety_plans.rs`**: 4 comandos nuevos (`list_safety_plan_items`,
  `add_safety_plan_item`, `update_safety_plan_item`, `delete_safety_plan_item`).
- **`src-tauri/src/repositories/documents.rs`**: `NewDocumentRow.patient_id` de `&str` a
  `Option<&str>` (cambio mínimo, retrocompatible — ver §8 sobre por qué se decidió sin detenerse a
  preguntar).
- **`src-tauri/src/services/documents.rs`**: `guess_mime_from_extension`/`sha256_hex`/
  `wrapped_key_to_columns` marcadas `pub(crate)` para reutilización desde Biblioteca; llamada a
  `create_document` actualizada a `Some(...)`.
- **`src-tauri/src/repositories/library.rs`** (nuevo): acceso a datos de `library_resources` y
  `library_resource_patients`.
- **`src-tauri/src/services/library.rs`** (nuevo): reglas de negocio de la Biblioteca —
  crear/listar/editar/archivar-restaurar recurso, descifrar contenido, asociar/desasociar
  paciente.
- **`src-tauri/src/commands/library.rs`** (nuevo): 14 comandos Tauri.
- **`src-tauri/src/backup/service.rs`**: 4 literales de versión de esquema actualizados
  (10→11→12, mecánico); 2 tests nuevos dedicados a Biblioteca.
- **`src-tauri/src/repositories/mod.rs`**, **`src-tauri/src/services/mod.rs`**,
  **`src-tauri/src/commands/mod.rs`**, **`src-tauri/src/lib.rs`**: registro de los módulos y
  comandos nuevos.

### Frontend (TypeScript/React)

- **`src/features/formulation/FormulationTab.tsx`**: fix de scroll (`items-start` +
  `max-h-[85vh] overflow-y-auto` en el modal).
- **`src/features/safety-plan/{types,api,schema}.ts`**: tipos/API/schema ampliados para los seis
  pasos.
- **`src/features/safety-plan/SafetyPlanTab.tsx`**: reescrito en seis secciones explícitas, con
  `ItemListEditor` (listas agregables con edición en línea) reemplazando los antiguos `<Textarea>`
  para los planes nuevos, y paneles "heredado" para el contenido narrativo de planes anteriores al
  rediseño.
- **`src/features/statistics/{api,StatisticsScreen}.tsx`**: `suppressSmallCategories` explícito
  (siempre `false` desde la pantalla privada); todas las regiones/comunas visibles con N y %;
  clic en región filtra sus comunas.
- **`src/components/ui/TagListField.tsx`** (nuevo): chips agregables/eliminables genéricos.
- **`src/features/clinical-profile/riskFlags.ts`** (nuevo): `parseRiskFlags`/`serializeRiskFlags`.
- **`src/features/clinical-profile/ClinicalProfileTab.tsx`**, **`schema.ts`**: "Factores de
  riesgo" migrado de `<Textarea>` de JSON crudo a `TagListField`.
- **`src/features/goals/GoalDetailScreen.tsx`**: navegación y guardado separados, barra fija de
  guardado cuando hay cambios sin guardar, aviso `beforeunload` + confirmación al salir.
- **`src/features/library/`** (nuevo): `types.ts`, `api.ts`, `LibraryScreen.tsx` (pantalla global),
  `PatientLibrarySection.tsx` (sección en la ficha del paciente).
- **`src/App.tsx`**, **`src/app/Layout.tsx`**, **`src/features/patients/PatientDetailScreen.tsx`**:
  ruta `/library`, enlace de navegación, sección "Biblioteca" en la ficha del paciente.

### Documentación

- **`docs/safety-plan.md`** §20 (rediseño de seis pasos).
- **`docs/library.md`** (nuevo, diseño completo de la Biblioteca).
- **`docs/ARCHITECTURE.md`**: dos filas nuevas en la tabla de fases.

## 4. Migraciones realizadas

| Migración | Tipo | Contenido |
| --- | --- | --- |
| `SCHEMA_V11` | Aditiva + un rebuild de tabla sin transformar datos | `safety_plan_list_items` (nueva); `safety_plan_contacts` reconstruida para ampliar `contact_type` (+`distraction_person`/`help_contact`) y agregar `address`/`service_phone`/`is_emergency_contact`/`is_crisis_service` — todas las filas existentes copiadas sin transformar, ningún contacto se reclasifica automáticamente |
| `SCHEMA_V12` | Puramente aditiva | `library_resource_patients` (relación N:M biblioteca↔paciente) |

Ambas siguen el patrón `rusqlite_migration` ya establecido (`M::up(...).foreign_key_check()`),
verificadas con tests de: base nueva llega a la versión correcta, base migrada desde una versión
anterior preserva datos existentes, cascadas `ON DELETE`, rechazo de valores fuera del `CHECK`, e
idempotencia (reaplicar no rompe nada).

**Sin ninguna migración destructiva.** Ningún dato preexistente se pierde, se sobrescribe ni se
reclasifica automáticamente en ningún punto de esta fase.

## 5. Tests agregados

| Área | Tests nuevos |
| --- | --- |
| `db::migrations` (V11) | 6 |
| `db::migrations` (V12) | 6 |
| `repositories::safety_plans` | ampliados con los 4 campos nuevos de contacto (pre-existentes actualizados, no nuevos en conteo) |
| `services::safety_plans` | 65 tests totales en el módulo (18 nuevos: ítems de lista, compatibilidad `support_person`, copia de ítems en "Actualizar plan") |
| `repositories::library` | 12 |
| `services::library` | 13 |
| `backup::service` (Biblioteca) | 2 (inclusión en manifiesto, restauración completa con asociación) |
| `backup::service`/`db::migrations` (literales de versión) | 6 actualizados mecánicamente (10→12) |

**Total: 73 tests de backend nuevos** desde el inicio de esta fase (768 → 841). Frontend: sin
tests automatizados nuevos porque **este proyecto no tiene un framework de tests de frontend
instalado** (sin `vitest`/`testing-library`, sin script `test` en `package.json`) — constraint
preexistente, no introducida por esta fase. La validación de `parseRiskFlags`/
`serializeRiskFlags` y del resto de la UI nueva se hizo por lectura de código, `tsc -b` (chequeo
de tipos estricto) y verificación visual limitada (ver §1).

## 6. Resultados completos de tests/lint/build

```
cargo build --release        → limpio, sin warnings
cargo test --release         → 841 passed; 0 failed; 0 ignored
cargo clippy --release --all-targets → limpio, sin warnings
npm run build                → limpio (tsc -b + vite build)
npm run lint (oxlint)        → 28 warnings, cero errores — todas las mismas 2 categorías
                                preexistentes (react/incompatible-library, react/set-state-in-effect)
                                ya toleradas en 15+ archivos antes de esta fase; los 4 archivos
                                nuevos de esta fase (SafetyPlanTab.tsx, LibraryScreen.tsx,
                                PatientLibrarySection.tsx, api/types/schema) no introducen ninguna
                                categoría nueva de warning
```

## 7. Riesgos residuales

1. **Validación GUI incompleta** (ver §1) — el flujo completo de los seis pasos del Plan de
   Seguridad y de Biblioteca (asociar/desasociar/abrir/previsualizar/exportar) no se ejercitó
   interactivamente de extremo a extremo en este entorno. Mitigación: checklist manual completo
   en §11, a ejecutar en Windows real antes de cualquier uso con datos reales.
2. **"No responde" sin causa raíz confirmada** — hipótesis identificada por lectura de código
   (§ítem C.4 más abajo), no medida. No se aplicó ningún cambio especulativo.
3. **Biblioteca es una feature nueva sin horas de uso real** — 73 tests de backend cubren la
   lógica de negocio, pero ninguna cantidad de tests reemplaza el uso real con archivos y
   asociaciones reales en Windows/macOS.
4. **Migración `SCHEMA_V11`/`V12` nunca se ejecutó sobre un vault real con datos clínicos
   reales** — solo sobre vaults de prueba con datos ficticios construidos en los tests. El primer
   vault real que pase por esta migración debería respaldarse antes (Ajustes → Respaldo).

## 8. Qué se decidió deliberadamente NO cambiar

- **Ninguna de las áreas explícitamente protegidas** (SQLCipher, `bundled-sqlcipher-vendored-
  openssl`, Argon2id, AES-GCM, envoltura de claves, HKDF, recuperación de arranque, restauración
  atómica, single-instance, backup/restore, bloqueo del vault, separación admin/clínico, modelo de
  IDs UUID, arquitectura repository→service→command→IPC→React) se tocó en absoluto.
- **`services::documents::create_document`** conserva exactamente su comportamiento — el único
  cambio fue ensanchar el tipo de un campo interno (`NewDocumentRow.patient_id`: `&str` →
  `Option<&str>`) para poder reutilizar la infraestructura de cifrado desde Biblioteca sin
  duplicarla. **Se decidió sin detenerse a preguntar** porque: (a) la columna SQL ya era nullable
  desde `SCHEMA_V1`, nunca se cambió ningún `CHECK` ni ninguna restricción; (b) `Document.patient_id`
  en Rust ya era `Option<String>` en el lado de lectura — solo el lado de escritura no lo reflejaba
  todavía; (c) es exactamente lo que pedía la instrucción explícita "reutiliza EXACTAMENTE la
  infraestructura de cifrado existente, sin segunda implementación"; (d) es mecánico,
  retrocompatible, y los 39 tests existentes de Documentos pasan sin ningún cambio de resultado.
  Se documenta aquí con el detalle completo por transparencia, no porque se considere una zona
  gris real.
- **`library_tags`/`library_resource_tags`** siguen sin usarse — quedan disponibles para una fase
  futura si se decide exponer etiquetas.
- **Ningún borrado físico de paciente** — se investigó (§C.2) pero no se implementó nada.
- **Ninguna optimización de arranque/desbloqueo** — Argon2id sigue con los mismos parámetros
  (RFC 9106, ~64 MiB/3 iteraciones/4 hilos), nunca debilitado.
- **UX de Google Calendar sin cambios** — se auditó (§C.3) pero no se tocó el manejo de
  OAuth/secretos.
- **Línea temporal sigue mostrando "Próximamente"** — sin decisión de producto tomada por mí.

## 9. Decisiones que requieren aprobación de la usuaria

### C.1 — Línea temporal

Pestaña "Línea temporal" en la ficha del paciente muestra únicamente "Próximamente." desde su
creación. Tu inclinación expresada es ocultarla. Opciones:

- **A. Ocultar la pestaña** de `SECTIONS` hasta tener una implementación real — la navegación
  vuelve a mostrar solo pestañas con contenido real.
- **B. Dejarla visible tal como está** (placeholder "Próximamente") — comunica que la funcionalidad
  está planeada.
- **C. Implementarla ahora** — requeriría diseño propio (qué eventos agrega, de qué verticales,
  orden cronológico, filtros) — fuera del alcance de "solo ocultar" y de esta fase de
  investigación.

Recomendación: A, si el objetivo es que la navegación solo muestre lo que existe hoy. Pendiente de
tu confirmación explícita antes de tocar código.

### C.2 — Archivo vs. borrado permanente de pacientes

**Auditoría del estado actual** (sin ningún cambio de código):

- No existe **ninguna** función de borrado físico de un paciente en todo el código — confirmado
  por grep exhaustivo en `repositories/`, `services/`, `commands/` (cero coincidencias de
  `hard_delete`/`DELETE FROM patients`). El propio comentario del código lo documenta
  explícitamente: *"no hay ninguna función `hard_delete` / `DELETE FROM patients` en todo el
  código, así que un borrado físico no es posible a través de una operación normal de la
  aplicación."*
- "Archivar"/"Restaurar" (`archive_patient`/`restore_patient`) son soft delete puro:
  `patients.deleted_at` se marca/desmarca, la fila nunca se toca de otra forma.
- Si alguna vez se intentara un borrado físico (p. ej. manualmente contra la base, fuera de la
  aplicación), el propio esquema SQL ya actúa como red de seguridad estructural: **9 tablas**
  (`patient_clinical_profile`, `sessions`, `case_formulations`, `therapeutic_goals`,
  `assessment_administrations`, `payments`, `patient_prep_notes`, `therapy_tasks`,
  `treatment_episodes`, `safety_plans`) usan `ON DELETE RESTRICT` — SQLite **rechazaría** el
  `DELETE` mientras exista cualquier fila relacionada, que en la práctica es casi siempre. Otras
  **3 tablas** (`appointments`, `documents`, `reminders`) usan `ON DELETE SET NULL` — esas filas
  sobrevivirían pero quedarían huérfanas (sin paciente asociado), un comportamiento inconsistente
  si alguna vez se expusiera como feature real.

**Propuesta (no implementada)**: si en el futuro se decide ofrecer un borrado permanente real,
debería:
1. Ser una acción explícita, separada de "Archivar", con su propio flujo de confirmación (nunca
   un botón junto a "Archivar").
2. Decidir explícitamente qué pasa con cada una de las 12 tablas relacionadas — probablemente
   exigiendo que el paciente esté archivado y sin actividad reciente, y bloqueando el borrado
   (no simplemente fallando con un error SQL críptico) si tiene sesiones/pagos/procesos
   reales, con un mensaje claro de qué lo bloquea.
3. Evaluar retención legal/clínica (en Chile, la ficha clínica tiene plazos de conservación
   legalmente exigidos en algunos contextos) — **esto requiere criterio profesional/legal que no
   me corresponde decidir**, se señala aquí explícitamente para tu evaluación.
4. Si se implementa, considerar un export previo obligatorio (ver regla 6 de `CLAUDE.md`,
   Backup ≠ Sync ≠ Export).

No se implementó nada de esto — queda completamente en tus manos decidir si y cómo se construye.

### C.3 — UX de Google Calendar

**Estado actual auditado**: `SettingsScreen.tsx` pide pegar directamente el "Client ID" y "Client
Secret" de un cliente OAuth de Google Cloud Console, con instrucciones técnicas en la propia
pantalla ("Crea un cliente OAuth de tipo 'Aplicación de escritorio' en Google Cloud Console...").
Es funcionalmente correcto y ya sigue buenas prácticas (el campo Secret está enmascarado, nunca se
vuelve a mostrar tras guardarlo, hay indicadores de estado por paso) — pero exige que la
profesional entienda qué es un "cliente OAuth" y navegue la consola de Google por su cuenta, algo
razonable para un perfil técnico pero no para la mayoría de las usuarias objetivo de esta
aplicación.

**Propuestas posibles (ninguna implementada)**, sin tocar el manejo de OAuth/secretos en absoluto:

- **A.** Guía paso a paso con capturas/enlaces directos a la consola de Google dentro de la propia
  pantalla (wizard con más contexto visual, mismos campos).
- **B.** Video o documento externo enlazado, manteniendo el formulario tal cual.
- **C.** Explorar si existe una vía de "aplicación pública" de Cuaderno Clínico registrada una
  sola vez en Google Cloud Console por el propio proyecto (no por cada profesional) — esto
  **cambiaría el modelo de credenciales actual** (de "cada usuaria trae su propio Client ID/Secret"
  a "credenciales compartidas de la aplicación") y por lo tanto **requiere una decisión
  arquitectónica explícita tuya antes de evaluarse siquiera**, no es una mejora de UX simple.

Recomendación: A como mejora incremental de bajo riesgo; C solo si decides que vale la pena
replantear el modelo de credenciales — eso sí exigiría detenerme y explicar antes de tocar nada
(regla de la Fase 11 de este mismo documento).

### C.4 — "No responde" breve en Windows al iniciar/desbloquear

**Investigación por lectura de código únicamente** (sin mediciones reales en Windows disponibles
desde este entorno):

- El hook `.setup()` de Tauri (`src-tauri/src/lib.rs`) ejecuta, de forma síncrona y **antes de que
  la ventana empiece a procesar mensajes de Windows**: `run_startup_recovery` (comprobaciones/
  movimientos de directorio), `create_dir_all` del directorio del vault, y
  `sweep_stale_temp_files` (barrido de temporales). Todo esto es E/S de archivos local,
  normalmente rápida — pero en Windows, un antivirus con escaneo en tiempo real (Windows Defender
  u otro) inspeccionando cada archivo tocado en `%APPDATA%` puede alargar estas operaciones de
  forma perceptible. **Hipótesis, no confirmada**: si `.setup()` tarda más de unos segundos, el
  sistema operativo marca la ventana como "(No responde)" porque su bucle de mensajes todavía no
  arrancó — no porque algo esté realmente colgado.
- El desbloqueo (Argon2id) usa los parámetros recomendados por RFC 9106 (64 MiB de memoria, 3
  iteraciones, 4 hilos) — **deliberadamente costoso** (cientos de milisegundos en hardware de
  escritorio típico), documentado así desde el diseño original. Los comandos Tauri (incluido
  `unlock_vault`) se despachan en el runtime asíncrono de Tauri, no en el hilo de la UI — por
  diseño de Tauri esto no debería bloquear la ventana, pero no se verificó empíricamente en este
  entorno.

**Nunca se debilitó Argon2id/SQLCipher/cifrado alguno como respuesta a esto** — ninguna
optimización especulativa se aplicó.

**Lo que falta para confirmar la causa real**: medir en Windows real, con y sin antivirus activo,
en build de depuración y de release, cuánto tarda exactamente `.setup()` (se puede instrumentar
temporalmente con `log::info!` con timestamps, sin tocar ninguna lógica) versus cuánto tarda el
propio desbloqueo. Sin esa medición, cualquier cambio sería especulativo — por eso no se propuso
ni se aplicó ninguno.

## 10. Estado actualizado Windows / pre-RC

- **Validado en Windows real** (19/09/2026, por la usuaria): arranque, GUI, desbloqueo, bloqueo/
  desbloqueo, persistencia de contraseña/vault entre reinicios, navegación, backup vía GUI,
  restore vía GUI (extremo a extremo: backup→modificar→restore→verificar que la modificación
  desapareció), single-instance (segunda instancia enfoca la existente), datos preservados al
  reabrir. Build vendored de OpenSSL lento pero exitoso.
- **No validado en Windows en esta fase** (implementado y probado solo por tests de backend +
  lectura de código + una validación GUI parcial en Linux): los seis pasos del Plan de Seguridad
  rediseñado, la pantalla y el flujo completo de Biblioteca, el fix de scroll de Formulación en
  las resoluciones/escalados mencionados, las estadísticas por región/comuna con N=1, los chips de
  "Factores de riesgo", el guardado/aviso de cambios sin guardar en Objetivos.
- **Sigue sin ser una validación exhaustiva de todos los módulos** — es un smoke test general más
  las verificaciones puntuales de esta fase, nunca una certificación completa.
- **Recomendación explícita: NO declarar esta build lista para datos clínicos reales** hasta
  ejecutar el checklist de §11 en Windows real y decidir los cuatro puntos de §9.

## 11. Checklist de validación manual en Windows (paso a paso, para una persona no desarrolladora)

Usa siempre un vault **desechable** y datos **completamente ficticios** (nombres/RUT/diagnósticos
inventados). Nunca datos de una paciente real.

### Preparación
1. Ejecutar `npm run tauri dev` desde el "x64 Native Tools Command Prompt" (o abrir el `.exe`
   compilado si ya existe uno).
2. Anotar la hora exacta en que se hace doble clic/se ejecuta el comando, y la hora exacta en que
   la ventana aparece y responde a un clic — esto documenta el "No responde" de forma objetiva
   (§9.4).
3. Crear un vault nuevo con una contraseña de prueba (nunca la real) y guardar el código de
   recuperación en un lugar cualquiera (es desechable).

### 1. Plan de Seguridad — flujo completo de seis pasos
1. Crear un paciente ficticio.
2. Ir a la pestaña "Plan de seguridad" → "Crear plan de seguridad".
3. **Paso 1**: agregar 2-3 señales de alerta ficticias escribiendo y presionando Enter. Hacer clic
   sobre una para editarla en línea, confirmar con Enter. Eliminar una con la ×.
4. **Paso 2**: repetir con 2-3 estrategias individuales.
5. **Paso 3**: agregar 1-2 lugares de distracción (lista); agregar 1 "persona de distracción"
   (botón "Agregar" en la sección de personas) con nombre ficticio.
6. **Paso 4**: agregar 1 "persona a la que pedir ayuda" con nombre y teléfono ficticios.
7. **Paso 5**: agregar 1 profesional/institución con dirección y teléfono de servicio ficticios;
   marcar "Es un contacto de emergencia" en uno de ellos.
8. **Paso 6**: escribir texto en "Acciones para aumentar la seguridad del entorno".
9. Hacer clic en "Guardar como plan vigente". Verificar que aparece como "Plan vigente — versión
   1" con todo el contenido de los seis pasos visible.
10. Hacer clic en "Actualizar plan". Verificar que el nuevo borrador trae copiados todos los ítems
    y contactos del plan anterior (con nombres distintos internamente, pero mismo contenido
    visible). Editar algo, confirmar como vigente. Verificar en "Ver historial" que la versión
    anterior sigue completa y accesible.
11. Cerrar la app y volver a abrirla (o bloquear/desbloquear el vault). Verificar que todo el
    contenido del Plan de Seguridad persiste exactamente igual.

### 2. Biblioteca — flujo completo, incluida asociación cruzada
1. Ir a "Biblioteca" en la barra de navegación superior.
2. "Agregar recurso": título ficticio, tipo "Protocolo", sin archivo (probar que se guarda solo
   con URL). Agregar un segundo recurso CON un archivo real (un PDF o imagen de prueba cualquiera,
   contenido ficticio).
3. Abrir el recurso con archivo (si es imagen, debe previsualizarse en la propia app; si es PDF,
   debe abrirse con el lector externo). Exportar copia a una carpeta cualquiera y confirmar que el
   archivo exportado se abre correctamente fuera de la app.
4. En el recurso, hacer clic en "Pacientes" → asociarlo a 2 pacientes ficticios distintos (crear un
   segundo paciente ficticio si hace falta). Verificar que ambos aparecen en la lista.
5. Ir a la ficha de uno de esos pacientes → pestaña "Biblioteca" → verificar que el recurso
   aparece ahí también. Desasociarlo desde la ficha del paciente. Volver a "Biblioteca" global →
   "Pacientes" del recurso → verificar que ya no aparece ese paciente.
6. Desde la ficha de un paciente → "Biblioteca" → "Asociar un recurso existente" → buscar el otro
   recurso por título → asociarlo. Verificar que aparece en ambos lados.
7. Intentar archivar un recurso que sigue asociado a un paciente → debe aparecer un aviso
   mencionando cuántos pacientes lo tienen asociado, con opción de confirmar igual. Confirmar.
   Verificar en "Archivados" que aparece ahí, y que la asociación con el paciente **sigue
   existiendo** (revisar "Pacientes" del recurso archivado). Restaurarlo.
8. **Backup/Restore con Biblioteca**: crear un backup desde Ajustes. Archivar/editar el recurso de
   Biblioteca (cambiarle el título). Restaurar el backup. Verificar que el recurso vuelve a su
   estado original (título anterior, sin archivar) y que sigue teniendo su archivo adjunto
   abrible y sus asociaciones con pacientes intactas.

### 3. Formulación — scroll
1. Crear una Formulación nueva con contenido largo en varias secciones (para que el modal supere
   la altura de la ventana).
2. Probar a **1366×768** y con el **escalado de Windows en 100% y en 125%** (Configuración →
   Sistema → Pantalla → Escala): el modal completo debe ser alcanzable con scroll, el botón
   "Guardar" siempre visible o alcanzable, sin contenido cortado permanentemente fuera de la
   ventana. Verificar que Tab navega correctamente entre campos sin saltos raros.

### 4. Estadísticas
1. Con menos de 3 pacientes en alguna región/comuna, ir a "Estadísticas".
2. Verificar que **cada** región y comuna aparece por su nombre real con su conteo y porcentaje —
   nunca agrupada bajo "Otras", ni siquiera con 1 solo paciente.
3. Hacer clic en una región del gráfico de donas → verificar que la lista de comunas se filtra a
   esa región. Usar "Ver todas las comunas" para volver.
4. Verificar el texto de "Sin registrar" para pacientes sin región/comuna informada (crear uno sin
   completar esos campos si hace falta).

### 5. Factores de riesgo (sin JSON)
1. Ir a Antecedentes clínicos de un paciente ficticio → "Factores de riesgo".
2. Verificar que la interfaz muestra/pide chips de texto (escribir + Enter), nunca JSON crudo
   entre corchetes y comillas.
3. Guardar, recargar la página (o navegar a otra pestaña y volver), verificar que los chips
   persisten correctamente.

### 6. Objetivos — guardado y cambios sin guardar
1. Abrir un objetivo terapéutico existente, editar un campo (p. ej. la descripción).
2. Verificar que aparece una barra fija abajo indicando cambios sin guardar, con un botón
   "Guardar cambios" siempre visible sin necesidad de hacer scroll.
3. Sin guardar, hacer clic en "Volver a la ficha del paciente" → debe aparecer una confirmación
   antes de salir. Cancelar, guardar de verdad, y ahora sí volver — sin ninguna advertencia
   (porque ya no hay cambios pendientes).

### 7. Single-instance
1. Con la app abierta, ejecutar el `.exe` (o `npm run tauri dev`) una segunda vez.
2. Verificar que no se abre una segunda ventana — la ventana existente pasa al frente.

### 8. "No responde" — observación en debug y en release
1. Repetir el paso de "Preparación" (arranque cronometrado) tanto con `npm run tauri dev` (debug)
   como con un `.exe` compilado en modo release, si hay uno disponible.
2. Anotar si aparece "(No responde)" en la barra de título de Windows, durante cuántos segundos
   aproximadamente, y si ocurre también al desbloquear el vault (no solo al arrancar).
3. Repetir con el antivirus de Windows temporalmente desactivado (si tu política de TI lo permite)
   para aislar si el antivirus es un factor — **reactivarlo después**, esto es solo para
   diagnóstico puntual.

## 12. `git status` al cierre

```
$ git status --short
(sin salida — árbol de trabajo limpio)
$ git branch --show-current
claude/cuaderno-clinico-desktop-udijjq
```

## 13. Commits de esta fase

| Commit | Descripción |
| --- | --- |
| `33891d3` | fix(formulacion): corregir scroll de los modales de crear/actualizar |
| `91de6b9` | feat(estadisticas): mostrar todas las regiones/comunas en la pantalla privada |
| `ee6386d` | fix(antecedentes): reemplazar el JSON crudo de "Factores de riesgo" por tags |
| `b0eae43` | fix(objetivos): separar navegación de guardado y avisar cambios sin guardar |
| `ff7538c` | feat(plan-seguridad): backend del rediseño en 6 pasos (Stanley & Brown) |
| `c5cad4f` | feat(plan-seguridad): rediseñar la pestaña en los 6 pasos de Stanley & Brown |
| `dc073fc` | docs(plan-seguridad): documentar el rediseño en 6 pasos (SCHEMA_V11) |
| `5788216` | refactor(documentos): permitir patient_id nulo en NewDocumentRow |
| `d80a6e9` | feat(biblioteca): backend de la Biblioteca global de recursos (SCHEMA_V12) |
| `af55c6f` | feat(biblioteca): pantalla global y sección en la ficha del paciente |
| `202e01c` | docs(biblioteca): documentar el diseño (docs/library.md + ARCHITECTURE.md) |

Todos en la rama `claude/cuaderno-clinico-desktop-udijjq`, ninguno pusheado a un remoto desde
este entorno (el mecanismo de sincronización de esta sesión se encarga de eso).

## 14. Próximo prompt recomendado

```
Continuemos desde el informe de cierre de la fase de continuación post-Fase 19. Antes de avanzar,
necesito que ejecutes el checklist de validación manual de Windows (sección 11 del informe) con
datos completamente ficticios en un vault desechable, y que me reportes honestamente qué pasó en
cada punto — incluida la observación cronometrada del "No responde" en debug y en release, con y
sin antivirus activo.

Mientras tanto, decide (no hace falta que me des la respuesta ahora, solo cuando quieras seguir):
1. Línea temporal: ¿ocultar la pestaña (mi recomendación) o dejarla visible como "Próximamente"?
2. Borrado permanente de pacientes: ¿lo necesitas como feature real, o el archivado actual es
   suficiente? Si lo necesitas, ¿hay algún requisito legal/clínico de retención de fichas en tu
   jurisdicción que deba respetar el diseño?
3. UX de Google Calendar: ¿mejoro la guía dentro de la misma pantalla (opción A), o prefieres que
   evalúe primero si tiene sentido un modelo de credenciales compartidas de la aplicación
   (opción C, que sí requiere que me detenga a explicar antes de tocar nada)?

Cuando tengas los resultados del checklist y tus decisiones sobre esos tres puntos, seguimos con:
- Si el checklist encuentra defectos reales, los corrijo antes de cualquier otra cosa.
- Si el "No responde" se confirma real y medible, investigo la causa exacta con instrumentación
  temporal (nunca debilitando Argon2id/SQLCipher) y te propongo una corrección concreta antes de
  aplicar nada.
- Implemento tu decisión sobre Línea temporal.
- Si decides que necesitas borrado permanente de pacientes, te presento un diseño concreto
  (qué tablas, qué bloqueos, qué confirmaciones) para tu aprobación antes de escribir código —
  nunca lo implemento directamente por ser un cambio de modelo de datos con riesgo de pérdida de
  información.
- Seguimos con lo que quede pendiente del roadmap (Fase 18: artefacto de release, seguridad/
  privacidad pre-RC, informe final con gate RC1 — ver tareas #154-156 ya trackeadas).

Recuerda: nunca inventes datos clínicos reales, nunca toques SQLCipher/Argon2id/AES-GCM/backup-
restore/single-instance sin detenerte a explicar primero, y si algo te obliga a apartarte de estas
instrucciones, detente y pregunta.
```
