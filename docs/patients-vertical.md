# Primer vertical funcional: Pacientes (Fase 1.5)

Documento técnico de la Fase 1.5. Complementa `docs/ARCHITECTURE.md`
(secciones 2 y 4) y demuestra en código la capa de arquitectura completa:
**SQLCipher → Repository → Service → Tauri Command → IPC tipado → React → UI**.

## Capas y responsabilidades

```
React (features/patients/*)
   │  invoke('create_patient', { input })
   ▼
commands::patients   (src-tauri/src/commands/patients.rs)
   │  — capa fina: SOLO obtiene la conexión y delega. Sin SQL, sin reglas.
   │  state.with_connection(|conn| services::patients::create_patient(conn, input))
   ▼
security::session::VaultSession::with_connection
   │  — único punto de acceso a la conexión. Si el vault está bloqueado,
   │    devuelve Err(VaultLockedError) ANTES de llegar a services/repositories.
   ▼
services::patients   (src-tauri/src/services/patients.rs)
   │  — reglas de negocio: validación autoritativa (nombre obligatorio,
   │    estado válido, RUT con dígito verificador, fechas con formato),
   │    generación del UUID, orquestación del repositorio.
   ▼
repositories::patients   (src-tauri/src/repositories/patients.rs)
   │  — SQL puro. No sabe qué es "válido", solo ejecuta INSERT/SELECT/UPDATE.
   ▼
SQLite + SQLCipher (vault.db, Fase 1.2/1.3)
```

Ningún comando recibe SQL ni un identificador de tabla/columna desde el
frontend — cada operación (`create_patient`, `list_patients`, etc.) es una
función con nombre y forma fijos. No existe, en ningún punto del código, un
comando genérico tipo `run_sql(query)`.

## Archivos creados o modificados

| Archivo | Rol |
|---|---|
| `src-tauri/src/repositories/mod.rs`, `repositories/patients.rs` | SQL puro: `insert`, `find_by_id`, `list_active` (con búsqueda), `update`, `soft_delete`, `restore`. Sin `hard_delete` — no existe esa operación en todo el código. |
| `src-tauri/src/services/mod.rs`, `services/patients.rs` | Validación autoritativa + orquestación. `PatientInput` (entrada), `Patient`/`PatientListItem` (salida). |
| `src-tauri/src/services/rut.rs` | Validación de RUT chileno (módulo 11) y normalización. Reutilizable fuera de pacientes si hace falta más adelante. |
| `src-tauri/src/commands/patients.rs` | 6 comandos Tauri, todos mediados por `VaultSession::with_connection`. |
| `src-tauri/src/lib.rs` | Registro de módulos y comandos; nada de lógica nueva. |
| `src/features/patients/*` | `api.ts` (wrappers de `invoke`), `types.ts`, `schema.ts` (Zod), `rut.ts` (réplica de la validación para feedback inmediato), `PatientForm.tsx`, `PatientsListScreen.tsx`, `PatientCreateScreen.tsx`, `PatientEditScreen.tsx`, `PatientDetailScreen.tsx`. |
| `src/app/Layout.tsx` | Barra superior (nombre de la app, cambiar contraseña, bloquear) + `<Outlet/>` del router. |
| `src/App.tsx` | Ahora monta `HashRouter` con las rutas de pacientes cuando el vault está desbloqueado, en vez del placeholder de la Fase 1.4. |
| `src/shared/useGlobalShortcut.ts` | Hook genérico ⌘/Ctrl+`tecla`, usado hoy para ⌘/Ctrl+N (nuevo paciente) y reutilizable tal cual para ⌘/Ctrl+K (búsqueda global) cuando corresponda. |
| `src/components/ui/TextField.tsx`, `Select.tsx` | Primitivos de formulario compartidos (ya existían `Button`, `PasswordField` de la Fase 1.4). |
| `src-tauri/Cargo.toml` | Nueva dependencia: `uuid` (feature `v4` + `rng-getrandom`) — ya estaba en el árbol de forma transitiva vía Tauri; se hizo explícita para generar los IDs de paciente. |

