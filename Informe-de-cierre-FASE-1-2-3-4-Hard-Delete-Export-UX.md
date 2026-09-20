# Informe de cierre — FASE 1 (validación Windows) + FASE 2 + FASE 3 + FASE 4

## 1. Resumen ejecutivo

Continuación autorizada explícitamente desde el estado de FASE 1 ya validada en Windows real
(19/09/2026, mismo vault preexistente que originó los reportes WIN-DB-01/WIN-DB-02). Esta entrega
cubre, en orden, tres fases completas y consecutivas:

- **FASE 2** — borrado permanente (hard delete) protegido, separado de "Archivar", para recursos de
  Biblioteca (2A) y para pacientes (2B).
- **FASE 3** — exportar/imprimir el Plan de Seguridad vigente, sin ninguna dependencia nueva de
  generación de PDF.
- **FASE 4** — cuatro mejoras de UX: paleta categórica en Estadísticas, ocultar "Línea temporal",
  simplificar la primera vista de Google Calendar, y auditoría de loading/error en las superficies
  nuevas.

Ninguna regla de parada de `CLAUDE.md` se activó: sin tocar SQLCipher/Argon2id/AES-GCM/HKDF/
key-wrapping/recovery/backup-restore/single-instance/UUID/arquitectura repository→service→
command→IPC→React, sin migración destructiva, sin dependencia nueva, sin envío de información
fuera del equipo, sin cambio al modelo OAuth de Google Calendar.

Suite completa: **852/852 tests Rust en verde** (843 al empezar esta sesión + 9 nuevos de hard
delete), `cargo build --release`/`cargo test --release`/`cargo clippy --release --all-targets`
limpios, `npm run build`/`npm run lint` limpios sin ninguna categoría nueva de warning, cero
dependencias nuevas en `Cargo.toml`/`Cargo.lock`/`package.json`/`package-lock.json`.

## 2. HEAD inicial

```
db19994570cdb9c5e2e78a4f3d264bb644dcc23  docs: registrar causa raíz de WIN-DB-01/WIN-DB-02, corrección y test de regresión
```

Branch: `claude/cuaderno-clinico-desktop-udijjq`. Working tree limpio, local/remoto sincronizados
al empezar.

## 3. FASE 1 — confirmación de que ya estaba validada en Windows

Antes de empezar esta sesión, FASE 1 (causa raíz de WIN-DB-01/WIN-DB-02: `unlock_vault`/
`recover_access` nunca ejecutaban `db::run_migrations` sobre un vault existente) ya estaba
implementada, testeada (843/843) y **validada por la usuaria en Windows real sobre el mismo vault
preexistente que originó ambos reportes**: Plan de Seguridad y Biblioteca↔paciente pasaron de
fallar a funcionar correctamente. No se volvió a tocar esa corrección en esta sesión — se mantienen
intactos sus dos tests de regresión (`unlock_vault_upgrades_an_existing_v10_vault_and_makes_its_data_reachable`,
`recover_access_also_upgrades_an_existing_v10_vault`). Ver `docs/safety-plan.md` §21 y
`docs/library.md` §12 para el detalle completo ya documentado entonces.

## 4. Hard-delete Biblioteca (FASE 2A)

**Diseño**: `services::library::hard_delete_resource(session, files_root, id)` — borrado físico e
irreversible, distinto de `archive_resource` (reversible).

**Restricciones, ambas verificadas en el servidor, nunca solo en la UI**:
1. El recurso debe estar ya archivado (`LibraryError::MustBeArchivedFirst` si no) — reforzado
   también a nivel de SQL (`WHERE deleted_at IS NOT NULL` en el propio `DELETE`).
2. Cero asociaciones con pacientes (`LibraryError::LinkedToPatientsBlocksHardDelete(n)` si tiene
   alguna) — a diferencia de `archive_resource`, aquí **no existe** un `force`: un borrado físico
   nunca puede dejar una fila de `library_resource_patients` apuntando a un recurso inexistente.

