# Seguridad: Argon2id + cifrado por sobres + sesión (Fase 1.4)

Documento técnico de la Fase 1.4. Complementa `docs/ARCHITECTURE.md` (sección
5, diseño original) y `docs/sqlcipher.md` (Fase 1.2, cómo se aplica el DEK a
SQLCipher). Aquí se documenta cómo se deriva y protege ese DEK.

## Resumen del flujo implementado

**Creación** (`security::vault_manager::PendingVaultCreation`):

```
generar DEK (256 bits) ──┐
                          ├─→ envolver con KEK(contraseña)  ──┐
generar código           │                                    ├─→ vault.meta.json
de recuperación ─────────┴─→ envolver con KEK(código)     ────┘
                                                            │
                                    (solo tras confirmación)│
                                                            ▼
                                          crear vault.db + migraciones
```

Deliberadamente en dos pasos (`begin_creation` → `confirm_creation`, expuestos
como comandos separados `begin_vault_creation`/`confirm_vault_creation`): el
DEK, el código de recuperación y ambos envoltorios se generan **en memoria**
en `begin_creation`, y el código se muestra a la usuaria. **No se escribe
nada en disco todavía.** Solo cuando confirma explícitamente haberlo guardado
(`confirm_creation`) se escribe `vault.meta.json`, se crea `vault.db`, y se
corren las migraciones. Si la usuaria cierra la aplicación entre medio, no
queda ningún vault a medio crear — la próxima vez que abra la app, es como si
nunca hubiera empezado.

**Desbloqueo:**

```
contraseña → Argon2id(contraseña, salt_guardada, params_guardados) → KEK
    → AES-256-GCM.desenvolver(DEK_envuelto, KEK) → DEK
    → db::open_vault(vault.db, DEK)  [Fase 1.2: verifica SQLCipher real + clave correcta]
```

**Cambio de contraseña** (`vault_manager::change_password`) y **recuperación**
(`vault_manager::recover_access`) siguen el mismo patrón: desenvolver el DEK
existente (probando la contraseña actual o el código de recuperación),
derivar una KEK nueva a partir del nuevo secreto, volver a envolver **el
mismo DEK**, y sobrescribir solo la sección correspondiente de
`vault.meta.json`. **La base SQLCipher nunca se vuelve a tocar ni a
re-cifrar** — es una operación de metadatos, no de datos.

## Qué se implementó, exactamente

| Pieza | Dónde | Biblioteca |
|---|---|---|
| DEK aleatorio de 256 bits | `security::envelope::generate_dek` | `getrandom` 0.4.3 |
| Derivación de KEK (Argon2id) | `security::kdf::derive_kek` | `argon2` 0.6.0 |
| Envoltura/desenvoltura del DEK | `security::envelope::{wrap_dek, unwrap_dek}` | `aes-gcm` 0.11.1 (AES-256-GCM) |
| Código de recuperación | `security::recovery_code` | `getrandom` (bytes) + codificación Base32 de Crockford propia (no es criptografía, ver más abajo) |
| Formato en disco | `security::vault_meta::VaultMetaFile` | `serde_json` + `base64ct` 1.8.3 |
| Orquestación (crear/desbloquear/cambiar/recuperar) | `security::vault_manager` | — |
| Sesión (bloqueo real, bloqueo automático) | `security::session::VaultSession` | — |
| Zeroización de material sensible | `db::VaultKey`, `security::kdf::Kek`, `security::recovery_code::RecoveryCode` | `zeroize` 1.9.0 |

### Parámetros de Argon2id

RFC 9106 §4, "second recommended option" (pensado para entornos con memoria
limitada; en un equipo de escritorio da margen de sobra):

- `m_cost` = 65536 KiB (64 MiB)
- `t_cost` = 3 iteraciones
- `p_cost` = 4 hilos

Se guardan junto a cada envoltorio en `vault.meta.json` (no son secretos),
para poder ajustarlos en el futuro sin romper vaults existentes — cada
desbloqueo usa los parámetros guardados, no una constante fija en el código.

Medido en este entorno (contenedor Linux sin GPU dedicada): cada derivación
de Argon2id toma aproximadamente 0.5–1 segundo. Como cambiar la contraseña y
recuperar acceso hacen **dos** derivaciones (desenvolver con la KEK vieja +
envolver con la KEK nueva), esas operaciones se sienten un poco más lentas
(1–2 segundos) — intencional y aceptable dada la frecuencia con la que
ocurren.

### `vault.meta.json`

