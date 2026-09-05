# Backup y Restore seguro (Fase 10)

Documento técnico de la Fase 10. Complementa `docs/ARCHITECTURE.md` (sección 9 y tabla de fases,
sección 17). Resuelve el objetivo central de la fase: que si el computador, disco, instalación o
vault principal se pierde o se daña, exista un respaldo real del que la información pueda
recuperarse de forma segura y verificada — no solo "copiar un archivo".

**Alcance de esta fase: Backup manual + Restore manual, exclusivamente.** Explícitamente fuera de
alcance, ninguno implementado: sincronización entre dispositivos, exportación abierta, Cierre/Alta,
iPhone/iPad, empaquetado/publicación de producción, actualizaciones automáticas, almacenamiento en
la nube, biometría, backups automáticos/programados, rotación de backups, historial de backups más
allá del propio archivo elegido por la usuaria.

## Formato del contenedor: `.cclinbackup`

Un `.cclinbackup` es un archivo ZIP (crate `zip`, `CompressionMethod::Stored` — sin compresión,
deliberado: el contenido de `vault.db` es cifrado y por lo tanto de alta entropía, comprimirlo no
ahorra espacio y solo añade complejidad) con exactamente tres entradas en la versión `1` del
formato:

```
manifest.json       — índice técnico (ver más abajo)
vault.db            — snapshot consistente del archivo SQLCipher completo
vault.meta.json     — el mismo archivo de envelope encryption que ya usa el vault activo
```

`documents/` (una carpeta cifrada de documentos clínicos individuales) está reservada en el diseño
del manifest para una fase futura — **no implementada, no incluida en ningún backup de hoy**. Un
backup `v1` de esta fase nunca la contiene y `restore_backup` nunca exige que exista; un backup
futuro que sí la incluya seguirá pudiendo restaurarse en una instalación de esta fase que la
ignore, siempre que `backup_format_version` no cambie de forma incompatible.

### `manifest.json`: minimización estructural, no por convención

```json
{
  "backup_format_version": 1,
  "backup_id": "4a3fb364-299f-4235-8784-c109dece7d0c",
  "created_at": "2026-09-05T03:18:13.256Z",
  "app_version": "0.1.0",
  "schema_version": 4,
  "vault_meta_format_version": 1,
  "files": [
    { "path": "vault.db", "size_bytes": 397312, "sha256": "..." },
    { "path": "vault.meta.json", "size_bytes": 629, "sha256": "..." }
  ]
}
```

El tipo Rust `BackupManifest` (`src-tauri/src/backup/manifest.rs`) **no tiene ningún campo** capaz
de contener un dato clínico, un nombre de paciente, un RUT, el nombre del equipo, el username del
sistema operativo o la cuenta de Google — la minimización no depende de que nadie escriba ahí algo
que no debería por descuido; el tipo mismo no tiene dónde ponerlo. `manifest_json_has_exactly_the_expected_top_level_keys`
fija ese contrato en un test — cualquier campo nuevo que alguien intente agregar en el futuro
tendría que romper ese test primero.

Tres números de versión, cada uno independiente y con un rol distinto:

- `backup_format_version` — versión de la *estructura del contenedor* (el ZIP + el manifest en
  sí). `restore_backup` la rechaza si no coincide exactamente con la que esta build sabe leer.
- `schema_version` — `PRAGMA user_version` de `vault.db` en el momento del backup, el mismo número
  que usa `rusqlite_migration` internamente. Decide compatibilidad de esquema (ver más abajo).
- `vault_meta_format_version` — versión de `vault.meta.json`, copiada tal cual desde el propio
  archivo; puramente informativa en el manifest.