**Archivos**: si el recurso tiene un `file_document_id`, su fila de `documents` se borra dentro de
la misma transacción SQL manual (`BEGIN IMMEDIATE`/`COMMIT`, `ROLLBACK` ante error) que la fila de
`library_resources`. El ciphertext en disco se borra **después** de que la transacción confirma,
en modo best-effort (`let _ = std::fs::remove_file(...)`) — si ese borrado falla, la base ya quedó
100% consistente; el peor caso es un ciphertext huérfano detectable por el diagnóstico ya existente
(`services::documents::find_orphan_storage_paths`), nunca una fila huérfana ni un estado a medias.

**Asociaciones**: nunca se tocan silenciosamente. La única forma de eliminar un recurso asociado es
desasociarlo primero de cada paciente (botón "Pacientes" → "Desasociar"), y solo entonces el
borrado permanente se habilita.

**Errores**: mensajes de dominio específicos (`MustBeArchivedFirst`, `LinkedToPatientsBlocksHardDelete(n)`)
que el frontend traduce en el modal — nunca un error SQL crudo.

**UX**: "Eliminar permanentemente" solo aparece en la vista "Archivados" — nunca junto a "Abrir" en
un recurso activo. Modal con advertencia ("no se puede deshacer"), y confirmación exigiendo escribir
"ELIMINAR" antes de habilitar el botón final. Si el backend rechaza por asociaciones, el modal
ofrece "Ver pacientes asociados" para desvincular sin salir del flujo.

## 5. Hard-delete paciente (FASE 2B)

**Inventario real de tablas** (auditado contra el esquema real en `db::migrations`, no contra la
lista aproximada del informe anterior):

| Tabla | Tipo de FK | Hijos transitivos |
|---|---|---|
| `patient_clinical_profile` | `RESTRICT` (1:1) | — |
| `sessions` | `RESTRICT` | `session_notes` (`RESTRICT`), `session_goals` (`CASCADE`) |
| `case_formulations` | `RESTRICT` | `formulation_versions` (`CASCADE`) → `formulation_nodes`/`formulation_edges` (`CASCADE`) |
| `therapeutic_goals` | `RESTRICT` | `goal_indicators`/`goal_interventions` (`CASCADE`), `session_goals` (`CASCADE`) |
| `assessment_administrations` | `RESTRICT` | — |
| `payments` | `RESTRICT` | — |
| `patient_prep_notes` | `RESTRICT` | — |
| `therapy_tasks` | `RESTRICT` | — |
| `treatment_episodes` | `RESTRICT` | `episode_clinical_profile` (`RESTRICT`, 1:1), `episode_closures` (`RESTRICT`) |
| `safety_plans` | `RESTRICT` | `safety_plan_contacts`/`safety_plan_list_items` (`CASCADE`) |
| `documents` (propios) | `SET NULL` | archivo cifrado en disco, fuera de SQL |
| `library_resource_patients` | `CASCADE` | — (nunca `library_resources` en sí) |
| `appointments` | `SET NULL` | — |
| `reminders` | `SET NULL` | — |

**Orden/estrategia**: `repositories::patients::hard_delete` borra cada tabla con su propio
`DELETE FROM` explícito, en orden hojas→raíz — **nunca depende de que SQLite complete el trabajo
por sí solo** vía los `ON DELETE CASCADE` ya declarados, ni siquiera en las tablas que ya lo tienen
(instrucción explícita). Orden real: `formulation_edges`→`formulation_nodes`→`formulation_versions`
→ `goal_indicators`/`goal_interventions`→`session_goals`→`session_notes`→`episode_clinical_profile`/
`episode_closures`→`safety_plan_contacts`/`safety_plan_list_items`→`therapy_tasks`→
`patient_prep_notes`→`assessment_administrations`→`payments`→`therapeutic_goals`→
`case_formulations`→`safety_plans`→`sessions`→`treatment_episodes`→`patient_clinical_profile`→
`documents`→`library_resource_patients`→`appointments`/`reminders`→`patients`.

