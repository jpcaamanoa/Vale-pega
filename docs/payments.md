# Pagos / Cobros internos (Fase 7)

Documento técnico de la Fase 7. Complementa `docs/ARCHITECTURE.md` (sección "Pagos" del esquema y
la tabla de fases). Cubre el quinto vertical funcional completo del cuaderno clínico: registrar
cobros internos por sesión/paciente, su ciclo de estados administrativo, y dos puntos de entrada
mínimos (ficha del paciente y sesión clínica).

## Propósito

Antes de esta fase, "Pagos" era una pestaña de la ficha del paciente que mostraba "Próximamente",
y el Dashboard mostraba "Ingresos del mes" como un placeholder fijo sin datos reales. Esta fase
las reemplaza por contenido real: registrar entradas de pago administrativas (no boletas, no
facturación electrónica, no integración con el SII), con estado, monto, método, fechas de
vencimiento/pago y notas administrativas libres.

## Alcance

Dentro de esta fase: `payments` (CRUD completo, archivado/restauración), pestaña "Pagos" de la
ficha del paciente, entrada de creación mínima desde `SessionDetailScreen` ("Registrar pago"),
agregados reales de Dashboard ("Ingresos del mes" e "Pagos pendientes").

Fuera de alcance (deliberadamente, ver aprobación de Fase 7): boletas, facturación electrónica,
integración con el SII, reembolsos como operación propia (se modela como `condonado` cuando
corresponde, nunca como una tabla o estado nuevo), recordatorios de pago, export/backup de pagos,
Modo Privacidad, cualquier envío de información de pagos a Google Calendar o a cualquier servicio
externo.

## Modelo de datos usado

Exactamente el de `SCHEMA_V1` (Fase 1.3) — **sin migraciones nuevas**. Una tabla:

```sql
CREATE TABLE payments (
  id TEXT PRIMARY KEY,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE RESTRICT,
  session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  amount REAL NOT NULL,
  currency TEXT NOT NULL DEFAULT 'CLP',
  method TEXT CHECK (method IN ('efectivo','transferencia','tarjeta','otro')),
  status TEXT NOT NULL CHECK (status IN ('pendiente','pagado','atrasado','condonado')) DEFAULT 'pendiente',
  due_date TEXT,
  paid_at TEXT,
  notes TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  deleted_at TEXT
);
CREATE INDEX idx_payments_status_due ON payments(status, due_date);
```

`amount` es `REAL` en el esquema (sin cambios), pero el servicio exige que represente un entero
en pesos chilenos — ver "Montos en CLP" más abajo. No se agregó ninguna librería monetaria.

## Arquitectura

Mismas capas que Objetivos (Fase 5) y Antecedentes (Fase 6):

```
React (features/payments/*)
   │  invoke('create_payment', { input }), invoke('payment_dashboard_summary'), etc.
   ▼
commands::payments   (src-tauri/src/commands/payments.rs, 8 comandos)
   │  — capa fina. Solo obtiene la conexión y delega. No importa nada de `calendar::*`.
   ▼
security::session::VaultSession::with_connection
   │  — vault bloqueado ⇒ Err antes de llegar a services/repositories.
   ▼
services::payments   (src-tauri/src/services/payments.rs)
   │  — reglas de negocio: validación de monto/estado/método, regla de integridad
   │    sesión↔paciente, bloqueo de creación para paciente archivado.
   ▼
repositories::payments   (src-tauri/src/repositories/payments.rs)
   │  — SQL puro sobre `payments`, incluido el cálculo de "atrasado" y los agregados de Dashboard.
   ▼
SQLite + SQLCipher (vault.db, sin migraciones nuevas)
```

## "Atrasado" es derivado, nunca se escribe automáticamente

Esta es la regla central de la fase, resuelta explícitamente en la aprobación formal.

- El estado almacenado (`status`) permanece en `pendiente` hasta que alguien lo marca
  manualmente como `pagado`/`condonado` (o, de forma explícita y manual, como `atrasado`). El
  flujo normal de la aplicación **nunca** escribe `atrasado` por sí solo.