`app_version` (`CARGO_PKG_VERSION`) es solo para diagnóstico humano ("¿con qué versión de la app
se hizo esto?") — nunca se usa para decidir si un backup es compatible; esa decisión la toma
exclusivamente `schema_version`.

## Snapshot consistente: por qué `VACUUM INTO` y no otra cosa

La regla no negociable de la aprobación (§14): nunca hacer `fs::copy` sobre `vault.db` mientras
existe una conexión viva, sin poder demostrar consistencia transaccional. Se evaluaron tres
opciones reales, no por intuición:

1. **`fs::copy` con la conexión cerrada** — exigiría bloquear la aplicación durante el backup
   (peor UX) y de todas formas requiere que el vault esté desbloqueado en algún momento previo
   para haber podido usarlo — no resuelve nada que `VACUUM INTO` no resuelva mejor.
2. **SQLite Backup API** (`sqlite3_backup_init`/`step`/`finish`) — oficialmente soportada, pero
   expone una API de bajo nivel por FFI que `rusqlite` no envuelve directamente para este caso de
   uso; más superficie de código para el mismo resultado.
3. **`VACUUM INTO '<ruta>'`** — **elegida**. Es SQL estándar, corre dentro de su propia transacción
   de lectura sobre la conexión que la aplicación ya tiene abierta y desbloqueada
   (`VaultSession::with_connection`), nunca ve una escritura a medias, y no depende del modo de
   journal (funciona igual con el rollback journal por defecto de este proyecto que con WAL, si
   algún día se activa).

La verificación es empírica, no solo teórica: el test
`backup_db_entry_opens_with_the_same_key_and_preserves_data` extrae el `vault.db` de un backup
real, lo abre con `security::unlock_vault` usando la **misma contraseña** de forma completamente
independiente del vault original, confirma que los datos están intactos, y además confirma que el
archivo en disco **no** empieza con el encabezado plano `"SQLite format 3"` — el snapshot sigue
siendo tan SQLCipher como el original, nunca "menos cifrado".

`create_backup` exige `VaultSession::with_connection`, que ya exige que el vault esté desbloqueado
— si está bloqueado, la función nunca llega a intentar nada (`BackupError::VaultLocked`), que es
exactamente la regla §17 de la aprobación.

### Respuesta a la pregunta obligatoria del §15: ¿se puede implementar sin activar WAL?

**Sí.** `VACUUM INTO` no depende del modo de journal — funciona idéntico con el rollback journal
por defecto de este proyecto (`docs/db-schema.md`, sección "Decisión explícitamente diferida") que
con WAL. El modo WAL **sigue sin activarse** en esta fase, y ahora por una razón distinta a la
original: ya no es una decisión pendiente por causa de backup (el motivo que `docs/db-schema.md`
daba desde la Fase 1.8), sino una pregunta puramente de rendimiento futuro, sin ningún síntoma
actual que la justifique. Ver la nota de corrección en `docs/db-schema.md`.

## Restore: reemplazo, nunca fusión — staging, validación completa, swap atómico

`restore_backup` (`src-tauri/src/backup/service.rs`) nunca escribe sobre el vault activo
directamente. El flujo completo, en orden:

1. **Extraer** el `.cclinbackup` completo a un directorio de staging desechable
   (`<vault_dir>/../vault-restore-tmp/<uuid>`) — nunca dentro del propio `vault_dir`.
2. **Validar el manifest**: existe, es JSON válido, `backup_format_version` coincide.
3. **Validar presencia y tamaño/hash de cada archivo requerido** (`vault.db`, `vault.meta.json`)
   contra lo que declara el manifest — SHA-256 completo, no solo tamaño.
4. **Validar la credencial** contra el `vault.meta.json` **del staging**, nunca contra el vault
   activo — abre una conexión real al `vault.db` extraído con la contraseña o el código de
   recuperación que la usaria ingresó para *ese respaldo*, reutilizando exactamente
   `security::unlock_vault`/`security::recover_access` (§36: cero lógica de cripto duplicada).
5. **Validar compatibilidad de esquema**: si `PRAGMA user_version` del staging es mayor que lo que
   esta build sabe migrar, se rechaza con un mensaje específico (`SchemaTooNew`) — nunca se intenta
   "bajar de versión" el esquema.
6. **Migrar el staging** a la versión de esquema actual (`db::run_migrations`, la misma cadena de
   migraciones que usa el resto de la aplicación — nunca una copia paralela).
7. **Verificación de integridad**: `PRAGMA foreign_key_check` debe devolver cero filas, y debe
   existir al menos una tabla — de lo contrario, `IntegrityCheckFailed`.
8. **Solo si todo lo anterior pasó** se bloquea la sesión activa (`session.lock()`, que zeroiza el
   DEK igual que un bloqueo manual) y se promueve el staging mediante **dos `rename`**: el vault
   anterior se mueve a `<vault_dir>/../vault-rescue` (nunca se borra todavía), y el staging se mueve
   a la ruta final `vault_dir`.
9. **Revalidación posterior**: se reabre el vault recién promovido con la misma credencial. Si
   falla, el vault promovido se mueve a un directorio `vault-restore-failed-<uuid>` (nunca se
   borra) y el `vault-rescue` se restaura a su lugar — el restore completo se revierte.
10. Solo si la revalidación posterior también pasa se borra `vault-rescue` y se limpia el staging.

Un backup inválido en **cualquier punto** de los pasos 1–7 nunca llega a tocar un solo byte del
vault actual — todo ese trabajo ocurre exclusivamente sobre la copia desechable en staging.

### Instalación nueva (sin vault previo)

Si `vault_dir` no existe todavía (instalación nueva, o vault eliminado manualmete por la usuaria),
`restore_backup` simplemente no encuentra nada que mover a `vault-rescue` y promueve el staging
directo a `vault_dir` — mismo camino de código, sin rama especial (verificado por
`restore_onto_a_fresh_installation_with_no_existing_vault`).

### Recuperación ante crash a mitad de un restore

`run_startup_recovery(vault_dir)` se llama una única vez al arrancar la aplicación, **antes** de
crear `VaultSession` (`lib.rs`, justo antes de `VaultSession::new`). No usa ningún archivo marcador
nuevo — el propio estado de los directorios en disco es la señal:

- Si `vault-rescue` existe y `vault_dir` **no** existe → el crash ocurrió exactamente entre mover
  el vault anterior a rescue y promover el staging. Se restaura: `vault-rescue` vuelve a ser
  `vault_dir`.
- Si `vault-rescue` existe y `vault_dir` **sí** existe → el restore ya se había completado antes
  del crash/cierre; el rescue es basura segura de eliminar.
- Cualquier `vault-restore-tmp`/`backup-scratch` que haya quedado de un intento anterior
  interrumpido (de restore o de backup) se limpia entero al arrancar — nunca es necesario en un
  arranque nuevo.

Tres tests dedicados (`run_startup_recovery_restores_the_previous_vault_if_interrupted_between_rescue_and_promote`,
`run_startup_recovery_cleans_up_an_orphaned_rescue_after_a_completed_restore`,
`run_startup_recovery_does_nothing_in_the_normal_case`) cubren exactamente estos tres casos.

### `StagingGuard`: limpieza automática ante cualquier error o `?` temprano

`struct StagingGuard(PathBuf)` implementa `Drop` para borrar el directorio de staging en cualquier
retorno anticipado (cualquiera de los `?`/`return Err(...)` de los pasos 1–7). Solo se libera con
`std::mem::forget(guard)` en el único punto donde el staging deja de existir en su ruta original
porque un `rename` exitoso ya lo movió a `vault_dir` — nunca antes.

## Compatibilidad de esquema: nunca downgrade

`current_app_schema_version()` calcula, sin depender de ninguna conexión externa, hasta qué
versión de esquema sabe migrar esta build: migra una base SQLite en memoria desde cero con
`db::run_migrations` (la misma función que usa el resto de la aplicación) y lee el
`PRAGMA user_version` resultante. Si el backup declara (en su propio `vault.db`, no solo en el
manifest) una versión mayor, se rechaza con:

> "Este respaldo fue creado con una versión más nueva de Cuaderno Clínico (esquema N, esta
> instalación soporta hasta M). Actualiza la aplicación antes de restaurarlo."