## Modelo de datos usado

Exactamente el de `docs/ARCHITECTURE.md` — no se agregó ninguna columna. La
información clínica (`patient_clinical_profile`) no se toca en esta fase; el
tipo `Patient` que viaja por IPC no incluye ningún campo de esa tabla.

## Decisiones relevantes

1. **Creación en dos capas de DTO, no tres.** Se consideró tener un DTO de
   entrada por comando, uno de validación intermedio, y el modelo de
   dominio — se simplificó a dos: `PatientInput` (lo que llega del
   formulario, con todo opcional salvo `fullName`) y `Patient`/
   `PatientListItem` (lo que sale). La validación construye una estructura
   intermedia (`ValidatedFields`) solo dentro de la función, sin exponerla.
2. **`PatientListItem` no tiene campo `rut`.** No es una omisión de la UI:
   el tipo que sale del backend hacia React estructuralmente no puede
   llevar el RUT al listado, porque el campo no existe en el struct. Un
   test (`list_items_never_include_the_rut_field`) lo deja documentado como
   garantía, no como convención.
3. **RUT: validado en Rust, replicado en TypeScript.** El algoritmo módulo
   11 se implementó dos veces (Rust autoritativo, TypeScript para feedback
   inmediato en el formulario) porque son procesos distintos (backend vs.
   UI) sin una forma práctica de compartir código entre Rust y TypeScript en
   este proyecto. Ambas implementaciones se probaron por separado con los
   mismos vectores verificados a mano (ver `services/rut.rs` y
   `features/patients/rut.ts`).
4. **RUT no obligatorio.** Tal como se pidió: el campo es `Option<String>`
   en todo el recorrido, y solo se valida el formato si viene con contenido.
5. **`update` reemplaza todos los campos, no hace PATCH parcial.** El
   formulario de edición siempre llega con el estado completo del paciente
   (se precarga con `get_patient` antes de mostrarse), así que un
   "reemplazo total validado" es más simple y menos propenso a errores que
   fusionar campos parciales, sin perder ninguna capacidad real.