```json
{
  "format_version": 1,
  "created_at": "2026-08-30T22:41:25.615Z",
  "password_wrap": {
    "salt_b64": "...",
    "kdf": { "m_cost_kib": 65536, "t_cost": 3, "p_cost": 4 },
    "nonce_b64": "...",
    "ciphertext_b64": "..."
  },
  "recovery_wrap": { "...": "mismo formato" }
}
```

Nada de este archivo es secreto por sí solo: las sales y nonces no necesitan
protección, y el `ciphertext_b64` es inútil sin la KEK correcta. Verificado
con un test dedicado
(`vault_meta_file_never_contains_the_password_or_recovery_code_in_plain_text`)
que confirma que ni la contraseña ni el código de recuperación usados en la
prueba aparecen como substring en el archivo.

Escritura atómica (`WrapRecord::save`): se escribe a un archivo temporal y se
renombra sobre el definitivo, para que un corte de energía a mitad de un
cambio de contraseña no deje el archivo corrupto a medias.

### Código de recuperación

- 15 bytes (120 bits) de `getrandom`, codificados en **Base32 de Crockford**
  (alfabeto de 32 símbolos que excluye I/L/O/U para reducir errores de
  transcripción) y agrupados como `XXXX-XXXX-XXXX-XXXX-XXXX-XXXX`.
- El codificador/decodificador Base32 está escrito a mano
  (`security::recovery_code::{encode, decode}`) porque **no es un componente
  criptográfico** — es una representación de texto para 120 bits ya
  generados por el CSPRNG del sistema operativo, exactamente igual de
  "hecho a mano" que si hubiéramos usado hexadecimal. La regla de "no
  criptografía propia" aplica al KDF y al cifrado (Argon2id, AES-GCM), que
  sí vienen de bibliotecas consolidadas (RustCrypto) sin excepción.
- El decodificador normaliza mayúsculas/minúsculas, ignora espacios/guiones,
  y mapea O→0 e I/L→1 (igual que la codificación nunca produce esos
  caracteres, la normalización es inequívoca).
- **El código nunca se guarda.** Ni en texto plano, ni cifrado como copia
  separada, ni como hash de verificación. Su única función es ser la entrada
  a Argon2id para derivar `KEK_recuperación`; la propia autenticación de
  AES-GCM al desenvolver el DEK es la verificación de que el código es
  correcto — no existe en ningún lado un segundo mecanismo de verificación
  separado que pudiera filtrarse independientemente.
- Al recuperar acceso, el envoltorio de recuperación (`recovery_wrap`) **no
  se toca**: solo se reemplaza `password_wrap`. Por eso el mismo código de
  recuperación sigue funcionando después de un cambio de contraseña o de
  una recuperación previa (verificado en
  `recovery_code_keeps_working_after_a_password_change`, y en la prueba
  manual de UI descrita más abajo).

### Política de contraseña (`security::password_policy`)

Bloqueo real (no solo visual): mínimo 12 caracteres + al menos 2 tipos de
carácter distintos (minúsculas, mayúsculas, dígitos, símbolos). Se aplica en
Rust en creación, cambio de contraseña, y recuperación — el frontend replica
la misma regla con Zod solo para dar retroalimentación inmediata, pero Rust
es quien realmente decide.

El medidor de fortaleza (`evaluate`) es una heurística de longitud +
diversidad de caracteres, deliberadamente más simple que un detector de
patrones estilo zxcvbn. **Decisión explícita:** no se agregó la dependencia
`zxcvbn` (crate con tablas de frecuencia de contraseñas comunes) en esta
fase, para mantener el número de dependencias bajo en una funcionalidad que
ya tiene una validación real subyacente (Argon2id hace que incluso una
contraseña de fortaleza media sea costosa de atacar offline). Si en el futuro
se quiere una estimación más precisa, es un cambio aislado a este módulo.

### Sesión y bloqueo (`security::session::VaultSession`)

Estados: `NoVault` → (`PendingCreation` →) `Unlocked` ⇄ `Locked`.

- **Bloqueo manual**: comando `lock_vault` → se reemplaza el estado por
  `Locked`, lo que suelta la `Connection` (se cierra) y el `VaultKey` del DEK
  (se zeroiza vía su `Drop`) en el mismo movimiento.