**Transacción**: toda la secuencia corre dentro de una única transacción SQL manual (`BEGIN
IMMEDIATE`/`COMMIT`, `ROLLBACK` explícito ante cualquier error) — nunca queda un estado a medias.

**Documentos**: los `storage_path` de los documentos propios del paciente se leen **antes** de la
transacción; sus archivos cifrados se borran del disco **después** de que la transacción confirma,
mismo modelo best-effort que Biblioteca.

**Biblioteca**: solo se borra `library_resource_patients` (la relación con ese paciente en
particular) — `library_resources` (el recurso global) **nunca** se toca, ni aunque sea el último
paciente asociado.

**Limpieza filesystem**: best-effort tras el commit; peor caso posible, un ciphertext huérfano
detectable, nunca una fila de base apuntando a un archivo inexistente.

**Requisito**: el paciente debe estar ya archivado (`PatientError::MustBeArchivedFirst` si no).
Resumen de alcance de solo lectura (`hard_delete_scope`) antes de confirmar, con advertencia neutral
de conservación de registros clínicos, y confirmación exigiendo escribir "ELIMINAR".

## 6. Cambios de schema/migraciones

**Ninguno.** Todas las tablas tocadas por hard-delete ya existían desde `SCHEMA_V1`–`V12`. Sin
`ALTER TABLE`, sin nueva migración, `schema_version` sigue en `12`.

## 7. Safety Plan export (FASE 3)

**Implementación**: `src/features/safety-plan/SafetyPlanExport.tsx` — modal "Exportar plan de
seguridad" + documento de impresión, ambos componentes React puros (sin lógica de backend nueva:
reutiliza exactamente los `plan`/`contacts`/`items` ya cargados para la vista en pantalla del plan
vigente).

**Formato**: `window.print()`, el diálogo de impresión nativo del WebView — "Guardar como PDF" es
un destino estándar de ese diálogo tanto en Windows (Microsoft Print to PDF) como en macOS. El
contenido se renderiza en un React Portal dentro de `#print-root`, creado como **hermano** de
`#root` directamente en `<body>` (nunca dentro de `#root`); dos reglas nuevas en `index.css`
ocultan el resto de la aplicación y muestran únicamente el documento durante la impresión.

**Include/exclude nombre**: casilla "Incluir nombre del paciente", marcada por defecto — permite
generar tanto una versión identificada como una completamente anónima del mismo documento, sin
volver a armar el contenido. El nombre se obtiene con `patientsApi.get` (`preferredName ||
fullName`), fallo silencioso (sin bloquear el resto del modal) si esa consulta falla.

**Imprimir**: mismo botón, mismo documento — no hay una segunda implementación ni una ruta de
código separada para "imprimir" contra "exportar a PDF": ambas son la misma llamada a
`window.print()`, la diferencia la decide la usuaria en el propio diálogo del sistema operativo
(elegir una impresora física o "Guardar como PDF").

**Manejo de privacidad**: advertencia explícita antes de generar nada ("la copia exportada ya no
está protegida por el cifrado de Cuaderno Clínico…"). Contenido excluido por construcción (nunca se
le pasa al componente de impresión): diagnóstico, motivo de consulta, notas clínicas, antecedentes,
sesiones, formulación, objetivos, evaluaciones, UUIDs, IDs, metadata de base de datos, historial de
versiones, borradores, logs, claves. Por defecto solo el plan **vigente** — nunca una versión
histórica desde este modal.

## 8. Dependencias nuevas

**Ninguna.** Confirmado por diff de `Cargo.toml`/`Cargo.lock`/`package.json`/`package-lock.json`:
sin cambios. La decisión explícita de usar `window.print()` en vez de una librería Rust de
generación de PDF (`printpdf`/`genpdf`/etc.) es precisamente lo que evitó tener que detenerme a
pedir aprobación de dependencia bajo la regla de parada correspondiente — ver §7 y
`docs/safety-plan.md` §22.