- El hecho de que un pago pendiente ya venció se calcula en el repositorio, en tiempo de lectura,
  como una columna `is_overdue` puramente derivada:

  ```sql
  (status = 'pendiente' AND due_date IS NOT NULL AND due_date < date('now')) AS is_overdue
  ```

- `is_overdue` nunca se persiste — es una proyección de la fila, recalculada en cada consulta.
  El frontend nunca escribe este valor de vuelta: el formulario de edición está atado al `status`
  crudo (`Pendiente`/`Pagado`/`Atrasado`/`Condonado`), y solo la vista de lectura (encabezado del
  detalle, listado) muestra "Atrasado" cuando `is_overdue` es verdadero, vía
  `effectivePaymentStatusLabel()` — una función puramente de presentación en
  `features/payments/types.ts` que nunca toca el campo `status` real. Esto evita
  estructuralmente el bug de que volver a guardar el formulario de un pago vencido persista
  accidentalmente el literal `"atrasado"` en la base.
- El valor literal `status = 'atrasado'` sigue siendo legal (el `CHECK` de `SCHEMA_V1` no cambió)
  y queda disponible como anulación manual explícita desde el selector "Estado" del formulario —
  verificado con el test
  `a_manually_set_atrasado_payment_is_accepted_and_never_auto_reverted`, que confirma que un pago
  guardado manualmente como `atrasado` no es "corregido" de vuelta a otro estado por ninguna
  lógica del servicio.
- **Limitación conocida y aceptada, documentada explícitamente (no oculta):** el cálculo usa
  `date('now')` de SQLite, que es UTC. Para una usuaria en un huso horario distinto a UTC, el
  límite exacto de "hoy" para propósitos de "atrasado" puede no coincidir exactamente con la
  medianoche de su hora local. Esta es la misma clase de limitación ya aceptada en otras partes
  del proyecto para cálculos de fecha "de hoy"/"del mes actual" en SQL nativo, y no se resuelve
  agregando una librería de fechas (`chrono`/`time`), decisión ya tomada desde la Fase 1.5.

Hay exactamente una fuente de verdad (`status`); todo lo demás es proyección.

## Reglas de monto y condonación

Validadas tanto en Zod (frontend, `paymentFormSchema` con `superRefine`, para feedback inmediato)
como en `services::payments::validate` (backend, autoritativo) — un valor inválido enviado
directamente por IPC, sin pasar por el formulario, se rechaza igual:

| Combinación | Resultado |
|---|---|
| `amount > 0` + cualquier estado | Válido |
| `amount == 0` + `status == 'condonado'` | Válido — permite condonar preservando el monto original en cero cuando así corresponde administrativamente |
| `amount == 0` + cualquier otro estado | **Inválido** — mensaje explícito: "Un monto de 0 solo es válido si el estado es 'Condonado'" |
| `amount < 0` | **Inválido** siempre, sin excepción |

No existe una tabla ni un estado de "reembolso" — un reembolso se registra como `condonado`
cuando así lo decide la profesional, sin inventar semántica nueva no pedida por la aprobación.

## Montos en CLP: enteros, sin decimales

`amount` sigue siendo `REAL` en el esquema (no se tocó `SCHEMA_V1` para esto — sería un cambio de
tipo de columna, fuera de alcance sin aprobación). La regla de que un peso chileno no tiene
decimales se aplica como regla de **servicio** (`validate` rechaza un monto no entero) y se
refuerza en el frontend (`amountField` en `schema.ts` exige un string que represente un entero
no negativo, y el campo del formulario usa `step="1"`). No se agregó ninguna librería monetaria
(`dinero.js` y similares) — un monto en CLP es, en la práctica, un entero de pesos.

## Método de pago: obligatorio solo al marcar "Pagado"

