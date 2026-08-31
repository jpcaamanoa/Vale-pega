# Agenda y Google Calendar (Fase 3)

Documento técnico de la Fase 3. Complementa `docs/ARCHITECTURE.md`. Cubre la Agenda local y la
integración unidireccional (app → Google) con Google Calendar. Referenciado desde
`src-tauri/src/calendar/mod.rs`.

## Alcance

Dentro de esta fase: Agenda local (CRUD de citas, cancelar/archivar/restaurar, advertencia de
solapamiento, citas sin paciente como bloqueos personales), conexión OAuth con Google Calendar,
selección de un calendario **existente**, espejado de citas como eventos, reintento manual, una
pantalla `/settings`, y el bloque "Hoy" del Dashboard con datos reales.

Fuera de alcance (deliberadamente, ver `docs/ARCHITECTURE.md`): Sesiones, `session_notes` como UI,
Pagos, Documentos, Formulación, Objetivos, Evaluaciones, Biblioteca, Herramientas, Recordatorios,
IA, sincronización multi-dispositivo, Backup, Export, iOS/iPadOS.

## Modelo de datos

`appointments` ya existía desde la Fase 1.3 con todo lo necesario — **no hubo migración nueva**.
Columnas relevantes: `patient_id` (nullable, `ON DELETE SET NULL`), `starts_at`/`ends_at`
(`CHECK (ends_at > starts_at)`), `status` (`programada`/`cancelada` son las únicas que este
servicio escribe — el `CHECK` de la base permite además `confirmada`/`completada`/`no_asistio`,
reservadas para una fase de Sesiones que no existe todavía), `modality`, `google_event_id`/
`google_calendar_id`/`last_synced_at` (los únicos escritos por `repositories::appointments::
set_google_link`, llamado exclusivamente desde `calendar::sync`), `deleted_at` (soft delete).

`title` es un campo interno de conveniencia (`"Sesión clínica"` o `"Bloqueo personal"` según haya
o no paciente) — nunca se expone por IPC ni se envía a Google. Las lecturas devuelven en su lugar
`patient_name` vía `LEFT JOIN`, así el nombre mostrado nunca queda desactualizado.

## Arquitectura del módulo `calendar`

```
calendar/
  oauth.rs    PKCE + state, listener de loopback, construcción de la URL de autorización
  client.rs   Llamadas HTTP a Google (tokens, calendarList, eventos) — sin acceso a datos clínicos
  tokens.rs   Refresh token en el keychain del SO (crate `keyring`)
  sync.rs     Orquestación local ↔ Google (prepare → reconcile → apply)
```

Ningún otro módulo construye una llamada HTTP a Google directamente.

### El problema síncrono/asíncrono, y cómo se resolvió

`security::VaultSession::with_connection` es deliberadamente síncrono: acepta un closure
`FnOnce(&Connection) -> T` y mantiene un `std::sync::Mutex` tomado durante exactamente ese
closure. No puede envolver un `.await`, y aunque pudiera, mantener ese mutex durante una llamada
de red bloquearía cualquier otra operación del vault mientras se espera la respuesta de Google.

Por eso `calendar::sync` separa la reconciliación en tres pasos, y ninguna función que hace
`.await` recibe jamás una `&Connection`:

1. **`prepare_reconcile(conn, appointment_id)`** — síncrono. Lee credenciales, calendario
   seleccionado y una foto de la cita. Devuelve `Err(SyncOutcome)` cuando ya se puede resolver sin
   tocar la red (nada configurado, o la cita no existe).
2. **`reconcile(input)`** — asíncrono, sin `&Connection`. Con esa foto ya en memoria, habla con
   Google (renovar token, crear/actualizar/borrar el evento).
3. **`apply_reconcile_effect(conn, appointment_id, effect)`** — síncrono. Persiste lo que haya que
   persistir (normalmente, el `google_event_id` nuevo).

`commands::appointments::sync_after_mutation` orquesta los tres pasos: un `with_connection` corto,
el `.await` intermedio, y un segundo `with_connection` corto — nunca ambos a la vez.