Nunca se intenta "arreglar" un backup demasiado nuevo bajándole la versión — eso destruiría
columnas/tablas que la versión actual no conoce. `restore_rejects_a_backup_from_a_newer_schema_version`
verifica exactamente este camino con un `PRAGMA user_version = 99` real dentro del snapshot.

### Nota honesta sobre cobertura de "backup antiguo genuino"

Esta fase introduce Backup/Restore en la versión de esquema actual (V4). No existe todavía, en la
vida real del proyecto, ningún backup genuinamente creado con un esquema anterior (V1/V2/V3) para
usar como fixture de un test de "migrar un backup viejo en staging". Fabricar uno sintéticamente
habría exigido exportar detalles internos de migración (`SCHEMA_V1`/`V2`/`V3`, la función privada
`migrations()`) fuera de `db::migrations` solo para un test — un costo de superficie productiva que
no se consideró justificado. En su lugar:

- El camino de rechazo "demasiado nuevo" está probado directamente y de forma realista
  (`restore_rejects_a_backup_from_a_newer_schema_version`).
- El propio mecanismo de migración (`db::run_migrations`) tiene su suite extensa y ya existente de
  tests V1→V4 en `db::migrations`, reutilizada sin cambios por `restore_backup` en el paso 6.
- Cuando exista una versión de esquema V5 real en una fase futura, un backup genuino hecho en V4 se
  convertirá automáticamente en el primer caso real de "restaurar un backup de esquema anterior" —
  en ese momento sí existirá un fixture genuino, sin necesidad de fabricar nada.

