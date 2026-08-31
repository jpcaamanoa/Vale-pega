# Esquema y migraciones (Fase 1.3)

Este documento registra qué se implementó, en qué difiere del esquema
propuesto en `docs/ARCHITECTURE.md` (sección 4), y qué se probó. Complementa
a `docs/sqlcipher.md` (Fase 1.2).

## Herramienta de migraciones

`rusqlite_migration` 2.6.0. **No fue la primera opción probada**: la versión
que resuelve automáticamente para nuestro `rust-version` declarado (1.3.1)
depende de `rusqlite ^0.32.1`, que es incompatible con el `rusqlite 0.40.2` +
SQLCipher que fijamos en la Fase 1.2 — Cargo lo rechaza de inmediato porque
dos versiones de `libsqlite3-sys` no pueden coexistir enlazando la misma
librería nativa (`links = "sqlite3"`). Es exactamente el tipo de problema de
compatibilidad que se pidió verificar antes de fijar la dependencia.

`rusqlite_migration` 2.6.0 sí es compatible con `rusqlite 0.40.2` (mismo
`libsqlite3-sys`, sin conflicto), pero exige Rust 1.95 y el entorno tenía
1.94.1. **Decisión tomada:** actualizar el toolchain de Rust a la versión
estable más reciente (`rustup update stable` → 1.98.0) en vez de degradar a
una versión más vieja de SQLCipher/rusqlite ya aprobada, o de escribir un
runner de migraciones propio. Es una actualización de compilador, no un
cambio de arquitectura ni de librerías del proyecto — se las señalo de todas
formas porque encaja con el punto 8 de tus decisiones ("verificar
compatibilidad real antes de fijar versiones"). Riesgo conocido: no se ha
probado que macOS/Windows tengan disponible un Rust ≥1.95 en el momento en
que compiles ahí; `rustup update stable` lo resuelve igual que aquí.
`rust-version` en `Cargo.toml` se actualizó de `1.77.2` a `1.95` para que
quede documentado honestamente en vez de dejar un valor que ya no es cierto.

## Tablas implementadas

Las 25 tablas de `docs/ARCHITECTURE.md` sección 4, sin quitar ni agregar
ninguna, creadas en una única migración `V1` (no había datos previos que
preservar, así que no había razón para partirla):

`patients`, `patient_clinical_profile`, `appointments`, `sessions`,
`session_notes`, `documents`, `case_formulations`, `formulation_versions`,
`formulation_nodes`, `formulation_edges`, `technique_categories`,
`clinical_techniques`, `technique_materials`, `therapeutic_goals`,
`goal_indicators`, `goal_interventions`, `session_goals`,
`assessment_instruments`, `assessment_administrations`, `payments`,
`library_resources`, `library_tags`, `library_resource_tags`, `reminders`,
`app_settings`.

El orden de creación dentro de la migración se reordenó respecto al orden en
que aparecen en `ARCHITECTURE.md` (que sigue el orden temático del producto,
no el de dependencias) para que cada `CREATE TABLE` con una foreign key
encuentre su tabla referenciada ya creada — por ejemplo, `documents` se crea
antes que `library_resources` (que la referencia) y `technique_categories`/
`clinical_techniques` se crean antes que `goal_interventions` (que referencia
`clinical_techniques`). Es un reordenamiento mecánico, no cambia ninguna
columna ni relación.

## Diferencias respecto al esquema de `ARCHITECTURE.md`

Todo lo siguiente son refuerzos de integridad, no cambios de alcance. Los
marco explícitamente porque me pediste revisar justo estos puntos:

1. **Trigger `updated_at` automático en cada tabla que lo tiene.** SQLite no
   tiene equivalente a `ON UPDATE CURRENT_TIMESTAMP`; sin un trigger,
   `updated_at` se habría quedado congelado en la fecha de creación salvo que
   cada UPDATE en Rust lo seteara a mano (fácil de olvidar en algún lugar).
   Se agregó `trg_<tabla>_touch_updated_at` a las 13 tablas con esa columna.
   Test: `updated_at_is_bumped_automatically_on_update`.
2. **Integridad de nodos/conexiones de formulación.** El FK de
   `formulation_edges` a `formulation_nodes(id)` no garantiza que ambos nodos
   pertenezcan a la misma `formulation_version` — un `id` de nodo de otra
   versión también es válido para SQLite. Se agregaron dos triggers
   (`BEFORE INSERT`/`BEFORE UPDATE`) que rechazan un edge si sus nodos no
   pertenecen exactamente a la versión del propio edge. Test:
   `formulation_edge_across_different_versions_is_rejected`.
3. **`CHECK (source_node_id <> target_node_id)`** en `formulation_edges`: un
   nodo no puede conectarse consigo mismo. Test:
   `formulation_edge_self_loop_is_rejected`.
4. **`UNIQUE (formulation_id, version_number)`** en `formulation_versions`:
   no pueden coexistir dos versiones con el mismo número para la misma
   formulación (no estaba explícito en `ARCHITECTURE.md`).
5. **`CHECK (ends_at > starts_at)`** en `appointments`: una cita no puede
   terminar antes de empezar. Test: `appointment_ending_before_it_starts_is_rejected`.
6. **`CHECK (amount >= 0)`** en `payments`. Es una asunción de negocio (no
   modela reembolsos/notas de crédito como montos negativos); no toca nada
   clínico, así que la agregué directamente, pero la marco por si en el
   futuro quieres registrar una devolución — habría que decidir si eso es un
   pago con estado propio o un monto negativo permitido.
7. **`CHECK (length(sha256_plaintext) = 64)`** y **`CHECK (size_bytes >= 0)`**
   en `documents`: integridad básica de metadatos de archivo.
8. **`CHECK` de 0/1 explícito** en las columnas booleanas-como-entero
   (`is_locked`, `is_current`, `is_clinical`, `is_custom`), y
   **`CHECK (version >= 1)`**, **`CHECK (duration_minutes IS NULL OR duration_minutes > 0)`**
   en `sessions`.
9. **`CHECK (related_entity_type IS NULL OR related_entity_type IN (...))`**
   en `reminders`: `related_entity_type`/`related_entity_id` siguen siendo
   una referencia polimórfica sin FK real (SQLite no soporta FK condicional a
   "una de varias tablas posibles" sin bastante complejidad adicional) — eso
   ya estaba implícito en `ARCHITECTURE.md` y no lo cambié, solo acoté los
   valores permitidos de `related_entity_type` a un conjunto conocido.
   **Limitación que dejo explícita:** `related_entity_id` no está validado
   por la base de datos contra la tabla real que le corresponda; esa
   integridad tendrá que garantizarla la capa de servicio en Rust cuando se
   implemente esa funcionalidad (Fase 2+).
10. **Ampliación del ciclo de vida de `session_notes`** con `CHECK`
    consistentes: `is_locked = 1` exige `closed_at` no nulo (y viceversa),
    `is_current = 0` exige `superseded_at` no nulo (y viceversa). Ya estaba
    descrito en prosa en `ARCHITECTURE.md`/nuestra conversación; aquí quedó
    también como restricción de base de datos, no solo como convención de la
    capa de aplicación.

## Algo que decidí NO agregar, y por qué

Consideré una restricción `CHECK (total_score >= 0)` en
`assessment_administrations` por paralelismo con `payments`/`documents`, pero
**no la agregué**: algunos instrumentos psicológicos usan puntajes
estandarizados que pueden ser negativos (p. ej. z-scores). Restringir esto a
nivel de base de datos habría bloqueado datos clínicos legítimos para ciertos
instrumentos sin que yo tenga el contexto clínico para saber cuáles vas a
usar. Te lo dejo señalado en vez de decidirlo unilateralmente en cualquiera
de las dos direcciones.

## Decisión explícitamente diferida (no implementada todavía)

**Modo WAL (`PRAGMA journal_mode = WAL`)**: no lo activé en esta fase. Tiene
implicancias de seguridad que no verifiqué todavía (si SQLCipher cifra
completamente los archivos `-wal`/`-shm` tan igual como el archivo principal
— debería, pero no lo he probado con un test dedicado) y de backup (los
archivos `-wal`/`-shm` deben incluirse en cualquier copia si no se hace
`checkpoint` antes). Prefiero activarlo con sus propios tests dedicados en la
fase donde corresponda (backup, Fase 7, o antes si el rendimiento lo exige)
en vez de introducirlo de paso en una fase enfocada en integridad relacional.

> **Revisión de la Fase 1.8 (31 de agosto de 2026) — sigue diferido, sin activar.**
> Se pidió explícitamente investigar y evaluar esta decisión, sin activarla. Hallazgos:
>
> - SQLCipher documenta soporte completo para modo WAL: las páginas del archivo `-wal` se cifran
>   con el mismo esquema por página (HMAC + AES) que el archivo principal, no hay una ruta donde
>   queden en texto plano. Esto es comportamiento documentado de la librería, **no verificado con
>   un test propio de este proyecto todavía** — la diferencia importa: es una base razonable para
>   decidir, no una garantía ya demostrada en este código.
> - El motivo real para diferirlo sigue siendo de **backup**, no de cifrado: en modo WAL, una copia
>   consistente del archivo principal sin incluir (o sin hacer `checkpoint` de) `-wal`/`-shm` puede
>   quedar incompleta. Como el diseño de backup (`docs/ARCHITECTURE.md` sección 9) todavía no
>   existe como código, activar WAL ahora introduciría un modo de fallo de backup que nadie ha
>   diseñado todavía para mitigar.
> - Para una aplicación de escritorio de una sola usuaria con el volumen de datos esperado (un
>   consultorio individual, no una clínica con decenas de profesionales concurrentes), no hay
>   indicio de un problema de rendimiento actual que justifique adelantar el cambio — no hay
>   síntoma que resolver todavía.
>
> **Recomendación: mantenerlo diferido hasta la Fase 7 (backup)**, activarlo ahí junto con sus
> propios tests dedicados (incluyendo uno que verifique en disco que `-wal` queda tan ilegible
> como el archivo principal, replicando el test
> `schema_and_data_are_unreadable_as_plain_sqlite_on_disk` de esta fase) y como parte del mismo
> diseño que decide cómo se hace el `checkpoint` antes de copiar. No se activó en esta fase ni se
> cambió la estrategia — queda exactamente como se dejó en la Fase 1.3, con esta evaluación
> agregada como antecedente para cuando corresponda decidir.

## Tests ejecutados y resultados

`cargo test` en `src-tauri/`: **29/29 en verde** (11 de la Fase 1.2 sin
cambios + 18 nuevos de esta fase). Ninguno usa datos de prueba fuera de su
propio test — no hay ningún seed ni dato de ejemplo incorporado al binario
de la aplicación; cada test crea su propio archivo de vault temporal aislado
(`db::test_support::temp_db_path`) y lo descarta.

| Requisito pedido | Test(s) que lo cubre |
|---|---|
| 1. Crear base nueva y correr todas las migraciones | `fresh_database_is_created_from_migrations_alone_with_all_expected_tables` |
| 2. Verificar que todas las tablas existen | mismo test anterior (compara el listado real de `sqlite_master` contra las 25 tablas esperadas) |
| 3. Verificar foreign keys | `foreign_keys_are_enforced_after_migration`, `deleting_a_patient_with_sessions_is_restricted` |
| 4. Verificar índices y restricciones importantes | `important_indexes_exist`, `only_one_current_session_note_version_is_allowed_per_session` |
| 5. Insertar datos relacionados reales y comprobar relaciones | `realistic_related_data_can_be_inserted_and_queried_across_all_domains` (paciente → cita → sesión → 2 versiones de nota → formulación con nodos/edge → objetivo con indicador/intervención → vínculo sesión-objetivo → evaluación → documento → pago → recurso de biblioteca con tag → recordatorio → configuración, todo verificado con JOINs reales) |
| 6. Las restricciones impiden estados inválidos | `invalid_enum_value_is_rejected_by_check_constraint`, `negative_payment_amount_is_rejected`, `appointment_ending_before_it_starts_is_rejected`, `closed_session_note_without_closed_at_is_rejected`, `formulation_edge_across_different_versions_is_rejected`, `formulation_edge_self_loop_is_rejected`, `document_hash_with_wrong_length_is_rejected` |
| 7. El esquema funciona sobre SQLCipher y no sobre SQLite plano | `schema_and_data_are_unreadable_as_plain_sqlite_on_disk` (inspección de bytes en disco + rechazo de clave incorrecta con el esquema completo ya cargado) |
| 8. Una migración futura no destruye datos existentes | `running_migrations_twice_on_the_same_database_is_a_safe_no_op`, `reopening_and_remigrating_an_existing_vault_preserves_data` (con el esquema real), `applying_a_new_migration_preserves_existing_data` (con un esquema sintético de 2 versiones, para probar el mecanismo de actualización incremental en sí, ya que el esquema real hoy solo tiene V1) |

Además, después de estos cambios volví a validar que el resto de la
aplicación sigue intacta: `npm run build` (typecheck + build de producción),
`cargo check`/`cargo clippy --all-targets` sin advertencias, y la app
arrancó igual bajo Xvfb con captura de pantalla.

## Decisiones que necesitan tu aprobación

1. **Actualización del toolchain de Rust** (1.94.1 → 1.98.0) para poder usar
   `rusqlite_migration` 2.6.0 en vez de la 1.3.1 (incompatible con nuestro
   SQLCipher). ¿De acuerdo con mantenerlo así, o prefieres que investigue
   fijar `rusqlite_migration` a una versión intermedia compatible con Rust
   1.94 si existiera una (no encontré ninguna en el rango 1.4.x–1.9.x; los
   saltos de esa librería no dejan una versión así)?
2. **`CHECK (amount >= 0)` en pagos**: asume que no vas a registrar
   devoluciones/reembolsos como montos negativos. ¿Correcto para tu forma de
   trabajar, o debería modelarse distinto más adelante?
3. **No restringí `total_score`** en evaluaciones a valores no negativos,
   por la posibilidad de instrumentos con puntajes estandarizados negativos.
   ¿Confirmas que no quieres ninguna restricción ahí, o hay un rango que sí
   quieras validar para tus instrumentos habituales?
4. **Modo WAL diferido**: no lo activé todavía (ver sección arriba). ¿De
   acuerdo con dejarlo para cuando se implemente backup (Fase 7) o antes si
   hiciera falta por rendimiento?