## 9. Estadísticas (FASE 4A)

**Paleta**: `CHART_PALETTE` en `src/features/statistics/StatisticsScreen.tsx` pasa de seis tonos
derivados del único acento verde (vía `color-mix`) a ocho tonos categóricos realmente distintos:
azul `#2a78d6`, naranja `#eb6834`, turquesa `#1baf7a`, dorado `#eda100`, fucsia `#e87ba4`, verde
`#008300`, violeta `#4a3aa7`, rojo `#e34948`.

**Accesibilidad**: validada con la herramienta de accesibilidad de paletas categóricas del
proyecto (`validate_palette.js`) — separación perceptual (`ΔE` en OKLab) entre colores adyacentes
por encima del umbral objetivo tanto bajo las formas más comunes de daltonismo (protan/deutan/
tritan) como en visión normal; el único WARN es de contraste de tres tonos contra el fondo claro,
resuelto por la "regla de alivio" ya cumplida estructuralmente (cada segmento/barra siempre lleva
su etiqueta de texto al lado — nunca depende solo del color).

**Región**: cada región visible tiene un color categórico distinto; el donut y el punto de la
leyenda usan **exactamente** la misma llamada a `colorFor(item, index)` — la correspondencia es
exacta por construcción, no por coincidencia visual.

**Comuna**: mismas ocho tonalidades para las barras horizontales; con más de ocho comunas visibles
el color se cicla (instrucción explícita de la usuaria) en vez de agregar tonos que ya no se
distinguirían entre sí. "Otras" (categoría agrupada por privacidad, `N<3`) sigue con un gris neutro
completamente fuera de esta paleta.

**Sin cambios de lógica**: N=1 sin agrupar, separación región/comuna, cantidad+porcentaje, filtro al
hacer clic en una región — todo ya validado en Windows real en la fase anterior, intacto.

## 10. Timeline (FASE 4B)

`'linea_temporal'` se removió del array `SECTIONS` (el que efectivamente genera la navegación) en
`src/features/patients/PatientDetailScreen.tsx` — la pestaña ya no aparece en la ficha del
paciente. Se **conservó** en el tipo `SectionId` y en el resto de la estructura de la pantalla
(nunca se borró código/estructuras reutilizables, solo se dejó de mostrar la pestaña), tal como
pide la instrucción explícita. Nada de "Línea temporal" se implementó — sigue sin funcionalidad
real, ahora simplemente invisible en vez de mostrar "Próximamente".

## 11. Google Calendar (FASE 4C)

**Nueva UX**: la vista simple (siempre visible primero, en `src/features/settings/SettingsScreen.tsx`)
muestra el estado en una frase ("No conectado"/"Conectado"), una descripción no técnica de qué se
sincroniza, y un botón "Conectar Google Calendar". Si las credenciales todavía no están
configuradas, ese mismo botón lleva a "Configuración avanzada" (nunca un botón mágico que finja
conectar sin credenciales) junto a una explicación de los tres pasos reales. Conectado, el
selector de calendario se muestra directamente en la vista simple (contenido legítimamente simple).
"Configuración avanzada" es una sección colapsable, cerrada por defecto, con exactamente el mismo
formulario de Client ID/Client Secret que ya existía.

**Qué NO cambió del OAuth**: cero cambios de código en `src-tauri/src/calendar/*`, en el manejo de
tokens/secrets, en el almacenamiento cifrado de credenciales (`keyring`), ni en los scopes
solicitados. Cada profesional sigue trayendo su propio cliente OAuth de Google Cloud Console — no
se evaluó ni se implementó un modelo de credenciales compartidas (opción C de la auditoría
anterior, que requeriría detenerse a explicar antes de tocar nada).

**Datos enviados**: sin cambios — el evento espejo en Google Calendar sigue llevando únicamente un
resumen genérico ("Sesión clínica"/"Bloqueo personal") y las dos marcas de tiempo. Nunca nombre,
RUT, diagnóstico, modalidad ni notas — código no tocado en esta fase.