6. **Búsqueda con `LIKE` sobre `full_name`/`preferred_name`, no FTS5.**
   FTS5 queda para la fase de búsqueda global (Fase 8 según
   `ARCHITECTURE.md`, o antes si se prioriza). Para el volumen de una sola
   psicóloga, un `LIKE` indexado por `ORDER BY full_name` es una consulta
   real contra SQLite (no un filtro en memoria del lado de React) y cumple
   el requisito tal como se pidió ("todavía no necesito el buscador global
   completo"). Se escapan `%`, `_` y `\` en el término de búsqueda para que
   caracteres especiales no rompan el patrón.
7. **Ficha del paciente con navegación por pestañas ya definida.** Las 9
   secciones (Resumen, Antecedentes, Sesiones, Formulación, Objetivos,
   Evaluaciones, Documentos, Pagos, Línea temporal) existen como
   navegación real desde ahora; solo "Resumen" tiene contenido. Las demás
   muestran "Próximamente" a través de una lista explícita
   (`SECTIONS_WITH_REAL_CONTENT`) — agregar una sección en una fase futura
   es agregar su componente y sumarla a esa lista, no rediseñar la ficha.
8. **Router real (`react-router-dom`, `HashRouter`).** Se introduce en esta
   fase porque ahora sí hay múltiples pantallas con navegación real
   (listado, crear, ver, editar). Se usa `HashRouter` en vez de
   `BrowserRouter` porque el WebView de Tauri sirve los assets por un
   protocolo propio, no un servidor HTTP con *fallback* de rutas — con
   `HashRouter` no hace falta configurar eso.
9. **⌘/Ctrl+N implementado con un hook genérico, no una solución puntual.**
   `useGlobalShortcut(key, handler)` ya sirve tal cual para ⌘/Ctrl+K más
   adelante; no hay que rehacer la detección de plataforma/modificador.

## Seguridad — verificado explícitamente

- **React nunca accede a SQLite.** Todo pasa por `invoke()` hacia comandos
  Tauri con nombre propio.
- **Nada de pacientes en `localStorage` ni en `Zustand persist`.** Esta
  fase ni siquiera usa Zustand para pacientes — el estado vive en
  componentes React (`useState`) y se vuelve a pedir al backend cuando
  hace falta (por ejemplo, al volver al listado). No hay caché persistente
  de datos clínicos en el cliente.
- **Sin `run_sql` genérico.** Cada comando es una operación de negocio
  específica.
- **Vault bloqueado ⇒ operaciones imposibles, no solo ocultas.** Cada
  comando obtiene la conexión exclusivamente vía
  `VaultSession::with_connection`, que devuelve `Err` si no hay sesión
  desbloqueada. No existe una ruta alternativa de código para llegar a la
  conexión. Probado explícitamente
  (`patient_operations_are_rejected_at_the_backend_while_locked`) para las
  cinco operaciones (crear, listar, leer, actualizar, archivar).
- **Sin datos de pacientes en logs ni en mensajes de error técnicos.**
  `PatientError::Database` nunca interpola el error de `rusqlite` (que
  podría incluir valores de columnas) en el mensaje mostrado — solo un
  texto genérico ("error interno al acceder a la base de datos"). Los
  errores de validación si describen el problema (p. ej. "RUT inválido"),
  pero nunca repiten datos de otro paciente ni contenido de la base.
- **Soft delete real, sin ruta de borrado físico.** `repositories::patients`
  no tiene ninguna función `DELETE FROM patients`. La única forma de
  "eliminar" es `soft_delete` (pone `deleted_at`), y el único comando
  expuesto es `archive_patient`. `restore_patient` existe y funciona
  (revierte `deleted_at` a `NULL`), aunque todavía no hay una pantalla de
  "papelera" — se puede invocar por backend/tests, tal como se autorizó.

## Tests ejecutados

`cargo test` en `src-tauri/`: **111/111 en verde** (87 de las Fases
1.1–1.4 sin cambios + 24 nuevos). `cargo clippy --all-targets`: sin
advertencias. `npm run build`/`npm run lint`: sin errores.

| # | Requisito pedido | Test(s) |
|---|---|---|
| 1 | Crear paciente | `services::patients::creates_a_patient_with_defaults` |
| 2 | Leer paciente | `services::patients::reads_a_created_patient_back` |
| 3 | Actualizar paciente | `services::patients::updates_a_patient` |
| 4 | Soft delete | `services::patients::archiving_soft_deletes_and_hides_from_listing_but_keeps_the_row` |
| 5 | Paciente eliminado no aparece en el listado normal | mismo test (verifica `list_patients`) |
| 6 | El registro sigue existiendo en la base | mismo test (verifica con `get_patient_including_deleted`) |
| 7 | Restaurar un paciente soft-deleted | `services::patients::restoring_a_soft_deleted_patient_brings_it_back_to_the_listing` |
| 8 | Buscar pacientes (contra la base real) | `services::patients::searches_patients_by_name_against_the_real_database` |
| 9 | Validar RUT correctamente | 9 tests en `services::rut::tests` (aceptación con/sin dígito K, formatos con y sin puntos/guion, rechazo de dígito verificador incorrecto, cuerpo no numérico, verificador inválido, entrada vacía) |
| 10 | Rechazar datos inválidos desde Rust | `rejects_empty_full_name`, `rejects_invalid_status`, `rejects_invalid_rut`, `rejects_malformed_birth_date` |
| 11 | Operaciones rechazadas con el vault bloqueado | `security::session::patient_operations_are_rejected_at_the_backend_while_locked` (las 5 operaciones) |
| 12 | Cerrar/reabrir la aplicación y comprobar persistencia | `security::session::patient_survives_a_full_close_and_reopen_of_the_app` (a nivel de `VaultSession`) **+ verificación manual sobre la aplicación real compilada** (ver abajo) |
| 13 | Volver a correr toda la suite de las Fases 1.1–1.4 | Incluida en el mismo `cargo test` — los 87 tests previos siguen en verde sin modificaciones |

## Prueba manual realizada (aplicación real, no solo tests)

Igual que en la Fase 1.4: se compiló con `tauri build --debug`, se ejecutó
bajo Xvfb, y se manejó con clics y tecleo reales (`xdotool`), con capturas
de pantalla en cada paso, siguiendo exactamente el criterio de terminado:

1. Crear vault → desbloquear.
2. **⌘/Ctrl+N** abre el formulario de nuevo paciente (atajo real, no solo
   el botón).
3. Formulario completo (datos personales, contacto, contacto de
   emergencia, administrativo) con un RUT válido (`12.345.678-5`,
   verificado a mano) → paciente creado → ficha muestra los datos reales,
   normalizados (RUT sin puntos).
4. Listado muestra el paciente real (sin RUT visible), con buscador
   funcional: una búsqueda sin coincidencias muestra "No se encontraron
   pacientes" (consulta real a la base, no un filtro falso), una búsqueda
   parcial ("perez") sí lo encuentra.
5. Navegación por las pestañas de la ficha — una sección sin contenido
   todavía ("Sesiones") muestra "Próximamente".
6. Editar paciente (cambiar teléfono) → guardar → ficha refleja el cambio.
7. **Cierre completo del proceso de la aplicación** (no solo bloquear) y
   **reapertura real** del binario.
8. La app arranca en estado `Locked` (no `NoVault` — confirma que
   `vault.meta.json`/`vault.db` persistieron en disco).
9. Desbloquear con la misma contraseña → el paciente editado sigue ahí,
   con el teléfono actualizado — persistencia real en SQLCipher confirmada
   fuera de un test automatizado.
10. Archivar paciente (con el diálogo de confirmación mostrado) → vuelve
    al listado → el paciente ya no aparece.

## Limitaciones y decisiones que necesito que apruebes

1. **Sin papelera visual.** `restore_patient` existe y funciona (backend +
   test), pero no hay pantalla para ver/restaurar pacientes archivados
   todavía — tal como autorizaste explícitamente para esta fase.
2. **Búsqueda solo por nombre/nombre preferido, no por RUT.** Se decidió
   así para no tener que resolver si buscar por RUT cuenta como "mostrarlo"
   en algún sentido. Si prefieres poder buscar por RUT (aunque no se
   muestre en el resultado), es un cambio acotado a
   `repositories::patients::list_active`.
3. **`update_patient` reemplaza todos los campos** (no hace fusión
   parcial). Funciona bien con el flujo actual (el formulario siempre
   precarga todo el paciente), pero si en el futuro se llama a
   `update_patient` desde un lugar que solo tiene algunos campos (por
   ejemplo, un cambio rápido de estado desde el listado), habría que
   revisar ese comando específico o agregar uno de "patch" más acotado.
4. **Validación de fecha estructural, no calendárica completa.** Se valida
   el formato `AAAA-MM-DD` y que mes/día estén en rango (1-12 / 1-31), pero
   no se verifica, por ejemplo, que el 30 de febrero sea inválido —
   evita agregar una dependencia de fechas (`chrono`/`time`) solo para
   este caso. Si prefieres validación calendárica exacta, es un cambio
   acotado, aunque probablemente implique agregar esa dependencia.
5. **`uuid` como nueva dependencia explícita.** Ya estaba en el árbol de
   forma transitiva (vía Tauri) en la misma versión, así que no se agregó
   peso real al binario — se hizo explícita porque ahora el código de la
   aplicación la usa directamente para generar IDs de pacientes.