`method` es opcional en `pendiente`/`atrasado`/`condonado`, y **obligatorio** cuando
`status == 'pagado'` — verificado por `rejects_paid_without_a_method` en el servicio. Solo se
aceptan los cuatro valores ya definidos por el `CHECK` de `SCHEMA_V1` (`efectivo`,
`transferencia`, `tarjeta`, `otro`); no se agregó ningún valor nuevo ni se tocó el `CHECK`.

## Relación con sesión: opcional, con integridad verificada en el servicio

`payments.session_id` es una FK opcional (`ON DELETE SET NULL`) hacia `sessions(id)`, sin
relación alguna con `appointments` — la misma decisión que Objetivos (Fase 5) tomó para
`session_goals`, aplicada aquí de forma más simple porque no hay tabla puente: un pago tiene, como
máximo, una sesión asociada.

La regla de integridad no negociable — igual en espíritu a la de Objetivos — es que **el
servicio verifica explícitamente `session.patient_id == payment.patient_id` antes de crear o
modificar cualquier asociación**, tanto en `create_payment` como en `update_payment` cuando
`session_id` cambia. Nunca se confía en el `patientId` que llega desde React sin validarlo contra
la sesión real. Cubierto por
`rejects_creating_a_payment_whose_session_belongs_to_a_different_patient` y
`update_rejects_changing_the_session_to_one_of_another_patient`.

## El punto de entrada "Registrar pago" desde la sesión

`SessionDetailScreen` agrega un botón "Registrar pago" que navega a
`/patients/:patientId/payments/new` pasando `sessionId` por `location.state` (un identificador
opaco, nunca contenido clínico). `PaymentCreateScreen` lee ese estado opcional y, si está
presente, precarga `sessionId` en el formulario y muestra el aviso "Se vinculará a la sesión desde
la que se abrió este formulario" — sin selector de sesión, sin segundo formulario, sin lógica
nueva en `services::sessions`. Es exactamente el mismo formulario, el mismo componente y las
mismas reglas de negocio que crear un pago desde la pestaña "Pagos" del paciente — la única
diferencia es qué valor trae precargado `sessionId`.

## Paciente archivado

- No se pueden crear pagos nuevos para un paciente archivado (`create_payment` revisa
  `patient.deleted_at`, con la autoridad en el backend — la UI de `PaymentsTab` solo oculta el
  botón "Nueva entrada de pago" como refuerzo, nunca como la única barrera).
- Los pagos **existentes** de un paciente archivado siguen siendo consultables y sus datos
  administrativos (monto, método, notas, fechas) siguen siendo corregibles — archivar un paciente
  no oculta ni bloquea la edición de sus pagos ya registrados, mismo criterio que con sesiones y
  objetivos. Verificado con `editing_a_historical_payment_of_an_archived_patient_is_allowed`.
- `update_payment` no vuelve a comprobar el estado archivado del paciente al guardar cambios —
  solo `create_payment` lo hace, porque archivar bloquea la creación de datos nuevos, no la
  corrección de datos existentes.
- Restaurar el paciente reactiva la posibilidad de crear pagos nuevos, sin ninguna acción
  adicional sobre los pagos ya existentes (que nunca se tocaron).

## Archivado y restauración de un pago

Igual patrón que pacientes, citas, sesiones y objetivos (soft delete real, reutilizando
exclusivamente `deleted_at` — nunca se introduce un segundo mecanismo para representar
reembolso/condonación/anulación tributaria, que siguen siendo, respectivamente, el estado
`condonado` o una simple corrección administrativa del registro):

- `archive_payment` fija `deleted_at`; el pago desaparece del listado "Activos" pero sigue
  completo en "Archivados".
- `restore_payment` revierte `deleted_at` a `NULL`.
- El pago archivado sigue siendo editable (mismo criterio que `GoalDetailScreen`/
  `PaymentDetailScreen` para objetivos: el formulario nunca se deshabilita solo por estar
  archivado).

## Dashboard: dos agregados reales, calculados en SQL

`repositories::payments::dashboard_summary()` calcula, en una sola consulta por agregado y
siempre excluyendo pagos archivados (`deleted_at IS NULL`):