## 12. Loading/error states (FASE 4D)

Auditadas las superficies nuevas de estas fases:
- **Hard-delete de paciente**: el resumen de alcance (`getHardDeleteScope`) ahora tiene una función
  `loadScope` reutilizable con botón "Reintentar" si falla, y el botón de confirmación final queda
  deshabilitado mientras el resumen no haya cargado (`scope === null`) — nunca se puede confirmar
  un borrado sin haber visto qué se va a perder.
- **Hard-delete de Biblioteca**: sin estado de carga inicial (formulario de confirmación estático);
  errores de la acción de borrado se muestran con mensaje claro y, si es por asociaciones, con un
  enlace directo a "Ver pacientes asociados".
- **Exportar Plan de Seguridad**: el fetch del nombre del paciente falla en silencio (no bloquea el
  resto del modal, dato no crítico); el propio `window.print()` es una operación síncrona del
  sistema operativo, sin estado de carga propio que pueda quedar colgado.
- **Google Calendar**: sin cambios de lógica de carga — la reorganización fue solo de layout.

Ningún nuevo spinner infinito introducido; mensajes genéricos y accionables donde corresponde; sin
exposición de SQL/stack trace en ninguna superficie nueva.

## 13. Tests nuevos

9 tests nuevos en backend (todos en `services::library::tests`/`services::patients::tests`):

- `services::library::tests`: `hard_delete_rejects_a_resource_that_is_not_archived`,
  `hard_delete_blocks_when_still_linked_to_a_patient_with_no_force_option`,
  `hard_delete_succeeds_after_unlinking_from_every_patient`,
  `hard_delete_removes_a_resource_without_a_file`,
  `hard_delete_removes_a_resource_with_a_file_and_its_ciphertext_from_disk`,
  `hard_delete_never_touches_other_resources`.
- `services::patients::tests`: `hard_delete_rejects_an_active_patient`,
  `hard_delete_removes_a_patient_with_no_related_data_at_all`,
  `hard_delete_removes_every_related_row_across_every_audited_table_and_nothing_else` (siembra una
  fila en cada una de las 14 tablas auditadas, incluido un documento con archivo real en disco y
  una asociación de Biblioteca, ejecuta el borrado real, y verifica: cero filas restantes en cada
  tabla, el ciphertext borrado del disco, el recurso global de Biblioteca intacto, y un segundo
  paciente de control con su propia sesión sin ningún efecto colateral).

Sin framework de tests de frontend instalado (constraint preexistente, no introducida en esta
fase) — Exportar Plan de Seguridad y las tres mejoras de UX de FASE 4 se validaron por `tsc -b`
estricto, lectura de código, y build/lint limpios.

## 14. Resultado completo

```
cargo build --release                      → limpio, terminó en 5m51s
cargo test --release                       → 852 passed; 0 failed; 0 ignored
cargo clippy --release --all-targets       → limpio, sin advertencias
npm run build                              → limpio (tsc -b + vite build)
npm run lint (oxlint)                      → 33 warnings, 0 errores, exit 0
```

## 15. Número total final de tests

**852 tests de backend** (843 al empezar esta sesión + 9 nuevos de hard-delete). Sin tests de
frontend automatizados (constraint preexistente del proyecto).

## 16. Warnings

**Baseline** (antes de esta sesión): 32 warnings de `npm run lint`, dos categorías ya toleradas en
15+ archivos (`react/set-state-in-effect`, `react/incompatible-library`).

**Nuevos**: 1 warning adicional (33 total), de la misma categoría `set-state-in-effect` ya
tolerada — en el nuevo `useEffect(loadScope, [patientId])` de `HardDeletePatientModal`
(`PatientDetailScreen.tsx:141`), idéntico patrón al usado docenas de veces en el resto de la base
(p. ej. `SafetyPlanTab.tsx`, `LibraryScreen.tsx`). Ninguna categoría nueva. `cargo clippy` (debug y
release, todos los targets): cero warnings, en ambos casos.