## Contraseña del backup vs. contraseña actual — nunca se asume la misma

Un `.cclinbackup` fue envuelto (Argon2id + AES-256-GCM, exactamente el mismo mecanismo que
`vault.meta.json` del vault activo, vía `security::unlock_vault`/`recover_access` reutilizados sin
modificación) con la contraseña vigente **en el momento en que se creó ese backup específico**. Si
la contraseña maestra cambió después, el backup sigue exigiendo la contraseña que tenía al
crearse — nunca la contraseña actual del vault activo. La UI lo deja explícito con dos opciones
igualmente válidas al restaurar: "Tengo la contraseña de ese respaldo" o "Uso mi código de
recuperación" (que, igual que en un `recover_access` normal, permite fijar una contraseña nueva
para el vault ya restaurado).

**No se tocó el formato de envelope encryption, el KDF, ni los parámetros de Argon2id** — la
integración es puramente de invocación de las funciones ya existentes de `security::vault_manager`,
reexportadas de forma aditiva (`VaultPaths`, `unlock_vault`, `recover_access`, `UnlockError`,
`RecoveryError`) desde `security::mod.rs` para que `backup::service` pueda validar un vault en
staging sin duplicar lógica de cripto.

## Qué pasa con Google Calendar tras un restore

Un backup **nunca** incluye el `refresh_token`/`access_token` de Google — esos viven exclusivamente
en el keychain/Credential Manager del sistema operativo (`keyring`, desde la Fase 3), nunca dentro
de `vault.db` ni de ningún archivo del contenedor. Tras restaurar un backup —especialmente en un
dispositivo distinto de donde se creó— es esperable que la conexión con Google Calendar deba
rehacerse manualmente; la UI lo advierte explícitamente antes de confirmar la restauración
("Puede ser necesario volver a conectar Google Calendar después de restaurar un respaldo en otro
dispositivo"). `src-tauri/src/calendar/*` no se tocó en esta fase.

## Arquitectura

Mismas capas que el resto de las verticales, con la particularidad de que `backup` no tiene una
capa `repositories` propia — opera sobre archivos y rutas, no sobre filas SQL de una tabla de la
aplicación:

```
React (features/backup/BackupRestoreSection.tsx, en Ajustes)
   │  invoke('create_backup', ...), invoke('inspect_backup', ...), invoke('restore_backup', ...)
   │  + diálogos nativos de archivo (tauri-plugin-dialog): pickDestination()/pickBackupFile()
   ▼
commands::backup   (3 comandos Tauri — capa fina, resuelve vault_dir vía AppHandle)
   ▼
backup::service    (lógica central, puramente sobre &Path y VaultSession — sin Tauri)
   │  create_backup · inspect_backup · restore_backup · run_startup_recovery
   ▼
backup::archive (ZIP + SHA-256) · backup::manifest (BackupManifest)
   ▼
security::{unlock_vault, recover_access, VaultPaths}   (reutilizadas, sin duplicar)
   ▼
SQLite + SQLCipher (vault.db) vía VACUUM INTO / rusqlite
```

`backup::service` es deliberadamente puro sobre rutas de archivo — no sabe nada de Tauri ni de
cómo cada plataforma elige origen/destino — para que el mecanismo central de consistencia y
validación sea el mismo en cualquier plataforma futura (macOS, Windows) sin reescribir lógica.

## UI: "Ajustes → Respaldo y restauración"

Sección nueva en `SettingsScreen.tsx`, después de Google Calendar, sin lenguaje técnico (nunca
"SQLCipher"/"DEK"/"KEK"/"manifest"/"schema" en pantalla — ver `BackupRestoreSection.tsx`):

- **Crear respaldo**: un botón, un diálogo nativo "Guardar como…" (`tauri-plugin-dialog`, filtro
  `.cclinbackup`, nombre por defecto con fecha/hora) y confirmación de éxito. `dest_path` nunca se
  sobrescribe en silencio (`BackupError::DestinationAlreadyExists` si ya existe).
- **Restaurar respaldo**: diálogo nativo "Abrir…", luego un modal de confirmación explícito que
  deja claro que **reemplaza, nunca combina**, con la elección contraseña/código de recuperación,
  antes de pedir cualquier credencial.

Tras un restore exitoso, la sesión pasa a `Locked` (vía `session.lock()` dentro de
`restore_backup`) y la UI lo detecta a través de `VaultSession::refresh_from_disk()` (método nuevo,
puramente aditivo: re-chequea si `vault_dir` existe y actualiza el estado en memoria a
`Locked`/`NoVault` según corresponda; no hace nada si el estado ya es `Unlocked`/`PendingCreation`)
— la usuaria simplemente ve la pantalla de "Desbloquear" y entra con la credencial del backup
restaurado.

## Privacidad

- **`manifest.json` minimizado por construcción** (ver arriba) — verificado con datos reales:
  `manifest_never_contains_patient_data` siembra un nombre de paciente con el marcador
  `XYZFASE10BACKUP` y confirma que el JSON serializado del manifest no lo contiene.
- **`vault.db` sigue genuinamente cifrado dentro del backup** — no es "menos cifrado" que el vault
  activo (verificado leyendo los bytes crudos, sin el encabezado plano de SQLite).
- **Sin logging de contenido clínico**: ninguno de los archivos nuevos (`backup/service.rs`,
  `backup/archive.rs`, `backup/manifest.rs`, `commands/backup.rs`) contiene una sola llamada a
  `log::`/`println!`/`dbg!`/`eprintln!`.
- **Sin tokens de Google en el backup** — ver sección dedicada arriba.
- **Auditoría manual con marcador ficticio (`XYZFASE10BACKUP`)**, sembrado en las notas
  diagnósticas de un proceso terapéutico de un paciente de prueba real, con un backup real creado y
  restaurado a través de la aplicación compilada (no solo de tests): cero coincidencias en
  `manifest.json`, `vault.meta.json`, los nombres de archivo dentro del `.cclinbackup`, los
  directorios de caché/almacenamiento del WebView (`CacheStorage`, `WebKitCache`, `storage`,
  `mediakeys`, `hsts-storage.sqlite`), los logs de la aplicación, el portapapeles, o cualquier
  archivo temporal fuera de la ubicación de staging esperada — el marcador solo aparece dentro de
  los bytes cifrados de `vault.db`, confirmado además que esos bytes no exhiben el encabezado plano
  de SQLite.

## Prueba manual realizada (aplicación real, no solo tests)

Compilada con `cargo build`, ejecutada bajo Xvfb con `xdotool` (clics y tecleo reales) sobre un
vault de prueba desechable (el vault real se guardó aparte antes de empezar y se restauró
exactamente al terminar, archivado bajo
`com.jpcaamano.cuadernoclinico.fase10-manual-test-used`), con capturas de pantalla en cada paso.

- **Backup A**: creado sobre un paciente ficticio ("Camila Fuentes Reyes") con proceso terapéutico
  (antecedentes con el marcador `XYZFASE10BACKUP`), sesión con nota de dos versiones, objetivo,
  tarea entre sesiones, nota de preparación y un pago pendiente — confirmado vía diálogo nativo
  "Guardar como…" real.
- **Modificación posterior**: creado un segundo paciente ("Paciente Post Backup Debe Desaparecer")
  después de Backup A, para verificar que el restore lo descarta.
- **Restore de Backup A**: seleccionado vía diálogo nativo "Abrir…" real, contraseña del backup
  ingresada en el modal de confirmación, restauración completada — la aplicación transicionó
  automáticamente a la pantalla de Desbloquear (confirmando `session.lock()` +
  `refresh_from_disk()` funcionando en la app real, no solo en tests).
- **Verificación de datos exactos tras el restore**: confirmado que solo "Camila Fuentes Reyes"
  existe (activos y archivados) — el paciente creado después de Backup A desapareció. Proceso,
  marcador `XYZFASE10BACKUP`, sesión, tarea, nota de preparación, objetivo y pago — todos
  verificados presentes e idénticos a Backup A, navegando cada pestaña real de la ficha del
  paciente.
- **Persistencia bloqueo → desbloqueo**: confirmado que los mismos datos siguen disponibles tras
  bloquear y volver a desbloquear con la contraseña del backup restaurado.
- **Persistencia a través de un cierre completo del proceso**: `kill` del proceso completo de la
  aplicación, relanzamiento, la pantalla de arranque mostró correctamente "Desbloquear" (vault
  existente, no "Crear vault") — confirmando que `run_startup_recovery` limpió correctamente
  `vault-restore-tmp` sin afectar el `vault_dir` ya promovido — y el desbloqueo posterior mostró
  exactamente los mismos datos (Dashboard: 1 paciente activo, 1 tarea pendiente, $35.000 en pagos
  pendientes).
- **Prueba de corrupción real**: una **copia** de Backup A (nunca el archivo original) corrompida
  modificando 64 bytes crudos del contenedor ZIP. El intento de restauración fue rechazado con un
  mensaje claro en la interfaz ("el archivo de respaldo no se pudo leer"), y se verificó
  explícitamente, con hashes SHA-256 tomados antes y después del intento, que los tres archivos del
  vault activo (`vault.db`, `manifest.json`, `vault.meta.json`) quedaron **exactamente idénticos**
  byte a byte — y que el directorio de staging (`vault-restore-tmp`) no dejó ningún archivo
  extraído sin limpiar tras el fallo.

## Decisiones de negocio tomadas en esta fase

1. **`VACUUM INTO` sobre la conexión ya desbloqueada** — nunca `fs::copy` con conexión viva, nunca
   SQLite Backup API por FFI directa. Verificado empíricamente, no solo en teoría.
2. **Restore siempre reemplaza, nunca fusiona** — ninguna lógica de merge/resolución de conflictos.
3. **Staging + swap atómico de dos `rename`**, con `vault-rescue` conservado hasta la revalidación
   posterior — un backup inválido en cualquier punto de la validación nunca toca el vault activo.
4. **`manifest.json` minimizado por el propio tipo Rust**, no por convención de código.
5. **Backup demasiado nuevo se rechaza explícitamente**, nunca se intenta un downgrade de esquema.
6. **No se asume que la contraseña actual abre un backup antiguo** — cada backup lleva su propio
   envelope encryption, sin modificar el mecanismo de cripto existente.
7. **Sin tokens de Google en el backup** — viven exclusivamente en el keychain del sistema.
8. **Modo WAL sigue sin activarse** — esta fase demuestra que no era necesario para resolver
   Backup/Restore; queda diferido por una razón de rendimiento futuro, no de backup.
9. **Sin backup automático, sin programación, sin rotación** — exactamente lo pedido: manual,
   explícito, iniciado por la usuaria.
10. **Dos dependencias nuevas, ambas pre-aprobadas**: `tauri-plugin-dialog` (diálogos nativos de
    archivo) y el crate `zip` (empaquetado del contenedor, sin compresión).

## Exclusiones explícitas de esta fase

Sincronización entre dispositivos, exportación abierta, Cierre/Alta, iPhone/iPad, empaquetado o
publicación de producción, actualizaciones automáticas, almacenamiento en la nube (Dropbox/iCloud/
OneDrive/Google Drive) como mecanismo de sincronización, biometría, Formulación, Evaluaciones,
Documentos clínicos, SII/Boletas, multiusuario, backups automáticos o programados, rotación de
backups, cualquier decisión sobre Sync/nube/cuentas/usuarios. Ninguna de estas áreas se tocó ni se
diseñó como efecto colateral de esta fase.

## Archivos creados o modificados

| Archivo | Rol |
|---|---|
| `src-tauri/src/backup/manifest.rs` (nuevo) | `BackupManifest`/`BackupFileEntry`, constantes de formato, 2 tests. |
| `src-tauri/src/backup/archive.rs` (nuevo) | ZIP (Stored) + SHA-256 streaming, protección zip-slip, 4 tests. |
| `src-tauri/src/backup/service.rs` (nuevo) | `create_backup`, `inspect_backup`, `restore_backup`, `run_startup_recovery`, `StagingGuard` — 32 tests. |
| `src-tauri/src/backup/mod.rs` (nuevo) | Registro del módulo. |
| `src-tauri/src/commands/backup.rs` (nuevo) | 3 comandos Tauri (`create_backup`, `inspect_backup`, `restore_backup`). |
| `src-tauri/src/security/mod.rs` | Reexporte aditivo: `VaultPaths`, `unlock_vault`, `recover_access`, `UnlockError`, `RecoveryError`. |
| `src-tauri/src/security/session.rs` | Método nuevo `refresh_from_disk()`. |
| `src-tauri/src/lib.rs` | `mod backup;`, plugin de diálogo, `run_startup_recovery` antes de `VaultSession::new`, registro de los 3 comandos. |
| `src-tauri/src/commands/mod.rs` | `mod backup;` + reexporte. |
| `src-tauri/capabilities/default.json` | `dialog:allow-open`, `dialog:allow-save`. |
| `src-tauri/Cargo.toml` | `zip = "8.6.0"` (sin features por defecto), `tauri-plugin-dialog = "2.7.3"`. |
| `src/features/backup/types.ts`, `api.ts`, `BackupRestoreSection.tsx` (nuevos) | Tipos IPC, diálogos nativos + invocaciones, UI completa. |
| `src/features/settings/SettingsScreen.tsx` | Sección "Respaldo y restauración" agregada. |
| `package.json`/`package-lock.json` | `@tauri-apps/plugin-dialog@2.7.3`. |

Ninguna migración de esquema SQL en esta fase (`schema_version` sigue en `4`, sin cambios).

## Tests ejecutados

`cargo test` en `src-tauri/`: **523/523 en verde** (491 previos sin cambios + 32 nuevos, todos en
`backup::service`). `cargo clippy --all-targets`: sin advertencias. `cargo build`: sin errores.
`npm run build`: sin errores. `npm run lint`: sin errores (warnings preexistentes de las mismas dos
categorías ya presentes desde fases anteriores — ninguno nuevo originado en un archivo de esta
fase).

Tests representativos de las reglas de negocio centrales:

| Requisito | Test |
|---|---|
| `VACUUM INTO` produce un SQLCipher válido con la misma clave, datos intactos | `backup_db_entry_opens_with_the_same_key_and_preserves_data` |
| El manifest nunca contiene datos de paciente | `manifest_never_contains_patient_data` |
| Crear un backup no modifica ni bloquea el vault activo | `creating_a_backup_does_not_modify_the_live_vault` |
| Backup rechazado si el vault está bloqueado, sin archivo parcial | `create_backup_is_rejected_while_vault_is_locked` |
| Nunca se sobrescribe un destino existente | `create_backup_rejects_an_existing_destination` |
| Restore reemplaza exactamente — la contraseña anterior deja de funcionar | `restore_over_an_existing_vault_replaces_it_exactly` |
| Restore sobre instalación nueva (sin vault previo) | `restore_onto_a_fresh_installation_with_no_existing_vault` |
| Un archivo que no es un ZIP válido no toca el vault actual | `a_failed_restore_leaves_the_previous_vault_intact` |
| Contraseña incorrecta no toca el vault actual | `restore_with_wrong_password_does_not_touch_the_current_vault` |
| Manifest corrupto no toca el vault actual | `restore_with_invalid_manifest_does_not_touch_the_current_vault` |
| Hash de archivo manipulado no toca el vault actual | `restore_with_tampered_file_hash_does_not_touch_the_current_vault` |
| Falta un archivo requerido no toca el vault actual | `restore_with_missing_required_file_does_not_touch_the_current_vault` |
| Base de datos corrupta dentro de un contenedor íntegro se rechaza | `restore_with_corrupt_database_inside_an_otherwise_valid_container_is_rejected` |
| Backup de esquema más nuevo se rechaza sin downgrade | `restore_rejects_a_backup_from_a_newer_schema_version` |
| Violación de integridad referencial detectada antes de promover | `restore_runs_foreign_key_check_and_rejects_a_violation` |
| Restore con código de recuperación fija una contraseña nueva funcional | `restore_with_recovery_code_sets_a_new_password_and_works` |
| Sin directorios de staging/rescue sobrantes tras un éxito | `restore_leaves_no_staging_or_rescue_directories_after_success` |
| Recuperación de arranque: crash a mitad del swap | `run_startup_recovery_restores_the_previous_vault_if_interrupted_between_rescue_and_promote` |
| Recuperación de arranque: rescue huérfano tras un restore ya completado | `run_startup_recovery_cleans_up_an_orphaned_rescue_after_a_completed_restore` |

## Limitaciones y decisiones que quedan pendientes de aprobación

- **Sin fixture de un backup genuinamente antiguo** (V1/V2/V3) para probar la migración de un
  backup real dentro del staging — ver la nota honesta más arriba. El camino de rechazo "demasiado
  nuevo" sí está probado con datos reales, y el mecanismo de migración en sí ya tiene su propia
  suite extensa reutilizada sin cambios.
- **Modo WAL sigue diferido** — esta fase demuestra que no bloqueaba Backup/Restore, pero no lo
  activa; queda como decisión de rendimiento puramente futura, sin síntoma actual que la justifique.
- Ninguna otra decisión de esta fase queda pendiente de aprobación — Sync, Export, Cierre/Alta,
  backups automáticos y todo lo demás fuera de alcance quedan documentados como **no
  implementados**, no como una extensión silenciosa de esta fase.