- **`paid_this_month_total`**: suma de `amount` donde `status = 'pagado'` y
  `strftime('%Y-%m', paid_at) = strftime('%Y-%m', 'now')` — reemplaza el placeholder fijo de
  "Ingresos del mes" que existía desde la Fase 2.
- **`pending_count`** / **`pending_total`**: conteo y suma de `amount` donde
  `status IN ('pendiente', 'atrasado')` — la nueva fila "Pagos pendientes" del Dashboard, distinta
  de la tarjeta genérica "Pendientes" (que sigue mostrando "Próximamente" para notas sin cerrar y
  tareas clínicas, sin relación con pagos).

Ambos agregados se calculan enteramente en el backend vía `SUM`/`COUNT`/`GROUP BY` — el frontend
nunca descarga la lista completa de pagos para sumarla del lado del cliente, mismo criterio que
`geographic_distribution` en Fase 6.1. `PaymentDashboardSummary` es un DTO puramente numérico: no
lleva nombre de paciente, RUT, ni ningún dato identificable.

**Interpretación explícita de un caso no cubierto literalmente por la aprobación:** los pagos de
un paciente archivado siguen contando en ambos agregados del Dashboard mientras no estén también
archivados como pago — se decidió así porque el Dashboard resume la operación administrativa
completa de la vault, no solo de pacientes activos, y la aprobación no excluyó este caso
explícitamente. Verificado con `dashboard_summary_excludes_archived_payments` (que sí exige
excluir un pago archivado, sin importar el estado del paciente).

"Sesiones del mes" —tarjeta contigua del Dashboard, perteneciente a la vertical Sesiones— no se
tocó en ningún punto de esta fase, tal como exigía la aprobación.

## Privacidad

- **IPC mínimo por construcción.** `PaymentListItem` (lo que devuelven `list_payments`/
  `list_archived_payments`) no lleva `patient_id` (el listado ya está scoped a un paciente por la
  llamada) ni `notes` (detalle administrativo, no necesario para una lista) — mismo criterio que
  `GoalListItem` en Fase 5.
- **Sin nombre/RUT/diagnóstico en ningún DTO de pagos.** `Payment`, `PaymentListItem` y
  `PaymentDashboardSummary` solo llevan campos administrativos (monto, moneda, método, estado,
  fechas, notas libres, `session_id` opaco) — verificado por inspección directa de los tres
  structs en `repositories/payments.rs`.
- **Sin pagos en Google Calendar.** El módulo `calendar` no referencia `payments` en ningún
  punto — verificado por inspección directa del código (`grep` sobre los archivos de
  `calendar/*.rs`: cero coincidencias) — y `commands/payments.rs` lleva un comentario de módulo
  explícito documentando que nunca importa nada de `calendar::*`.
- **`location.state` solo lleva `sessionId`.** La navegación "Registrar pago" desde
  `SessionDetailScreen` pasa exclusivamente un identificador opaco, nunca contenido clínico ni
  administrativo de la sesión.
- **Sin contenido clínico ni de pagos en logs ni en el título de la ventana.** El título de la
  ventana permanece "Cuaderno Clínico" en todo momento; el log propio de la aplicación
  (`~/.local/share/com.jpcaamano.cuadernoclinico/logs/Cuaderno Clínico.log`) solo registra el
  evento genérico de migración de base de datos.
- **Auditoría manual realizada con una cadena marcador ficticia (`XYZFASE7PAGOS`)** sembrada en
  las notas administrativas de un pago real de prueba: no aparece en `WebKitCache`,
  `CacheStorage`, `storage` (incluida la carpeta `origin` del *storage* del WebView), ni en el
  log propio de la aplicación — solo dentro de `vault.db`, que `file` reconoce como datos
  opacos (no como una base SQLite reconocible en claro), confirmando que sigue cifrado con
  SQLCipher.

## Decisiones de negocio tomadas en esta fase

1. **"Atrasado" es derivado, nunca auto-escrito.** Ver sección dedicada arriba — la decisión
   central de la aprobación de Fase 7.