## 17. Riesgos residuales

1. **Validación GUI real pendiente** — igual que en fases anteriores, este entorno de desarrollo no
   tiene un compositor real para ejercitar interactivamente los modales de confirmación, el flujo
   completo de exportar/imprimir, ni el diálogo nativo de impresión de Windows/macOS. Todo lo
   descrito en este informe está verificado por tests automatizados + lectura de código + build/lint
   estrictos — la validación humana real queda en el checklist de §20.
2. **`window.print()` nunca se ejercitó contra un WebView2 real de Windows en este entorno** —
   funciona por especificación estándar de ambos motores (WebView2/WKWebView soportan impresión
   nativa desde hace años), pero no hay confirmación empírica en este entorno headless.
3. **El ciclo de colores de Estadísticas más allá de ocho categorías** es una limitación conocida y
   aceptada explícitamente (instrucción de la usuaria) — dos comunas muy separadas en la lista
   podrían compartir color si hay más de ocho comunas visibles simultáneamente.
4. **Ningún vault con datos clínicos reales pasó por un hard-delete todavía** — solo vaults de
   prueba con datos ficticios en los tests automatizados.

## 18. Qué NO se cambió deliberadamente

- **Ninguna de las áreas explícitamente protegidas** (SQLCipher, Argon2id, AES-GCM, HKDF/key-
  wrapping, master password, recovery code, startup recovery, atomic restore, backup format,
  single-instance, vault lock, modelo de IDs UUID, arquitectura repository→service→command→
  IPC→React) se tocó en absoluto.
- **Backup/Restore**: sin ningún cambio de código en esta sesión (ver §7 de las restricciones del
  pedido) — las tablas nuevas de hard-delete no requieren cambios ahí porque `create_backup` ya
  copia el estado completo de la base independientemente de qué tablas existan.
- **Modelo OAuth de Google Calendar**: sin cambios (ver §11).
- **Formulación/Objetivos/Factores de riesgo**: sin tocar, ya cerrados en la fase anterior.
- **`library_tags`/`library_resource_tags`**: siguen sin usarse.
- **"No responde" de Windows**: no investigado en esta sesión (fuera del alcance del pedido).

## 19. Validaciones todavía pendientes

- Checklist manual completo de §20, en Windows real, sobre datos completamente ficticios.
- Confirmación empírica de que `window.print()`/"Guardar como PDF" produce un PDF legible y
  correctamente formado en Windows real.
- Confirmación de que el hard-delete de un paciente/recurso con volumen real de datos (no solo el
  test sintético) se ejecuta en un tiempo razonable y sin bloquear la UI perceptiblemente.

## 20. Checklist de validación manual en Windows (paso a paso)

Usa siempre un vault **desechable** y datos **completamente ficticios**.

### Preparación
1. `cd` al repositorio.
2. `git fetch`
3. `git pull --ff-only`
4. `git log -1 --oneline` — debe mostrar `dd72ebb docs: registrar FASE 1...` como el commit más
   reciente.
5. `npm run tauri dev`

### A. Plan de Seguridad
- Abrir el mismo vault ficticio ya usado para validar FASE 1.
- Abrir el plan vigente, verificar pasos 1–6 visibles con su contenido.
- Agregar/editar/eliminar un ítem de lista.
- Agregar un contacto (Paso 4/5) con teléfono y dirección.
- Guardar como borrador, luego confirmar como vigente.
- "Actualizar plan" y verificar que copia el contenido anterior.
- "Ver historial" y confirmar que la versión anterior sigue completa.
- Cerrar y reabrir la app — verificar persistencia.

### B. Exportar plan
- Con un plan vigente abierto, clic en "Exportar plan".
- Dejar "Incluir nombre del paciente" marcado, clic en "Exportar / Imprimir".
- En el diálogo de impresión de Windows, elegir "Microsoft Print to PDF", guardar, abrir el PDF
  resultante.
