# Fase 6.1 — Ubicación geográfica y estadísticas de pacientes

Documento técnico de la Fase 6.1 (3 de septiembre de 2026). Complementa
`docs/patients-vertical.md` (vertical Pacientes, Fase 1.5) y
`docs/ARCHITECTURE.md` (sección 4, subsección "Pacientes"). Extensión
pequeña y aislada, autorizada explícitamente sobre una vertical ya
existente — no es un vertical nuevo ni una fase de alcance comparable a
Sesiones/Objetivos/Antecedentes.

## Qué se implementó

1. Dos campos nuevos, opcionales, en la ficha de paciente: **región** y
   **comuna** de residencia, validados contra un catálogo cerrado de Chile
   (nunca texto libre).
2. Una pantalla nueva e independiente, **Estadísticas** (no dentro del
   Dashboard), con:
   - Conteo de pacientes con/sin ubicación registrada.
   - Distribución por región (donut) y por comuna (barras horizontales).
   - Filtro Activos (por defecto) / Todos.
   - Categorías con menos de 3 pacientes agrupadas en **"Otras"**, tanto
     en región como en comuna, para que ninguna categoría pequeña
     identifique indirectamente a una persona.
   - Sin click-through: ningún gráfico ni fila navega hacia un paciente.

## Capas y responsabilidades

Mismo patrón de siempre — `SQLCipher → Repository → Service → Tauri
Command → IPC tipado → React`:

```
repositories::patients::geographic_distribution(conn, include_archived)
   │  — SQL puro, agregación real vía GROUP BY. Nunca trae la lista
   │    completa de pacientes a Rust para contarla a mano.
   ▼
services::patients::geographic_statistics(conn, include_archived)
   │  — agrupa categorías <3 en "Otras" (group_small_categories).
   │    Reglas de negocio, no SQL.
   ▼
commands::patients::get_geographic_statistics
   │  — capa fina, igual que el resto de comandos de pacientes.
   ▼
src/features/statistics/{api.ts,types.ts,StatisticsScreen.tsx}
```

Para crear/editar región y comuna se reutilizan los comandos existentes
(`create_patient`/`update_patient`) — no hay comandos nuevos para eso, solo
dos campos más en `PatientInput`/`Patient`.

## Migración `V2`

```sql
ALTER TABLE patients ADD COLUMN region TEXT;
ALTER TABLE patients ADD COLUMN commune TEXT;
```

- Exclusivamente aditiva: dos `ALTER TABLE ADD COLUMN`, sin `DEFAULT`
  obligatorio, sin `DROP`, sin `DELETE`, sin backfill.
- `SCHEMA_V1` (`src-tauri/src/db/migrations.rs`) no se modificó — se
  agregó `SCHEMA_V2` como una constante nueva y `migrations()` ahora
  encadena `M::up(SCHEMA_V1).foreign_key_check(), M::up(SCHEMA_V2).foreign_key_check()`.
- Verificado explícitamente con tres tests nuevos en
  `db::migrations::tests`:
  - `v2_migration_preserves_all_existing_patient_data` — crea un vault
    manualmente en `V1`, inserta pacientes con todos los campos
    completos, migra a `V2` con `run_migrations`, confirma que **todos**
    los campos anteriores siguen intactos y que `region`/`commune` quedan
    `NULL` (no forzados a ningún valor).
  - `fresh_database_gets_region_and_commune_columns_from_v1_plus_v2` —
    una base nueva llega directo a `V1+V2` con ambas columnas.
  - `v2_migration_is_idempotent_like_v1` — correr las migraciones dos
    veces no falla ni duplica nada.

## Catálogo cerrado Región/Comuna — fuente única de verdad

- 16 regiones, 346 comunas de Chile, más el valor reservado
  `"Extranjero"` (para pacientes residentes fuera del país — no es una
  región del catálogo, no tiene comunas).
- Fuente: dataset atribuido a DEIS (Departamento de Estadísticas e
  Información de Salud, Ministerio de Salud de Chile), obtenido vía
  GitHub (`jhonattanvargas/regiones`). El dataset original era anterior a
  2018 (no incluía la Región de Ñuble, creada ese año por separación de la
  Región del Biobío) — se re-derivó estructuralmente usando la propia
  agrupación provincial ya presente en los datos (la "Provincia de Ñuble"
  del dataset original pasó a ser la Región de Ñuble; las otras tres
  provincias del Biobío original se mantuvieron como Región del Biobío),
  sin inventar ni un solo nombre de comuna. Verificado: 16 regiones, 346
  comunas totales, cero duplicados entre regiones.