2. **`amount == 0` es válido únicamente junto a `condonado`.** Resuelve de forma explícita la
   pregunta abierta de la fase de planificación.
3. **Sin tabla ni estado de "reembolso".** Un reembolso se modela como `condonado` cuando
   corresponde — no se introduce semántica nueva no pedida.
4. **Montos en CLP como enteros, por regla de servicio, no de esquema.** `amount` sigue siendo
   `REAL` en `SCHEMA_V1`; la restricción de "sin decimales" vive en `services::payments::validate`
   y se refuerza en el frontend, sin tocar el tipo de columna ni agregar una librería monetaria.
5. **Dashboard: ambos agregados ("Ingresos del mes" y "Pagos pendientes") en esta misma fase**,
   no diferidos a una subfase — decisión explícita de la aprobación.
6. **Los pagos de un paciente archivado cuentan en los agregados del Dashboard** mientras el pago
   en sí no esté archivado — interpretación razonada de un caso no cubierto literalmente por la
   aprobación (ver sección "Dashboard" arriba).
7. **"Registrar pago" desde la sesión reutiliza el mismo formulario, sin selector de sesión ni
   segunda implementación de CRUD.** El único dato que aporta la sesión es su propio `id`,
   precargado vía `location.state`.

## Exclusiones explícitas de esta fase

Ninguno de estos puntos se tocó, tal como exigía la aprobación:

- Boletas, facturación electrónica, integración con el SII — no existen en el proyecto, no se
  diseñó ninguna tabla ni campo "por si acaso".
- Reembolsos como entidad u operación propia — se modelan enteramente con el estado `condonado`
  ya existente en `SCHEMA_V1`.
- Recordatorios de pago, export/backup de pagos, Modo Privacidad — sin cambios.
- `docs/SCHEMA_V1.md` y `src-tauri/src/db/migrations.rs` — sin migraciones nuevas.
- `src-tauri/src/security/*`, `src-tauri/src/calendar/*`, `src-tauri/src/db/connection.rs` — sin
  tocar.
- `services/sessions.rs` — sin tocar; la integración con `SessionDetailScreen` es puramente
  aditiva (un botón nuevo que navega a una pantalla ya existente).
- `appointments` — ninguna relación nueva ni existente entre `payments` y `appointments`.
- "Sesiones del mes" del Dashboard — pertenece a la vertical Sesiones, fuera de alcance de esta
  fase, no se tocó.
- Ninguna dependencia nueva — todo el frontend reutiliza `Button`, `TextField`, `Select`,
  `Textarea`, Zod, `react-hook-form`, `react-router-dom`, ya presentes desde fases anteriores; el
  backend reutiliza `strftime`/`date('now')` nativos de SQLite, sin `chrono`/`time`.

## Archivos creados o modificados

| Archivo | Rol |
|---|---|
| `src-tauri/src/repositories/payments.rs` (nuevo) | SQL puro sobre `payments`, incluido el cálculo de `is_overdue` y `dashboard_summary()`. |
| `src-tauri/src/services/payments.rs` (nuevo) | Validación de monto/estado/método, regla de integridad sesión↔paciente, bloqueo de creación para paciente archivado. |
| `src-tauri/src/commands/payments.rs` (nuevo) | 8 comandos Tauri, todos mediados por `VaultSession::with_connection`. |
| `src-tauri/src/repositories/mod.rs`, `services/mod.rs`, `commands/mod.rs`, `lib.rs` | Registro de los nuevos módulos y comandos. |
| `src/features/payments/*` (nuevo) | `types.ts`, `api.ts`, `schema.ts`, `formatCurrency.ts`, `PaymentsTab.tsx`, `PaymentCreateScreen.tsx`, `PaymentDetailScreen.tsx`. |
| `src/features/patients/PatientDetailScreen.tsx` | Pestaña "Pagos" ahora renderiza `PaymentsTab` en vez de "Próximamente". |
| `src/features/sessions/SessionDetailScreen.tsx` | Nuevo botón "Registrar pago" — puramente aditivo, no modifica ningún flujo existente de la nota clínica. |
| `src/features/dashboard/DashboardScreen.tsx` | "Ingresos del mes" pasa de placeholder a valor real; nueva fila "Pagos pendientes". |
| `src/App.tsx` | Rutas `/patients/:patientId/payments/new` y `/patients/:patientId/payments/:paymentId`. |