## OAuth 2.0 Authorization Code + PKCE

Cliente Google Cloud Console de tipo **"Aplicación de escritorio"** (público/instalado, RFC 8252).
Google exige igualmente el parámetro `client_secret` en el intercambio de código para este tipo de
cliente, aunque RFC 8252 lo clasifique como público — por eso el Client Secret se trata como
configuración de la app (vive en `app_settings`, dentro del vault cifrado), **no** como un secreto
equivalente a un refresh token, y no se le construyó ninguna arquitectura de protección adicional
más allá de "vive cifrado en reposo junto con el resto de `app_settings`".

Flujo completo (`commands::calendar::begin_google_auth`):

1. `oauth::generate_verifier()` / `oauth::generate_state()` — 32 bytes aleatorios cada uno
   (`getrandom`), codificados en base64url sin padding. Viven solo en memoria durante el intento.
2. `oauth::code_challenge(verifier)` — SHA-256 (crate `sha2`) + base64url sin padding, verificado
   contra el vector de prueba del Apéndice B de RFC 7636.
3. `oauth::bind_loopback_listener()` — un `TcpListener` en `127.0.0.1` (nunca `0.0.0.0` ni un
   `localhost` ambiguo) en un puerto efímero.
4. `oauth::build_auth_url(...)` construye la URL de consentimiento con `access_type=offline` y
   `prompt=consent` (para garantizar que Google entregue un `refresh_token`) y los scopes finales.
5. `open::that(url)` abre el navegador predeterminado del sistema.
6. `oauth::wait_for_redirect(listener, expected_state, timeout)` — poll no bloqueante con
   deadline (5 minutos), valida que el `state` recibido coincida antes de devolver el código.
7. `client::exchange_code(...)` intercambia código + `code_verifier` por tokens.
8. El `refresh_token` se guarda exclusivamente en el keychain del SO. El `access_token` **no** se
   cachea — se pide uno nuevo antes de cada llamada (costo bajo para el volumen de uso esperado:
   una psicóloga, sincronización ocasional de citas).

### Scopes finales

- `https://www.googleapis.com/auth/calendar.calendarlist.readonly` — listar los calendarios
  existentes de la cuenta, para que la usaria elija uno.
- `https://www.googleapis.com/auth/calendar.events` — crear/actualizar/borrar eventos.

Verificados directamente contra el discovery document oficial de la API
(`https://www.googleapis.com/discovery/v1/apis/calendar/v3/rest`): `calendarList.list` exige uno
de `calendar`/`calendar.calendarlist`/`calendar.calendarlist.readonly`/`calendar.readonly` — nunca
alcanza con `calendar.events` solo. Se descartó `calendar.app.created` porque restringe el acceso
a calendarios creados por la propia app, incompatible con la decisión de usar siempre un
calendario ya existente.

## Almacenamiento de credenciales — dónde vive cada cosa

| Dato | Dónde | Por qué |
|---|---|---|
| `refresh_token` | Keychain del SO (`keyring`) | Es la credencial de larga vida — nunca en SQLite ni en un archivo plano |
| `access_token` | No se guarda | Vida corta (~1h); se pide uno nuevo cada vez |
| Client ID / Client Secret | `app_settings` (dentro del vault SQLCipher) | Configuración de la app, no un secreto de usuaria — pero igual queda cifrado en reposo |
| `code_verifier` / `state` | Memoria, solo durante el intento de conexión | Nunca se persisten |
| Calendario seleccionado (`google_calendar_id`) | `app_settings` | Configuración, no dato clínico |

## Minimización — qué llega a Google y qué nunca llega

`calendar::client::event_payload(starts_at, ends_at)` es el único punto donde se construye el
cuerpo JSON de un evento. Recibe **exclusivamente** dos horarios — nunca un `Appointment`
completo — así es estructuralmente imposible que un campo clínico se cuele aunque `Appointment`
gane campos nuevos en el futuro. El texto del evento es una constante fija dentro del módulo
(`"Sesión clínica"`), no un parámetro:

```json
{ "summary": "Sesión clínica", "start": { "dateTime": "…" }, "end": { "dateTime": "…" } }
```