- **Un único archivo**, `src/data/chile-geo.json`, es la fuente real:
  - Rust lo incluye en el binario en tiempo de compilación con
    `include_str!` (`src-tauri/src/geo.rs`).
  - TypeScript lo importa directamente como módulo (`features/patients/geo.ts`),
    habilitado agregando `"resolveJsonModule": true` a `tsconfig.app.json`
    (única modificación de configuración que requirió esta arquitectura;
    no es una dependencia nueva ni una herramienta nueva).
  - No existen dos copias de los 346 nombres de comuna en el código: si el
    archivo cambiara, ambos lados verían exactamente el mismo contenido
    sin sincronización manual — el problema de divergencia que la
    aprobación anticipaba (con su salida de "tests de divergencia si no
    puede existir un único archivo") queda resuelto estructuralmente, no
    con un test.
  - `geo.rs` sí incluye un test de integridad
    (`test_catalog_matches_the_expected_shape_of_chile`) — no detecta
    divergencia entre dos copias (no existen dos copias), detecta
    corrupción o un error de edición del único archivo fuente.

## Validación (`services::patients::validate_geo`) — autoritativa en Rust

| # | Caso | Resultado |
|---|---|---|
| 1 | Región y comuna ambas ausentes | Válido — "no informado" |
| 2 | Región conocida + comuna que le pertenece | Válido |
| 3 | Región conocida + comuna real pero de otra región | Rechazado (`CommuneNotInRegion`) |
| 4 | Región `"Extranjero"` + comuna informada | Rechazado (`ForeignRegionCannotHaveCommune`) |
| 5 | Comuna informada sin región | Rechazado (`CommuneRequiresRegion`) |
| 6 | Región informada, comuna ausente | Válido — región sola es un caso legítimo, no forzado a completarse |
| 7 | Región desconocida (no está en el catálogo ni es `"Extranjero"`) | Rechazado (`UnknownRegion`) |
| 8 | Strings en blanco (`"   "`, `""`) | Se normalizan a `None` vía `none_if_blank` antes de validar |

El frontend usa `<select>` dependientes (nunca texto libre) precargados
desde el mismo catálogo, así que en la práctica el usuario no puede enviar
un valor inválido — pero la validación de arriba es la que realmente
decide: el frontend es solo para UX, igual que el resto de validaciones de
esta vertical (RUT, fechas, estado).

## Estadísticas — agregación y privacidad

- `repositories::patients::geographic_distribution(conn, include_archived)`
  hace tres consultas `GROUP BY` (con/sin ubicación, por región, por
  comuna) — nunca trae la lista completa de pacientes a Rust ni al
  frontend para contarla ahí.
- `services::patients::group_small_categories` agrupa en `"Otras"`
  cualquier categoría con **menos de 3 pacientes**, aplicado igual a
  región y a comuna. Orden resultante: de mayor a menor cantidad.
- El DTO que sale por IPC (`GeoDistributionItem { label, count }`,
  `GeographicStatistics { withLocation, withoutLocation, byRegion,
  byCommune }`) no tiene ningún campo que permita llegar a un paciente
  individual — verificado estructuralmente por
  `geographic_distribution_items_never_carry_identifying_patient_data`.
- La pantalla `StatisticsScreen.tsx` no tiene ningún `onClick`/`Link`/
  `navigate()` hacia una ficha de paciente — el único evento de la
  pantalla es el toggle local Activos/Todos.
- El donut (región) y las barras horizontales (comuna) son SVG/CSS nativo
  — no se instaló ninguna librería de gráficos (Recharts/Chart.js/D3
  quedaron explícitamente descartados por la aprobación). Colores
  derivados del único token de acento (`--color-accent`) vía `color-mix`,
  nunca hexadecimales nuevos escritos a mano; "Otras" usa un tono neutro
  aparte para distinguirse visualmente del resto.

## Minimización de exposición

`PatientListItem` (lo que devuelve el listado de pacientes) **no se
tocó** — sigue sin `rut` (Fase 1.5) y ahora tampoco lleva `region`/
`commune`. Región y comuna solo viajan en el tipo `Patient` de ficha
completa, igual que el resto de datos de contacto. Decisión explícita de
la aprobación (§7), consistente con el mismo principio ya aplicado al RUT.

## Frontend

- `src/features/patients/geo.ts` — helpers sobre el catálogo compartido:
  `REGION_OPTIONS` (16 regiones + `"Extranjero"`), `communesForRegion(region)`.
- `PatientForm.tsx` — nueva sección "Ubicación" con dos `<Select>`
  dependientes. La comuna se limpia automáticamente cuando el usuario
  cambia de región (nunca en el primer render, para no perder la comuna
  ya guardada de un paciente existente al abrir el formulario de
  edición); se deshabilita cuando no hay región elegida o la región es
  `"Extranjero"`.
- `PatientDetailScreen.tsx` — región y comuna se muestran de forma
  discreta en la sección "Contacto" de la ficha, junto a teléfono/correo/
  dirección — ningún tratamiento especial ni destacado.
- `src/features/statistics/` — feature nueva e independiente
  (`types.ts`, `api.ts`, `StatisticsScreen.tsx`), con su propia ruta
  `/statistics` y su propio ítem de navegación "Estadísticas" en
  `Layout.tsx` (no es una pestaña del Dashboard ni de la ficha de
  paciente).

## Decisiones relevantes

1. **Región sola es un caso válido.** La aprobación dejaba explícitamente
   sin resolver el caso "región informada, comuna ausente" — se decidió
   permitirlo (no forzar a completar la comuna), consistente con la
   filosofía general del proyecto de no obligar a completar datos que el
   usuario no tiene a mano.
2. **Agregación en el repositorio, agrupación "Otras" en el servicio.**
   El conteo real (`GROUP BY`) es SQL puro en `repositories::patients`;
   decidir el umbral de 3 y armar la categoría `"Otras"` es una regla de
   negocio y vive en `services::patients` — misma separación de capas que
   el resto del proyecto.
3. **`tsconfig.app.json` con `resolveJsonModule: true`.** Cambio de
   configuración mínimo y deliberado, no una herramienta nueva ni una
   dependencia — es lo que permite que la arquitectura de fuente única
   (un solo `chile-geo.json` para Rust y TypeScript) sea real y no solo
   una intención.
4. **Sin librería de gráficos.** Donut y barras horizontales en SVG/CSS
   nativo, explícitamente para no agregar una dependencia frontend nueva
   — la aprobación pedía detenerse si el SVG se volvía frágil o
   complicado; no fue el caso (un `<circle>` con `strokeDasharray`/
   `strokeDashoffset` por segmento, más una lista de barras con `width%`).

## Excepción aprobada — actualizaciones mecánicas de tests

Agregar `region`/`commune` a `NewPatientRow` (repositorio) y `PatientInput`
(servicio) — ambos structs compartidos por **todas** las verticales de la
aplicación, porque cada una necesita crear un paciente ficticio para sus
propios tests — rompió la compilación de los `#[cfg(test)]` que construyen
ese struct en archivos de otras verticales, incluidos varios que la
aprobación de Fase 6.1 marcó explícitamente como "no tocar". Se corrigió
agregando exactamente `region: None, commune: None,` a cada literal
exhaustivo. Esta excepción fue evaluada, confirmada por diff línea por
línea, y **aprobada explícitamente** antes del commit de cierre — nunca
aplicada por iniciativa propia sin aprobación.

Archivos afectados, con la cantidad exacta de líneas modificadas:

| Archivo | Líneas | Vertical |
|---|---|---|
| `src-tauri/src/security/session.rs` | 4 (2 helpers) | Seguridad/sesión |
| `src-tauri/src/repositories/session_notes.rs` | 2 | Sesiones/notas versionadas |
| `src-tauri/src/services/sessions.rs` | 2 | Sesiones/notas versionadas |
| `src-tauri/src/repositories/sessions.rs` | 2 | Sesiones/notas versionadas |
| `src-tauri/src/repositories/goals.rs` | 2 | Objetivos |
| `src-tauri/src/services/goals.rs` | 2 | Objetivos |
| `src-tauri/src/repositories/session_goals.rs` | 2 | Objetivos |
| `src-tauri/src/repositories/goal_indicators.rs` | 2 | Objetivos |
| `src-tauri/src/repositories/patient_clinical_profile.rs` | 2 | Antecedentes |
| `src-tauri/src/services/patient_clinical_profile.rs` | 2 | Antecedentes |
| `src-tauri/src/services/appointments.rs` | 2 | Agenda |

**Total: 11 archivos, 24 líneas insertadas, 0 líneas eliminadas.**

Confirmado por `git diff`, archivo por archivo:

- El 100% de los cambios cae dentro de un bloque `mod tests { ... }`
  (confirmado por el contexto de función que `git diff` imprime junto a
  cada `@@`, que en los 12 hunks totales siempre dice `mod tests {`).
- El 100% de las líneas agregadas son, literalmente, `region: None,` o
  `commune: None,` — verificado filtrando todas las líneas `+` del diff
  combinado de los 11 archivos: no aparece ninguna otra línea.
- Cero líneas eliminadas en cualquiera de los 11 archivos.
- Motivo técnico: Rust exige que los literales de struct sean
  exhaustivos: agregar un campo a `NewPatientRow`/`PatientInput` obliga a
  actualizar **todo** punto del código que construye uno de esos
  structs, sin excepción — no hay forma de agregar los dos campos nuevos
  sin que esto ocurra, dado que la arquitectura aprobada explícitamente
  pedía reutilizar esos structs compartidos (no crear una segunda
  variante paralela).
- Ninguna lógica productiva, regla de negocio, comportamiento de esas
  verticales, arquitectura, modelo de seguridad, Google Calendar,
  versionado de notas, objetivos o antecedentes se modificó — confirmado
  por el diff (cero cambios fuera de `#[cfg(test)]`) y por la suite
  completa de tests (`cargo test`) permaneciendo en 302/302 verde,
  incluidos los tests propios de cada uno de estos 11 archivos.

## Tests ejecutados

`cargo test` en `src-tauri/`: **302/302 en verde** (271 de las Fases 1–6
sin cambios + 31 nuevos: 5 en `geo::tests`, 3 en `db::migrations::tests`,
6 en `repositories::patients::tests`, 17 en `services::patients::tests`).
`cargo clippy --all-targets`: sin advertencias. `npm run build`: sin
errores de tipos. `npm run lint`: 16 advertencias (15 preexistentes de
fases anteriores, sin cambios + 1 nueva de la misma categoría ya aceptada
en el resto del código — `react(set-state-in-effect)` en el `useEffect`
de carga de `StatisticsScreen.tsx`, el mismo patrón *fetch-en-efecto* que
ya usan `PatientsListScreen`, `AgendaScreen`, `GoalsTab`, `SessionsTab`,
etc. — ninguna categoría nueva de advertencia).

| Capa | Tests nuevos (resumen) |
|---|---|
| `geo::tests` | Forma del catálogo (16 regiones, 346 comunas, sin duplicados, sin espacios sobrantes), `"Extranjero"` no es una región del catálogo, reconocimiento de región real vs. inventada, pertenencia de comuna a su región, separación correcta de Ñuble/Biobío. |
| `db::migrations::tests` | V2 preserva datos existentes de pacientes reales (fixture manual en V1 + migración real), una base nueva llega directo a V1+V2, migrar dos veces es idempotente. |
| `repositories::patients::tests` | Insertar con y sin región/comuna, actualizar región/comuna, `geographic_distribution` cuenta con/sin ubicación vía `GROUP BY`, trata `"Extranjero"` como región sin comuna, respeta el filtro de archivados. |
| `services::patients::tests` | Las 8 reglas de `validate_geo` (una por caso de la tabla de arriba, incluidos límites), crear sin ubicación, actualizar limpiando la ubicación de vuelta a "no informado", strings en blanco normalizados, umbral de agrupación "Otras" en 2/3 pacientes exactos, conteo con/sin ubicación, filtro archivados en las estadísticas, DTO sin campos identificables, catálogo vacío sin pacientes. |

## Prueba manual realizada (aplicación real, no solo tests)

Igual que en fases anteriores: compilada con `cargo build` (backend) +
`npm run build` (frontend embebido), ejecutada bajo Xvfb con clics y
tecleo reales (`xdotool`), sobre un **vault de prueba desechable** (el
vault de pruebas manuales preexistente en este entorno se renombró a
`com.jpcaamano.cuadernoclinico.pre-fase6.1-manual-test-backup`, no se
borró, siguiendo la misma práctica ya usada en fases anteriores):

1. Crear vault nuevo → confirmar código de recuperación → app
   desbloqueada, nav "Estadísticas" visible entre "Agenda" y "Ajustes".
2. Pantalla "Estadísticas" en vacío: 0/0, "Sin datos de región/comuna
   para mostrar", nota de privacidad visible.
3. Formulario "Nuevo paciente": sección "Ubicación" con selects Región/
   Comuna dependientes; comuna deshabilitada hasta elegir región.
4. Siete pacientes ficticios creados/editados cubriendo: región+comuna
   válidas (3 en Región de Valparaíso/Quillota, 2 en Región Metropolitana
   de Santiago/Ñuñoa), `"Extranjero"` sin comuna, y un paciente sin
   ubicación — la comuna se deshabilita correctamente al elegir
   `"Extranjero"`.
5. Ficha de cada paciente: región/comuna visibles de forma discreta junto
   a teléfono/correo/dirección.
6. Estadísticas con los 7 pacientes: 6 con ubicación / 1 sin ubicación;
   por región, Valparaíso (3, no agrupada) y "Otras" (Metropolitana 2 +
   Extranjero 1 = 3, agrupadas, cada una <3 individualmente); por comuna,
   Quillota (3, no agrupada) y "Otras" (Ñuñoa 2, agrupada) — el umbral de
   agrupación funcionando en vivo con datos reales, no solo en tests.
7. Filtro Activos/Todos verificado con un cambio de estado real: se
   archivó uno de los tres pacientes de Quillota → en "Activos" Región de
   Valparaíso cae a 2 (<3) y pasa a agruparse en "Otras" (100% agrupado);
   en "Todos" vuelve a mostrarse individual (3, no agrupada) — la
   agregación se recalcula correctamente según el filtro, no es un valor
   cacheado. Paciente restaurado al finalizar la verificación.
8. Regresión funcional en el mismo vault: pestaña "Antecedentes" de un
   paciente (Fase 6) sin cambios; Dashboard reflejando el conteo real de
   pacientes activos (7); Agenda cargando sin errores.
9. **Bloqueo manual** → pantalla de desbloqueo → **desbloqueo con la
   misma contraseña** → la app vuelve exactamente a donde estaba (Agenda)
   — persistencia de sesión confirmada tras un ciclo de bloqueo real, no
   solo un test automatizado.

## Auditoría de privacidad

- **Logs:** revisados los logs del proceso Tauri generados durante toda
  la verificación manual (creación de vault, 7 pacientes, edición,
  estadísticas, archivar/restaurar, bloqueo/desbloqueo) — la única línea
  de log es la de migración (`Database migrated to version 2`); ningún
  nombre, región ni comuna de los pacientes ficticios usados apareció en
  ningún log. Confirmado también que ningún archivo nuevo o modificado de
  esta fase (`geo.rs`, `repositories::patients`, `services::patients`,
  `commands::patients`) agrega ningún `println!`/`log::*`/`dbg!`.
- **Google Calendar:** `src-tauri/src/calendar/*` no se tocó en esta
  fase (confirmado por `git status`) — ningún dato geográfico puede
  llegar ahí porque el código que arma el evento espejo no cambió.
- **`localStorage`/`sessionStorage`:** ausentes en todo el código nuevo
  (`features/statistics/`, `features/patients/geo.ts` y los archivos de
  pacientes modificados) — confirmado por búsqueda de texto.
- **DTO de estadísticas:** `GeoDistributionItem`/`GeographicStatistics`
  solo tienen `label`/`count`/`withLocation`/`withoutLocation` — sin
  ningún campo que permita llegar a un paciente, verificado
  estructuralmente por test y visualmente en la app real.
- **Umbral "Otras":** aplicado y verificado en vivo (ver prueba manual,
  punto 7) tanto en tests automatizados como en la aplicación real.
- **Sin click-through:** confirmado por inspección de código
  (`StatisticsScreen.tsx` no tiene ningún `onClick`/`Link`/`navigate()`
  hacia una ficha de paciente) y por uso real de la pantalla.

## Limitaciones y decisiones que quedan documentadas, no ocultas

1. **Sin geocodificación, mapas ni coordenadas.** Explícitamente fuera de
   alcance de la aprobación — región/comuna son categorías administrativas
   de texto, no datos de ubicación en el sentido de mapas/GPS.
2. **Catálogo fijo en el binario.** Un cambio administrativo futuro de
   Chile (otra región nueva, fusión de comunas) requeriría actualizar
   `src/data/chile-geo.json` y recompilar — no hay actualización dinámica
   del catálogo, algo que nunca se pidió y sería una superficie de riesgo
   innecesaria para datos que cambian con una frecuencia de años, no de
   días.