## Tests ejecutados

`cargo test` en `src-tauri/`: **355/355 en verde** (302 previos sin cambios + 53 nuevos: 14 en
`repositories::payments`, 39 en `services::payments`). `cargo clippy --all-targets`: sin
advertencias. `npm run build`: sin errores. `npm run lint`: 19 warnings (16 preexistentes de
fases anteriores + 3 nuevos en `PaymentDetailScreen.tsx`/`PaymentCreateScreen.tsx`/
`PaymentsTab.tsx`, exactamente las mismas dos categorías ya presentes en el resto del código —
`react(incompatible-library)` por `react-hook-form` y `react(set-state-in-effect)` por el patrón
de carga de datos en `useEffect` ya usado en `GoalsTab.tsx`/`SessionsTab.tsx`/
`ClinicalProfileTab.tsx` — sin categoría nueva). `cargo build`: sin errores.

Tests representativos de las reglas de negocio centrales:

| Requisito | Test |
|---|---|
| "Atrasado" se calcula, nunca se persiste automáticamente | `repositories::payments::a_pending_payment_past_its_due_date_is_flagged_overdue_without_changing_status` |
| Un pago pagado vencido nunca se marca atrasado | `repositories::payments::a_paid_payment_past_its_due_date_is_never_flagged_overdue` |
| `atrasado` manual se acepta y no se revierte solo | `services::payments::a_manually_set_atrasado_payment_is_accepted_and_never_auto_reverted` |
| `amount == 0` solo válido con `condonado` | `services::payments::rejects_zero_amount_when_not_condoned` |
| `amount < 0` siempre inválido | `services::payments::rejects_negative_amount` |
| Método obligatorio solo al marcar pagado | `services::payments::rejects_paid_without_a_method` |
| Sesión de otro paciente rechazada al crear | `services::payments::rejects_creating_a_payment_whose_session_belongs_to_a_different_patient` |
| Sesión de otro paciente rechazada al editar | `services::payments::update_rejects_changing_the_session_to_one_of_another_patient` |
| No se pueden crear pagos para un paciente archivado | `services::payments::rejects_creation_for_an_archived_patient` |
| Pagos históricos de un paciente archivado siguen editables | `services::payments::editing_a_historical_payment_of_an_archived_patient_is_allowed` |
| Dashboard suma correctamente pagado del mes y pendientes | `repositories::payments::dashboard_summary_counts_paid_this_month_and_pending` |
| Dashboard excluye pagos archivados | `repositories::payments::dashboard_summary_excludes_archived_payments` |

## Prueba manual realizada (aplicación real, no solo tests)

Compilada con `cargo build`, ejecutada bajo Xvfb con `xdotool` (clics y tecleo reales) sobre un
vault de prueba desechable (creado y usado en esta sesión — el vault real se guardó aparte antes
de empezar y se restauró exactamente al terminar), con capturas de pantalla en cada paso:

1. Crear vault de prueba → desbloquear → crear paciente ficticio ("Marcela Rios Soto").
2. Pestaña "Pagos" deja de mostrar "Próximamente" — confirmado con el empty state real.
3. Crear pago pendiente sin sesión asociada → guardado correctamente.
4. Crear un segundo pago, marcarlo `pagado` sin método → rechazado con el mensaje explícito de
   método obligatorio → completar método (`efectivo`) → guardado.
5. Dashboard confirmado en vivo: "Ingresos del mes" mostró exactamente el monto del pago pagado
   (con fecha de pago del mes actual); "Pagos pendientes" mostró el monto y conteo del pago
   pendiente restante — coincidencia exacta, sin discrepancias.