- **Bloqueo automático por inactividad**: cada conexión desbloqueada lleva un
  `AutoLockTracker` con la marca de tiempo de la última actividad. El
  frontend llama a `record_vault_activity` (con throttle de 5 segundos) en
  cada movimiento de mouse/tecla mientras el vault está desbloqueado. Una
  tarea en segundo plano (`tauri::async_runtime::spawn`, iniciada en el
  `setup` de la app) llama a `tick_auto_lock()` cada 10 segundos; si pasó el
  período configurado (15 minutos por defecto) sin actividad, bloquea sola.
  Configurable en caliente vía el comando `set_auto_lock_timeout_seconds`
  (todavía sin pantalla de ajustes que lo exponga — eso es de una fase
  posterior, pero el mecanismo ya es real).
- **Lo que NO se implementó, delimitado explícitamente**: reaccionar a que
  el sistema operativo se suspenda o se bloquee la pantalla. Eso requiere
  integración nativa por plataforma (`NSWorkspace` en macOS, mensajes de
  sesión de Windows, señales de `login1` por D-Bus en Linux) que no es
  razonable implementar en esta fase. El bloqueo automático de hoy cubre
  únicamente inactividad medida por tiempo transcurrido.
- **Imposibilidad estructural de leer datos bloqueada**: `VaultSession`
  nunca expone la `Connection` directamente. El único método que la entrega
  es `with_connection(f)`, que devuelve `Err(VaultLockedError)` si el estado
  no es `Unlocked` — no hay ningún otro camino en el código para llegar a la
  conexión. (Sin comando Tauri todavía: llega con el primer repositorio de
  datos clínicos en la Fase 1.5; por ahora lo prueban los tests de
  `session.rs`.)

## Mensajes de error: qué se distingue y por qué

| Situación | Error | ¿Por qué es seguro revelarlo? |
|---|---|---|
| No existe `vault.db`/`vault.meta.json` | `NoVault` | No requiere ningún secreto para observarse |
| `vault.meta.json` no es JSON válido / esquema desconocido | `MetaFileUnreadable` | Ocurre *antes* de intentar ninguna clave — no compara nada secreto |
| Contraseña incorrecta, o el registro cifrado de la contraseña está dañado | `IncorrectPassword` | Indistinguibles por diseño (autenticación AES-GCM) — mismo principio ya aceptado para SQLCipher en la Fase 1.2 |
| Código de recuperación incorrecto, o su registro está dañado | `IncorrectRecoveryCode` | Mismo principio |
| Contraseña demostrablemente correcta, pero `vault.db` no abre | `CorruptDatabase` | Solo se revela **después** de probar criptográficamente que la contraseña era correcta — no ayuda a nadie que no la tenga |

Ninguno de estos mensajes revela más información de la que ya se puede
inferir sin la contraseña, salvo `CorruptDatabase`, que exige haber probado
la contraseña correcta primero.

## Tests ejecutados (los 16 pedidos)

Todos en `src-tauri/src/security/` (`cargo test`), más una verificación
manual de extremo a extremo sobre la aplicación real (ver más abajo).

| # | Requisito | Test(s) |
|---|---|---|
| 1 | Crear vault con contraseña | `vault_manager::finalize_creates_a_working_vault` |
| 2 | Vault creado está realmente cifrado | `vault_manager::created_vault_is_actually_encrypted_on_disk` |
| 3 | Desbloquear con contraseña correcta | `vault_manager::unlock_with_correct_password_succeeds`, `session::unlock_and_lock_roundtrip` |
| 4 | Rechazar contraseña incorrecta | `vault_manager::unlock_with_wrong_password_is_rejected` |
| 5 | Recuperar acceso con código de recuperación | `vault_manager::recovery_code_unlocks_the_vault` |
| 6 | Cambiar contraseña | `vault_manager::change_password_then_old_password_stops_working_and_new_one_works` |
| 7 | La contraseña antigua deja de funcionar | mismo test anterior |
| 8 | La nueva contraseña funciona | mismo test anterior |
| 9 | El código de recuperación sigue funcionando tras el cambio | `vault_manager::recovery_code_keeps_working_after_a_password_change` |
| 10 | El DEK no cambia al cambiar contraseña | `vault_manager::dek_is_unchanged_by_a_password_change` |
| 11 | No se almacena la contraseña maestra | `vault_manager::vault_meta_file_never_contains_the_password_or_recovery_code_in_plain_text` |
| 12 | Zeroización del material sensible | `kdf::debug_never_prints_the_kek_bytes`, `db::connection::debug_never_prints_the_key_bytes`, `recovery_code::debug_never_prints_the_code` — ver limitación abajo |
| 13 | Bloquear y volver a desbloquear | `session::unlock_and_lock_roundtrip` |
| 14 | No se puede acceder a datos estando bloqueado | `session::cannot_access_the_connection_while_locked` (contraprueba: `can_access_the_connection_and_read_real_data_while_unlocked`) |
| 15 | Vault corrupto rechazado correctamente | `vault_manager::corrupt_database_file_is_reported_distinctly_from_wrong_password`, `corrupt_meta_file_is_reported_distinctly` |
| 16 | No aparecen secretos en logs | `vault_manager::no_secret_material_appears_in_log_output` |