- Verificar: título "PLAN DE SEGURIDAD", nombre del paciente visible, los 6 pasos con su contenido,
  teléfonos/direcciones legibles.
- Verificar que **no** aparece: diagnóstico, notas clínicas, UUIDs, historial de versiones.
- Repetir desmarcando "Incluir nombre del paciente" — confirmar que el nombre no aparece en el PDF.
- Repetir y cancelar el diálogo de impresión — confirmar que no queda ningún archivo parcial.
- Si el sistema tiene una impresora física configurada, probar "Imprimir" con una hoja real
  (opcional).

### C. Biblioteca
- Ir a "Biblioteca". Confirmar que el recurso global de la validación de FASE 1 sigue ahí.
- Asociar ese recurso a un segundo paciente ficticio.
- Ir a "Archivados", archivar el recurso (con confirmación si está asociado).
- En el recurso archivado, clic en "Eliminar permanentemente" — debe rechazar mientras siga
  asociado a algún paciente, mostrando cuántos.
- Clic en "Ver pacientes asociados" desde ese mismo error, desasociar de ambos pacientes.
- Reintentar "Eliminar permanentemente", escribir "ELIMINAR", confirmar.
- Verificar que el recurso desaparece de "Archivados" y de la lista activa.

### D. Paciente
- Crear un paciente ficticio nuevo, agregar antecedentes, una sesión, un documento ficticio, y
  asociar un recurso de Biblioteca.
- Archivar el paciente. Verificar "Restaurar" y "Eliminar permanentemente" ambos visibles en la
  ficha archivada.
- Clic en "Eliminar permanentemente" — verificar que el resumen muestra correctamente sesiones=1,
  documentos=1, asociaciones de Biblioteca=1, antecedentes=Sí.
- Escribir "ELIMINAR", confirmar.
- Verificar que el paciente desaparece de "Archivados".
- Volver a "Biblioteca" → confirmar que el recurso global asociado sigue existiendo intacto.

### E. Estadísticas
- Con varias regiones/comunas de pacientes ficticios (algunas con N=1), ir a "Estadísticas".
- Verificar que cada segmento del donut y su punto de leyenda tienen colores **visiblemente
  distintos** entre sí (no todos verdes) y coinciden exactamente entre sí.
- Verificar lo mismo en las barras de comuna.
- Confirmar que N=1 sigue apareciendo sin agrupar en "Otras", y que el filtro al hacer clic en una
  región sigue funcionando.

### F. Timeline
- En la ficha de cualquier paciente, confirmar que "Línea temporal" **ya no aparece** en la barra
  de navegación de pestañas.

### G. Google Calendar
- Ir a Ajustes. Confirmar que la vista simple muestra "No conectado"/"Conectado" y una descripción
  no técnica, sin campos de Client ID/Secret visibles de entrada.
- Clic en "Configuración avanzada", confirmar que el formulario de Client ID/Client Secret sigue
  accesible ahí, con las mismas instrucciones de siempre.
- **No completar credenciales reales durante esta validación** (usa el vault ficticio, sin
  necesidad de conectar Google de verdad).

### H. Backup/Restore
- Crear un backup desde Ajustes.
- Cambiar un dato del paciente, cambiar una asociación de Biblioteca.
- Restaurar el backup.
- Confirmar que el estado vuelve exactamente al del backup, incluidas las asociaciones de
  Biblioteca.

### I. Seguridad básica
- Bloquear/desbloquear el vault.
- Cerrar y reabrir la aplicación completa.
- Ejecutar una segunda instancia — confirmar que no abre una segunda ventana (single-instance).
- Confirmar persistencia de todo lo anterior tras el reinicio completo.

Todo con **datos ficticios**.

## 21. `git status` final

```
$ git status --short
(sin salida — árbol de trabajo limpio)
```

## 22. Commits creados