Nunca se envía: nombre ni iniciales del paciente, RUT, diagnóstico, motivo de consulta,
modalidad, notas, evaluaciones, formulación, ni ningún documento. Cubierto por
`calendar::client::tests::event_payload_never_contains_anything_beyond_the_generic_summary_and_the_two_timestamps`
y `event_payload_is_identical_regardless_of_what_the_timestamps_look_like`.

La advertencia de solapamiento (`services::appointments::OverlapWarning`) tampoco revela el
paciente de la cita en conflicto — solo horario y si tiene o no paciente asociado — cubierto por
`overlap_warning_never_includes_a_patient_name`.

## Contrato de comportamiento local ↔ Google

La cita local **siempre** se guarda primero y de forma independiente. La sincronización con
Google es siempre best-effort y nunca puede deshacer, revertir ni alterar el estado local:

| Operación local | Efecto en Google |
|---|---|
| Crear cita | Best-effort: crea el evento espejo. Si falla, la cita queda creada igual, sin vínculo — reintentable |
| Editar cita | Best-effort: actualiza el evento existente, o crea uno si nunca se sincronizó |
| Cancelar cita | Best-effort: borra el evento espejo si existía. La cita local sigue como registro histórico |
| Archivar cita | Igual que cancelar: borra el evento espejo si existía |
| Restaurar cita | Vuelve a evaluar si debería tener evento (según su `status`) y sincroniza en consecuencia |
| Evento borrado directamente en Google | Se detecta (404/410) al intentar actualizarlo — se limpia `google_event_id` localmente, la cita **no se toca** |
| Token revocado/expirado | Se limpia del keychain, se reporta `Disconnected` — ninguna cita local cambia |
| Reintento manual | Repite exactamente la misma reconciliación de tres pasos |

Google nunca tiene autoridad para crear, modificar ni eliminar una cita local. Como mucho, una
respuesta 404/410 al actualizar un evento limpia el vínculo técnico (`google_event_id`) — la fila
de la cita, su `status` y su `deleted_at` no se tocan nunca desde `calendar::sync`.

## Desconexión

`commands::calendar::disconnect_google_calendar`: revoca el token contra Google (best-effort — si
falla, igual se limpia localmente), borra el `refresh_token` del keychain y el calendario
seleccionado. El Client ID/Client Secret **no** se borran, para no obligar a reconfigurarlos si la
usuaria vuelve a conectar más adelante.

## Limitaciones conocidas de este entorno de desarrollo

Este documento distingue explícitamente lo que se verificó de lo que no pudo verificarse en el
entorno de desarrollo (contenedor Linux headless, sin acceso interactivo a un navegador real con
sesión de Google):

- **Backend de keychain**: no hay daemon de Secret Service/D-Bus en este entorno — confirmado por
  inspección directa (sin proceso `dbus-daemon`/`gnome-keyring`, sin socket, sin `secret-tool`).
  El código de `calendar::tokens` está escrito y probado en cuanto a su lógica (redacción en
  `Debug`, manejo de `NoEntry`), pero un guardado/lectura real contra un keychain del SO **no se
  pudo ejercitar** aquí. En macOS/Windows con el keychain nativo disponible, `keyring` usa el
  backend correspondiente sin cambios de código.
- **Flujo OAuth completo contra Google real**: `oauth.rs` y `client.rs` están probados
  unitariamente (PKCE contra el vector RFC 7636, listener real de loopback, parsing de la
  respuesta HTTP) y verificados manualmente hasta el punto de guardar credenciales de prueba y
  habilitar el botón "Conectar con Google" — pero completar un login real contra
  `accounts.google.com` requiere credenciales reales de un cliente OAuth y un navegador
  interactivo, ninguno disponible en este entorno. No se afirma que el intercambio de código
  contra Google real, la renovación de tokens, ni la revocación se hayan probado end-to-end.
- **Multiplataforma**: todo lo anterior se desarrolló y probó en Linux. El comportamiento en
  macOS/Windows del backend de `keyring` y del listener de loopback no se verificó en esta fase.