Además, `session.rs` agrega pruebas del bloqueo automático
(`auto_lock_locks_after_the_configured_timeout_of_inactivity`,
`recording_activity_resets_the_auto_lock_timer`) y del flujo de creación en
dos pasos (`cancelling_creation_leaves_no_vault_on_disk`,
`full_creation_flow_ends_unlocked`) que no estaban en la lista explícita pero
verifican comportamiento nuevo de esta fase.

**Total: 87/87 tests en verde** (11 de la Fase 1.2 + 18 de la Fase 1.3 sin
cambios, + 58 nuevos de esta fase).

### Verificación manual de extremo a extremo (aplicación real)

Además de los tests automatizados, se ejecutó la aplicación real (compilada
con `tauri build --debug`, no solo `cargo test`) bajo un display virtual
(Xvfb), y se manejó con clics y tecleo reales (`xdotool`) a través de todo el
flujo, con capturas de pantalla en cada paso:

1. Crear vault con contraseña → medidor de fortaleza responde en vivo
   ("Fuerte") → código de recuperación generado y mostrado.
2. Casilla de confirmación bloquea el botón "Continuar" hasta marcarla.
3. Vault creado: `vault.db` y `vault.meta.json` aparecen en disco;
   `vault.db` no empieza con el encabezado de SQLite en claro.
4. Bloquear → pantalla de desbloqueo.
5. Contraseña incorrecta → mensaje "contraseña incorrecta" real.
6. Contraseña correcta → desbloquea.
7. Cambiar contraseña (contraseña actual + nueva + confirmación) → mensaje
   de éxito.
8. Bloquear → la contraseña **antigua** es rechazada.
9. La contraseña **nueva** desbloquea correctamente.
10. Bloquear → "¿Olvidaste tu contraseña?" → Recuperar acceso con el código
    de recuperación generado en el paso 1 (a pesar de que la contraseña ya
    había cambiado una vez) + una tercera contraseña nueva → desbloquea
    correctamente.

Esto confirma en la aplicación real, no solo en tests aislados, que el
código de recuperación generado al crear el vault sigue siendo válido
después de un cambio de contraseña, y que el DEK subyacente nunca cambió
(los bytes cifrados de la página 1 de `vault.db` fueron idénticos antes y
después de ambas operaciones, porque ni el DEK ni el contenido de esa página
cambiaron).

### Limitación honesta sobre "verificar zeroización" (punto 12)

No existe una forma confiable en Rust seguro de inspeccionar la memoria de
un proceso después de que un valor se soltó para comprobar que
efectivamente quedó en ceros — cualquier intento (leer memoria cruda vía
punteros) sería `unsafe`, frágil, y dependiente de decisiones del
optimizador que no deberíamos estar probando. En su lugar, la verificación
que sí es razonable y se implementó es:

1. Confirmar que el tipo llama a `zeroize()` en su `Drop` (revisión de
   código — `VaultKey`, `Kek`, `RecoveryCode` todos lo hacen, usando el
   crate `zeroize`, no una implementación propia).
2. Confirmar que **nunca se puede imprimir el secreto por accidente**
   (`Debug` redactado) — probado explícitamente para los tres tipos.
3. Confirmar que el secreto no persiste en ningún artefacto observable
   (archivo de metadatos, logs) — probado explícitamente.

Esto es lo que se puede demostrar de forma determinística; lo que no se
puede demostrar con una prueba automatizada (que la memoria física quedó en
ceros) se cubre delegando en una biblioteca consolidada para ese propósito
exacto (`zeroize`) en vez de una implementación propia.

## Antes de implementar: ¿algún cambio respecto a `ARCHITECTURE.md`?

Ninguna decisión criptográfica se apartó del diseño aprobado (DEK de 256
bits, Argon2id, AES-256-GCM, código de recuperación de alta entropía, DEK
estable entre cambios de contraseña). Las únicas adiciones son de
implementación, no de diseño, y se detallan arriba: parámetros exactos de
Argon2id (RFC 9106), formato de `vault.meta.json`, la codificación Base32 de
Crockford para el código de recuperación (no es cifrado), y el mecanismo de
bloqueo automático por inactividad (con su límite explícito respecto a
eventos del sistema operativo).