| Commit | Descripción |
| --- | --- |
| `7996704` | feat(hard-delete): borrado permanente protegido de recursos de Biblioteca y de pacientes (incluye 4B, ocultar Línea temporal, por compartir archivo) |
| `4518ce0` | feat(safety-plan): exportar/imprimir el plan vigente sin dependencia nueva |
| `8535f42` | feat(estadisticas): paleta categórica de ocho tonos realmente distintos |
| `ae594c2` | ux(calendar): vista simple de Google Calendar, credenciales detrás de "Configuración avanzada" |
| `dd72ebb` | docs: registrar FASE 1 (causa raíz WIN-DB-01/02) y FASE 2/3/4 en la tabla de fases |

**Nota de transparencia**: "ocultar Línea temporal" (FASE 4B) quedó incluido dentro del commit de
hard-delete (`7996704`) en vez de en un commit separado — ambos cambios tocan el mismo archivo
(`PatientDetailScreen.tsx`) y se hicieron antes del primer commit de esta sesión, así que no podían
separarse limpiamente sin staging por hunks. Sustantivamente equivalente a la división sugerida.

## 23. HEAD final

```
dd72ebb  docs: registrar FASE 1 (causa raíz WIN-DB-01/02) y FASE 2/3/4 en la tabla de fases
```

## 24. Confirmación de push remoto

```
$ git push -u origin claude/cuaderno-clinico-desktop-udijjq
To https://github.com/jpcaamanoa/Vale-pega
   db19994..dd72ebb  claude/cuaderno-clinico-desktop-udijjq -> claude/cuaderno-clinico-desktop-udijjq
```

Confirmado: local y remoto sincronizados en `dd72ebb`.

## 25. Instrucciones exactas para actualizar tu PC Windows

1. Abre el "x64 Native Tools Command Prompt" (o tu terminal habitual) y ve a la carpeta del
   repositorio.
2. Ejecuta:
   ```
   git fetch origin
   git pull --ff-only
   ```
3. Ejecuta `git log -1 --oneline` — debería mostrar:
   ```
   dd72ebb docs: registrar FASE 1 (causa raíz WIN-DB-01/02) y FASE 2/3/4 en la tabla de fases
   ```
4. Para abrir la app en modo desarrollo: `npm run tauri dev` (primera vez puede tardar varios
   minutos compilando SQLCipher/OpenSSL vendorizados).
5. Si prefieres un `.exe` compilado: `npm run tauri build`, el instalador queda en
   `src-tauri/target/release/bundle/`.
6. Sigue el checklist de la sección 20 de este informe, con datos completamente ficticios.

## 26. Siguiente prompt recomendado

```
Continuemos desde el informe de cierre de FASE 1+2+3+4 (commit dd72ebb). Antes de avanzar más,
necesito que ejecutes el checklist de validación manual de Windows (sección 20 del informe) con
datos completamente ficticios en un vault desechable, y que me reportes honestamente qué pasó en
cada punto — especialmente si el PDF exportado del Plan de Seguridad se ve bien, si el hard-delete
de un paciente/recurso realmente elimina todo lo esperado y nada más, y si la nueva vista simple de
Google Calendar se entiende sin necesitar la sección avanzada.

Cuando tengas los resultados, decide (no hace falta la respuesta ahora):
1. Si el checklist encuentra defectos reales, los corrijo antes de cualquier otra cosa.
2. Si quieres seguir con lo que queda pendiente del roadmap de Fase 18 (artefacto de release,
   seguridad/privacidad pre-RC, informe final con gate RC1 — tareas ya trackeadas).
3. Si hay alguna decisión de producto nueva que quieras que evalúe (p. ej. implementar Línea
   temporal de verdad, o revisar el modelo de credenciales compartidas de Google Calendar).

Recuerda: nunca inventes datos clínicos reales, nunca toques SQLCipher/Argon2id/AES-GCM/backup-
restore/single-instance sin detenerte a explicar primero, y si algo te obliga a apartarte de estas
instrucciones, detente y pregunta.
```