6. Crear una sesión clínica ficticia para el mismo paciente (Fase 4, sin regresión).
7. Botón "Registrar pago" desde `SessionDetailScreen` → formulario con aviso de vinculación
   automática a la sesión → guardado → confirmado con "Ver sesión vinculada" en el detalle del
   pago, y navegación de vuelta a la sesión correcta.
8. Crear pago `condonado` con `amount = 0` → aceptado.
9. Intentar crear pago `pendiente` con `amount = 0` → rechazado en el frontend con el mensaje
   "Un monto de 0 solo es válido si el estado es 'Condonado'", sin llegar a IPC.
10. Editar un pago existente: cambiar su fecha de vencimiento a una fecha pasada → confirmado que
    el encabezado del detalle y el listado muestran "Atrasado" mientras el selector "Estado" del
    formulario sigue mostrando "Pendiente" — confirmando que la derivación nunca sobrescribe el
    valor almacenado.
11. Archivar ese pago (con diálogo de confirmación) → desaparece de "Activos" → aparece en
    "Archivados", con "Atrasado" seguido mostrándose correctamente y el formulario editable →
    Restaurar → confirmado de vuelta en "Activos" con todos sus datos intactos.
12. Archivar al paciente completo → confirmado que "Nueva entrada de pago" desaparece de la
    pestaña "Pagos" del paciente archivado, mientras los cuatro pagos históricos siguen
    completamente visibles → Restaurar paciente → botón de creación disponible de nuevo.
13. **Persistencia a través de bloqueo/recuperación del vault**: con los cuatro pagos ya creados,
    bloquear el vault → usar el flujo real de "¿Olvidaste tu contraseña? Recuperar acceso" con el
    código de recuperación real generado al crear el vault (cambio de contraseña genuino, no una
    simulación) → confirmado que los cuatro pagos, sus estados, montos y vínculo con sesión
    siguen exactamente iguales.
14. **Cierre completo del proceso de la aplicación y reapertura real** (no solo bloquear): matar
    el proceso, relanzar el binario → arranca en estado `Locked` → desbloquear con la nueva
    contraseña → Dashboard confirmado con los agregados correctos ("Ingresos del mes: $30.000",
    "Pagos pendientes: $90.000 / 2 pagos", sumando correctamente los estados `pendiente` y
    `atrasado`) → pestaña "Pagos" confirmada con los cuatro pagos intactos.
15. **Auditoría de privacidad**: búsqueda del marcador `XYZFASE7PAGOS` (sembrado en las notas
    administrativas de un quinto pago de prueba) en `WebKitCache`, `CacheStorage`, `storage`
    (incluida la carpeta `origin` del *storage* del WebView) y el log propio de la aplicación —
    cero coincidencias en todos ellos; `vault.db` reconocido por `file` como datos opacos, no
    como una base SQLite en claro.
16. **Regresión funcional de Fases 1–6.1**: Pacientes (activos/archivados), Objetivos,
    Antecedentes, Estadísticas (con el único paciente sin ubicación registrada) y Agenda
    revisados visualmente tras el reinicio completo — sin cambios de comportamiento respecto a
    fases anteriores.
17. Limpieza: proceso de la aplicación de prueba y servidor de desarrollo de Vite detenidos,
    vault de prueba conservado bajo un nombre de respaldo identificado (nunca eliminado, por la
    regla permanente del proyecto), vault real restaurado exactamente como estaba antes de
    empezar.

## Limitaciones y decisiones que quedan pendientes de aprobación

- **Limitación de huso horario en el cálculo de "atrasado" (UTC vs. hora local)** — ya
  documentada explícitamente arriba, aceptada como parte de la aprobación de esta fase, no una
  omisión.
- El resto de las decisiones de esta fase estaban resueltas de forma definitiva en la aprobación
  formal de Fase 7, o son decisiones internas de implementación sin impacto arquitectónico (ver
  "Decisiones de negocio tomadas en esta fase" arriba). Ninguna decisión queda pendiente de
  aprobación adicional.
