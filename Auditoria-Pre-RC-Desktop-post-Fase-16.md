# Auditoría Pre-RC Desktop — post Fase 16

**Solo auditoría. Ningún hallazgo de este documento se implementó.** Documento único,
autocontenido, pensado para copiarse y entregarse completo a un tercero (incluido ChatGPT) sin
reconstruir contexto de mensajes anteriores.

Fase 16 ("Documentos y adjuntos clínicos cifrados") queda **cerrada y aprobada** a nivel de
implementación. Esta auditoría no la reabre — ningún hallazgo aquí compromete pérdida de datos,
confidencialidad, integridad o recuperación de forma que exija deshacer Fase 16; los hallazgos se
documentan, se demuestran y se clasifican, con recomendaciones explícitamente **no implementadas**.

---

## 1. Baseline real

Verificado con los comandos exactos pedidos, antes de cualquier análisis:

```
git rev-parse HEAD                                          → d240990fad66be2837bc2905f2d1dd72e9534437
git branch --show-current                                    → claude/cuaderno-clinico-desktop-udijjq
git status --porcelain=2                                     → (sin salida — árbol de trabajo limpio)
git log --oneline --decorate -15                              → ver sección 2
git rev-parse origin/claude/cuaderno-clinico-desktop-udijjq  → d240990fad66be2837bc2905f2d1dd72e9534437
```

Respuestas a las 7 preguntas del encargo:

1. **HEAD real**: `d240990` (`docs: agregar informe de cierre de Fase 16`).
2. **Rama real**: `claude/cuaderno-clinico-desktop-udijjq`.
3. **HEAD remoto**: `d240990` — idéntico al local, sin divergencia.
4. **¿Existe un commit documental posterior a `4f7c5a1`?** Sí: `d240990`.
5. **¿El informe de cierre de Fase 16 quedó versionado?** Sí — el nombre real del archivo es
   `Informe-de-cierre-Fase-16-Documentos-cifrados.md` (no
   `Informe-de-cierre-Fase-16-Formulacion.md`; ese nombre corresponde al patrón de la Fase 15 y
   parece un lapsus de copia en el encargo de esta auditoría). `git show --stat d240990` confirma
   un único archivo, 458 inserciones, exactamente ese informe.
6. **¿Working tree limpio?** Sí.
7. **¿Existe código productivo posterior a `4f7c5a1` no declarado?** No — `d240990` es
   estrictamente documental (1 archivo `.md`, cero cambios en `src-tauri/` ni `src/`).

**Conclusión**: la única diferencia frente a lo que el propio informe de Fase 16 declaraba como
HEAD (`4f7c5a1`) es el commit documental que agrega el informe mismo — exactamente el caso que el
encargo autorizó aceptar sin detenerse. Baseline real aceptado: `d240990`.

## 2. Git — detalle

```
d240990 (HEAD -> claude/cuaderno-clinico-desktop-udijjq, origin/...) docs: agregar informe de cierre de Fase 16
4f7c5a1 Fase 16: documentos y adjuntos clínicos cifrados
2c179ca docs: agregar auditoría y plan pendiente de aprobación de Fase 16
0845654 docs: agregar informe de cierre de Fase 15
8524f31 Fase 15: formulación clínica textual versionada
b42d356 docs: agregar auditoría de transición post-Fase 13 e informe de cierre de Fase 14
c4fb227 Fase 14: historial de cierres y hardening longitudinal
5427aee docs: agregar informe de cierre de Fase 13
bd6d9b1 Fase 13: evaluaciones clínicas y seguimiento psicométrico
684f048 fix: reforzar versionado y archivado del plan de seguridad
7f7bfbd Fase 12: plan de seguridad clínico versionado
d66686b fix: unificar semántica de archivado de pacientes
7e95dc9 Fase 11: cierre estructurado de procesos terapéuticos
6ae28f1 Fase 10: Backup y restauración segura
01743b6 Fase 9: episodios terapéuticos mínimos (treatment_episodes)
```

Historial lineal, sin merges, sin rebases, sin force-push — consistente con la política del
proyecto de nunca reescribir historia.

## 3. Regresión real del baseline (ejecutada, no asumida)

Ejecutado literalmente desde `d240990`, sin dar por sentado el "766/766" del informe de Fase 16:

| Comando | Resultado real |
|---|---|
| `cargo test --release` | **765 passed; 1 FAILED** (ver hallazgo abajo — no es 766/766) |
| `cargo clippy --release --all-targets` | 0 warnings (confirmado) |
| `cargo build --release` | limpio (confirmado) |
| `npm run build` | limpio, sin errores TS, 54.99s (confirmado) |
| `npm run lint` | 25 warnings / 0 errors (confirmado, coincide exactamente con Fase 16) |
| `git diff --check` | limpio (confirmado) |

### Hallazgo REG-1 — test no determinista (flaky), no es una regresión funcional

**Test fallido**: `services::document_crypto::tests::resolve_within_files_root_rejects_an_uppercase_shard`.

**Diagnóstico** (código en `src-tauri/src/services/document_crypto.rs`):

```rust
#[test]
fn resolve_within_files_root_rejects_an_uppercase_shard() {
    let root = Path::new("/vault/files-root");
    let id = Uuid::new_v4();
    let shard_upper = id.to_string()[0..2].to_uppercase();
    let bad = format!("files/{shard_upper}/{id}.enc");
    let err = resolve_within_files_root(root, &bad).unwrap_err();
    assert!(matches!(err, StoragePathError::InvalidFormat));
}
```

El test construye un "shard inválido" poniendo en mayúsculas los dos primeros caracteres hex de un
`Uuid::new_v4()` **aleatorio**. Cuando esos dos caracteres son dígitos (`0`-`9`) —
`.to_uppercase()` sobre un dígito es un no-op— el `shard_upper` resultante es **byte a byte
idéntico** al shard válido en minúsculas. El "path malicioso" que el test cree estar construyendo
es, en ese caso, un path **legítimo**: `resolve_within_files_root` devuelve correctamente `Ok(..)`
(no hay ningún bypass de seguridad — un shard compuesto solo de dígitos es legítimamente invariante
a mayúsculas/minúsculas) y `.unwrap_err()` entra en pánico.

**Confirmado empíricamente**, sin tocar ningún código: 8 reejecuciones aisladas del mismo test
produjeron una mezcla de `ok`/`FAILED` (aproximadamente 4 de 8), consistente con la probabilidad
teórica de que ambos nibbles hexadecimales de un UUID aleatorio sean dígitos.

**Esto no es una regresión de seguridad.** `resolve_within_files_root` (la función de producción)
es correcta; el defecto está exclusivamente en la construcción de datos de prueba del test. No se
tocó ni se debilitó el test — se diagnosticó y se deja documentado, según instrucción explícita de
esta auditoría de no implementar código.

- **Clasificación**: **B — MUST FIX BEFORE RC**. Justificación: es un test de la suite de
  seguridad de Documentos (path traversal), y un test no determinista socava exactamente la
  garantía que el proyecto exige de sí mismo ("nunca eliminar ni debilitar un test", regresión
  obligatoria como señal confiable). Dejarlo así arriesga que una futura ejecución de CI/regresión
  reporte una "regresión" falsa y genere ruido o, peor, que alguien lo silencie incorrectamente.
- **Recomendación (no implementada)**: reemplazar el UUID aleatorio del test por un valor fijo
  cuyo shard contenga al menos una letra hex (p. ej. un UUID literal conocido), o generar en un
  bucle hasta obtener un shard con letra. Cambio de una sola función de test; cero riesgo, cero
  impacto en `resolve_within_files_root`.
- **Archivo**: `src-tauri/src/services/document_crypto.rs` (módulo `#[cfg(test)]` únicamente).
- **Tests/migración/dependencia**: no requiere tests nuevos, no requiere migración, no requiere
  dependencia nueva.

## 4. Inventario funcional real (por código, no solo documentación)

Verificado contra `src-tauri/src/commands/mod.rs` (19 módulos de comando registrados) y contra la
lectura directa de cada vertical a lo largo de esta sesión y de las anteriores:

| Módulo | Estado | Evidencia |
|---|---|---|
| Vault / Security | IMPLEMENTADO | `security::session`/`vault_manager`/`envelope`/`kdf` — creación, unlock, lock, auto-lock, recovery, cambio de contraseña, todos con tests y verificados en esta auditoría (§20) |
| Pacientes | IMPLEMENTADO | CRUD completo + archivar/restaurar, validación de RUT chileno |
| Antecedentes clínicos | IMPLEMENTADO | `patient_clinical_profile`, registro mutable simple |
| Ubicación / Estadísticas | IMPLEMENTADO | región/comuna + `services::patients` estadísticas + pantalla dedicada |
| Agenda | IMPLEMENTADO | `appointments`, CRUD + solapamiento + bloqueo personal |
| Google Calendar | IMPLEMENTADO (código) / PENDIENTE HARDWARE (prueba real con cuenta) | OAuth PKCE, payload minimizado verificado en §25; nunca ejercitado de punta a punta contra Google real en ningún entorno de esta sesión |
| Sesiones | IMPLEMENTADO | `sessions` + `session_notes` versionadas append-only |
| Notas versionadas | IMPLEMENTADO | ver arriba, con historial de solo lectura |
| Objetivos | IMPLEMENTADO | `goals` + `goal_indicators` + `session_goals` |
| Pagos | IMPLEMENTADO | `payments`, estado derivado "atrasado" en lectura |
| Continuidad | IMPLEMENTADO | `patient_prep_notes` + `therapy_tasks` |
| Procesos terapéuticos | IMPLEMENTADO | `treatment_episodes`, un activo por paciente |
| Cierre | IMPLEMENTADO | `episode_closures`, inmutable, reapertura con motivo |
| Historial de cierres | IMPLEMENTADO | `ClosureHistorySection` (Fase 14) |
| Plan de Seguridad | IMPLEMENTADO | `safety_plans` versionado borrador/vigente/reemplazado |
| Evaluaciones | IMPLEMENTADO | `assessment_instruments`/`assessment_administrations`, sin contenido propietario |
| Formulación | IMPLEMENTADO (textual) | `case_formulations`/`formulation_versions`; Formulación Visual (`formulation_nodes`/`edges`) NO IMPLEMENTADO, reservado |
| Documentos | IMPLEMENTADO | Fase 16, auditado en profundidad §6-§21 de este documento |
| Backup | IMPLEMENTADO, con 1 hallazgo (§17) | `create_backup`, incluye documentos desde Fase 16 |
| Restore | IMPLEMENTADO, verificado sólido (§18) | `restore_backup`, staging + swap atómico |

**No se infirió ningún estado solo desde documentación** — cada fila se verificó contra el código
real (`src-tauri/src/commands/mod.rs`, `services/*`, `repositories/*`) durante esta sesión y las
que la precedieron dentro del mismo hilo de trabajo.

Módulos que **no existen en el código** (confirmado por la ausencia de su `mod` en
`commands/mod.rs`, `services/mod.rs`, `repositories/mod.rs`, y por grep de nombres): Línea
temporal, Recordatorios, Biblioteca/Hub de estudio, Herramientas clínicas, Export general,
Formulación visual, Sync, iOS/iPadOS, biometría, updater. Ver §5.

## 5. Features todavía ausentes — clasificación

| Feature | Clasificación | Justificación |
|---|---|---|
| Línea temporal | RECOMENDABLE V1.1 | Vista agregada de solo lectura (sesiones/cierres/formulaciones/evaluaciones/documentos por fecha) — no requiere tabla nueva, sin dependencia funcional de ninguna otra fase. Útil pero ninguna funcionalidad clínica depende de que exista. |
| Recordatorios | POST-V1 | Sin diseño previo; notificaciones internas vs. del SO es una decisión de producto no resuelta. Nada depende de esto para V1. |
| Biblioteca / Hub de estudio | POST-V1 | Fuera del núcleo clínico operacional; no bloquea nada. |
| Herramientas clínicas | POST-V1 | Mismo criterio — ampliación de producto, no requisito de cierre. |
| Export general | RECOMENDABLE V1.1 (no blocker) | Ver análisis dedicado en §42 — Documentos ya cubre exportación individual; un export "de todo" no tiene ninguna dependencia funcional que bloquee V1. |
| Formulación visual | POST-V1 | Explícitamente diferida desde la Fase 15; `formulation_nodes`/`edges` reservadas sin usar. Ninguna funcionalidad depende de esto. |
| Sync | POST-V1 (fase dedicada obligatoria) | CLAUDE.md regla 6 lo exige como fase específica separada — nunca improvisado. No es candidato ni a V1.1. |
| iOS/iPadOS | POST-V1 (fase dedicada obligatoria) | CLAUDE.md regla 7 — requiere fase específica. Fuera de alcance de Desktop RC por definición. |
| Biometría | POST-V1 | Conveniencia de desbloqueo, no cambia ninguna garantía de seguridad existente (contraseña/recovery siguen siendo la base). Ninguna dependencia funcional. |
| Updater | **BLOCKER V1** (no de RC1) | Ver §27 — una V1 para uso profesional sin mecanismo de actualización deja a la usuaria sin forma de recibir parches de seguridad; RC1 puede vivir sin él (builds de prueba se redistribuyen manualmente), V1 no debería. |

Ninguna de estas se consideró blocker solo por ser útil — el criterio aplicado en cada fila es
"¿alguna otra funcionalidad de V1 deja de funcionar o de ser segura sin esto?".

## 6-9. Auditoría post-implementación de Documentos, criptografía, nonces, CCD1, `sha256_plaintext`

Revisado directamente (no solo el informe de Fase 16): `security/session.rs`,
`services/document_crypto.rs`, `services/document_temp.rs`, `services/documents.rs`,
`repositories/documents.rs`, `commands/documents.rs`, `backup/service.rs`, `commands/vault.rs`,
`lib.rs`, `SCHEMA_V9` (en `db/migrations.rs`), `src/features/documents/*`.

### Confirmado (coincide con lo declarado en el informe de Fase 16)

- Envelope encryption real: DEK de archivo de 256 bits, aleatoria e independiente por documento
  (`generate_file_dek`, `getrandom`), nunca almacenada en claro.
- `wrap_file_key`/`unwrap_file_key` son los únicos dos puntos donde `services::document_crypto`
  toca algo de `security::session` — la DEK cruda del vault nunca cruza esa frontera (verificado
  leyendo la firma y el cuerpo completo de ambos métodos).
- Nonce de contenido (`CONTENT_NONCE_LEN = 12`) y nonce de envoltura
  (`FILE_KEY_WRAP_NONCE_LEN = 12`) se generan de forma independiente y aleatoria en cada llamada
  (`getrandom::fill`/`random::bytes`), nunca derivados ni fijos — sin riesgo de colisión práctico
  bajo el volumen de uso esperado de una app de escritorio de un solo usuario.
- Zeroización: `FileKey` (Drop + zeroize), el `plaintext` intermedio de `unwrap_file_key` se
  zeroiza en **ambos** caminos (éxito y `InvalidLength`) antes de retornar — exception-safe.
- Formato `CCD1` verificado: truncamiento, magic inválido, versión desconocida y fallo de
  autenticación se detectan en ese orden exacto, y **nunca hay aceptación silenciosa** de una
  discrepancia — cualquier bit modificado (incluyendo bytes extra anexados al final del archivo,
  que AES-GCM incluye dentro de lo que autentica) hace fallar la autenticación explícitamente.
- Discrepancia `documents.format_version` (SQL) vs. el byte de versión del header físico: no hay
  una comparación *directa* entre ambos, pero el resultado es equivalente — `get_document_content`
  rechaza si la columna SQL no coincide con la constante soportada, y `decrypt_document`
  independientemente rechaza si el header físico tampoco coincide. Ninguna combinación posible
  termina en una aceptación silenciosa.
- `sha256_plaintext`: se reafirma el análisis ya hecho en el cierre de Fase 16 — es una
  verificación adicional de integridad del contenido descifrado (no del ciphertext), no
  reemplazable por la autenticación de AES-GCM porque cubre un punto distinto de la cadena (que la
  lectura del archivo de origen al importar no se corrompió antes de cifrar). Conservarla para V1
  es razonable; no constituye una huella dactilar explotable porque vive exclusivamente dentro de
  SQLCipher, nunca se expone por IPC.

### Hallazgo CRYPTO-1 — reutilización de la DEK del vault sin separación de dominio criptográfico

**Evidencia**:
- `src-tauri/src/db/connection.rs`: la DEK del vault se aplica directamente como clave cruda de
  SQLCipher (`PRAGMA key = "x'<64 hex>'"`, modo raw key de 32 bytes).
- `src-tauri/src/security/session.rs` (`wrap_file_key`/`unwrap_file_key`, Fase 16): la **misma**
  DEK (`session.dek.expose_secret()`) se usa **directamente** como clave AES-256-GCM
  (`Aes256Gcm::new(&Key::from(*dek))`) para envolver/desenvolver las DEKs de archivo de Documentos.
- No existe ninguna derivación de subclave (p. ej. HKDF con una cadena de contexto distinta por
  propósito) entre ambos usos — el mismo material de 32 bytes entra directo en dos consumidores
  criptográficos distintos.
- `security/envelope.rs` (`wrap_dek`/`unwrap_dek`) es un uso **diferente y no problemático**: ahí
  la DEK es el *plaintext* que se envuelve con una KEK derivada de la contraseña/código de
  recuperación vía Argon2id — no hay solapamiento de material de clave con el hallazgo anterior.

**Impacto y probabilidad**: bajo en la práctica. SQLCipher no usa el raw key directamente como
clave AES sin más — internamente deriva claves de cifrado y HMAC distintas a partir del material
recibido (parte de su propio diseño de "cipher provider"), lo que da cierta separación *de facto*
del lado de SQLCipher. No existe un ataque conocido y demostrado que explote esta reutilización
específica. Aun así, viola el principio de ingeniería "una clave, un propósito" y aumenta el radio
de impacto ante cualquier debilidad futura no detectada en cualquiera de las dos implementaciones.

**Clasificación**: **B — MUST FIX BEFORE RC**, con la salvedad honesta de que la urgencia viene de
disciplina de ingeniería y defensa en profundidad, no de un exploit demostrado. Dado que Documentos
es la única fase que introdujo un segundo consumidor de la DEK cruda, y que el propio diseño de
Fase 16 ya fue meticuloso en no exponer la DEK fuera de `security::session`, cerrar esta brecha es
coherente con el nivel de rigor que el proyecto ya se exige a sí mismo.

- **Recomendación (no implementada)**: derivar, dentro de `wrap_file_key`/`unwrap_file_key`, una
  subclave dedicada vía HKDF-SHA256 a partir de la DEK del vault con una cadena de contexto fija
  (p. ej. `"cuaderno-clinico:file-key-wrap:v1"`), y usar esa subclave — nunca la DEK cruda — como
  clave AES-256-GCM de envoltura. Requiere agregar el crate `hkdf` (familia RustCrypto, ya
  consistente con `aes-gcm`/`sha2` presentes) — **dependencia nueva, requiere aprobación explícita
  previa** por la regla 11 de `CLAUDE.md`.
- **Archivos afectados**: `src-tauri/src/security/session.rs` únicamente (mismo archivo ya
  autorizado en Fase 16, sin ampliar el alcance de `security/*`).
- **Migración**: ninguna — es un cambio de cómo se deriva la clave de envoltura, no del formato
  almacenado (`wrapped_file_dek`/`wrap_nonce` siguen siendo el mismo tipo de dato).
- **Tests necesarios**: roundtrip con la subclave derivada, verificación de que una DEK envuelta
  con la versión anterior deja de poder desenvolverse (documentar como **cambio incompatible que
  requiere migración de datos o versión explícita del esquema de envoltura** si se implementa en
  el futuro — no se decide aquí, solo se señala como consecuencia a evaluar).

### Hallazgo CRYPTO-2 — ausencia de AAD (informativo, no es una vulnerabilidad)

Ni `wrap_file_key`/`unwrap_file_key` ni `encrypt_document`/`decrypt_document` usan Additional
Authenticated Data. Se analizaron los dos escenarios de intercambio (ciphertext físico entre dos
documentos; fila `wrapped_file_dek`/`wrap_nonce` entre dos documentos) y en **ambos casos** la
independencia de la DEK por archivo, combinada con la autenticación AEAD a nivel de contenido, ya
detecta el intercambio como un fallo de autenticación — sin necesitar AAD explícito.

- **Clasificación**: **E — DOCUMENTAL**. Recomendación: mencionar esta propiedad ya cubierta
  implícitamente en `docs/documents.md`, sin cambiar código.

### Hallazgo CRYPTO-3 — zeroización no exception-safe en `create_document`

En `services::documents::create_document`, `file_dek` es un `[u8; 32]` plano (no un tipo con
`Drop`-zeroize). Si `encrypt_document(...)` fallara (ventana angosta: solo `Empty`/`TooLarge`/fallo
de `getrandom`, ya prevalidados antes, pero no estructuralmente imposible), la función retorna vía
`encrypted?` **antes** de alcanzar el `zeroize_plaintext(&mut file_dek)` que solo corre después de
`wrap_file_key`. El array queda en el stack sin zeroizar explícitamente hasta ser sobrescrito por
uso posterior del mismo stack frame. Por contraste, `unwrap_file_key`'s `FileKey` sí es
exception-safe (el `Drop` zeroiza en cualquier punto de salida de scope).

- **Clasificación**: **C — SHOULD FIX**. Riesgo real bajo (memoria de proceso local; requiere
  acceso de lectura de memoria — coredump, debugger, cold-boot — para explotar, threat model de
  escritorio de un solo usuario).
- **Recomendación (no implementada)**: envolver `file_dek` en `zeroize::Zeroizing<[u8; 32]>`
  (mismo crate `zeroize` ya usado en el proyecto, cero dependencia nueva) para que el `Drop`
  zeroice automáticamente en cualquier retorno temprano.
- **Archivo**: `src-tauri/src/services/documents.rs` (función `create_document`).

## 10. Symlinks / junctions / reparse points — traversal físico

Fase 16 cubrió traversal **sintáctico** (`resolve_within_files_root`, 8 tests). Esta auditoría
cubrió traversal **físico**: grep exhaustivo en todo `src-tauri/src/` confirma **cero** usos de
`canonicalize`, `symlink_metadata`, `read_link`, `O_NOFOLLOW` o equivalentes Windows en cualquier
punto del código. Ningún camino de E/S sobre `vault/files/*` verifica la naturaleza real del inodo
antes de abrir/leer/escribir/renombrar.

Análisis de explotabilidad por operación (threat model: app local, un solo usuario, sin servidor):

- **Import (escritura)**: el path final depende de un UUID recién generado, impredecible — un
  atacante necesitaría plantar el symlink en la ruta exacta antes de que la app escriba ahí, lo
  que exige ya tener escritura en `vault/files/` y ganar una carrera contra 122 bits de
  aleatoriedad. Impracticable sin ya tener control del directorio del vault (en cuyo caso hay
  ataques más directos disponibles). **HARDENING**, no blocker.
- **Lectura (abrir/exportar)**: si un `.enc` existente fuese reemplazado por un symlink, `fs::read`
  seguiría el symlink pero `decrypt_document` trataría el contenido leído como ciphertext —
  fallaría la autenticación AES-GCM (o el magic/versión) casi con certeza, nunca filtra el
  contenido del archivo enlazado. Verificado por el propio diseño del formato, sin necesitar
  hardware. **NO APLICA**.
- **Backup (`create_backup`)**: usa `std::fs::copy(&source, &dest)`, que **sí sigue symlinks** en
  Rust estándar. Si un atacante con escritura local ya lograda en `vault/files/` reemplazara un
  `.enc` por un symlink a un archivo arbitrario del disco (p. ej. una clave SSH), `create_backup`
  copiaría el contenido de ese archivo ajeno dentro del `.cclinbackup` resultante — que después
  puede salir del equipo (nube, USB). Es el escenario de mayor impacto marginal porque convierte
  una manipulación local en un canal de exfiltración que sobrevive en un artefacto exportable.
  - **Clasificación**: **C — SHOULD FIX**. Recomendación (no implementada): usar
    `std::fs::symlink_metadata(&source)` y rechazar/errar si `file_type().is_symlink()` antes de
    copiar — defensa en profundidad barata, mismo criterio ya aplicado para validar
    `storage_path`. Archivo: `src-tauri/src/backup/service.rs` (`create_backup`).
- **Restore (extracción del ZIP)**: revisado `backup/archive.rs::extract_container` — usa
  `entry.enclosed_name()` (rechaza rutas inseguras) y **siempre** escribe cada entrada con
  `File::create` + `io::copy`, tratando el contenido como bytes de archivo normal — nunca
  interpreta bits de permisos Unix ni crea symlinks reales en disco aunque una entrada manipulada
  del ZIP declarara serlo. **VERIFICADO SEGURO por código**, sin necesitar hardware.

## 11. TOCTOU

Analizado con el threat model real (app local, un solo usuario, sin servidor), sin exagerar el
riesgo:

- Ventana entre `fs::metadata` (chequeo de 50MB en `create_document`) y la lectura real: si el
  archivo de origen creciera en esa ventana, `encrypt_document` lo rechaza de todas formas
  (`TooLarge`) al validar el buffer ya leído — sin corrupción, solo una falla explícita algo más
  tarde de lo ideal.
- Ventana entre `resolve_within_files_root` (validación sintáctica) y la escritura/rename real:
  el path final es un UUID fresco sin proceso adversario compitiendo por esa ruta exacta en un
  sistema de un solo usuario. Riesgo teórico, impacto despreciable para V1.

**Clasificación consolidada**: NO APLICA / bajo impacto — no se considera blocker ni de RC ni de
V1 dado el modelo de amenaza declarado explícitamente por la usuaria.

## 12. Multi-instance

Confirmado por grep: **cero** mecanismo de single-instance (`tauri-plugin-single-instance` no está
en `Cargo.toml`; ningún lock file/flock manual en `lib.rs`). Nada impide lanzar dos procesos de
Cuaderno Clínico simultáneamente apuntando al mismo `app_data_dir`.

- **SQLCipher/`vault.db`**: sin modo WAL (decisión ya documentada desde Fase 10), usa rollback
  journal — SQLite soporta multiacceso multiproceso vía locking de archivo del SO en este modo; un
  segundo proceso escribiendo mientras el primero también escribe puede recibir `SQLITE_BUSY` (no
  se encontró ningún `busy_timeout` configurado en `db/connection.rs` — comportamiento por defecto:
  falla inmediata sin reintento). Se traduce en un error visible a la usuaria, **no** en corrupción
  del archivo.
- **Backup — instante consistente, verificado con más rigor que "confianza"**: `create_document`
  siempre **escribe el ciphertext físico antes de insertar la fila** en `documents` (orden ya
  documentado en Fase 16). Esto significa que cualquier fila visible en el snapshot que
  `create_backup` lee dentro de su `with_connection` (junto al `VACUUM INTO`) **ya tiene** su
  archivo físico en disco en ese instante, incluso si la escritura la hizo otra instancia
  concurrente — el invariante write-then-insert es lo que realmente sostiene la afirmación del
  informe de Fase 16, más allá de la coincidencia de estar en la misma llamada.
  **VERIFICADO POR CÓDIGO**.
- **Backup — destino duplicado entre instancias**: el check inicial
  `if dest_path.exists() { return Err }` no es atómico por sí solo, pero
  `archive::write_container` usa `File::create_new(dest)` internamente (apertura exclusiva
  atómica) — aunque dos instancias pasaran el check "a la vez" sobre el mismo `dest_path`, solo
  una gana la escritura real; la otra falla con un error de E/S explícito, nunca con un archivo
  mezclado. **VERIFICADO SEGURO**.
- **Caso crítico señalado explícitamente por el encargo — `sweep_stale_temp_files` entre
  instancias**: **confirmado como hallazgo real**. Corre solo al arranque de cada instancia, antes
  de `VaultSession::new`, y borra **todo** archivo bajo el directorio temporal del sistema cuyo
  nombre empiece con `cuaderno-clinico-doc-`, sin distinguir de qué proceso es ni si sigue
  legítimamente en uso. Escenario concreto: Instancia A tiene un PDF abierto externamente
  (temporal legítimo, registrado solo en la memoria del proceso de A). Se lanza Instancia B (nada
  lo impide) → el `setup()` de B ejecuta el barrido → B no tiene forma de saber que ese archivo
  pertenece a una sesión legítima de A todavía en uso → lo borra. En Windows, si el visor externo
  ya tiene el archivo abierto sin `FILE_SHARE_DELETE`, `remove_file` fallaría silenciosamente (el
  código ignora el resultado con `let _ =`) — sin crash, pero sin garantía de limpieza tampoco; en
  macOS/Linux, `unlink` sobre un archivo abierto no rompe al proceso que ya lo tiene abierto, pero
  cualquier reapertura posterior por ruta (algunos visores la hacen al refrescar/guardar) falla con
  "archivo no encontrado".
  - **Clasificación**: **B — MUST FIX BEFORE RC**. Es una acción cotidiana plausible (doble clic
    accidental, acceso directo duplicado, "abrir con" dos veces desde el explorador de archivos),
    sin necesidad de ningún atacante, con consecuencia visible de "mi PDF desapareció" — tensiona
    el espíritu de "no pérdida de datos" aunque el dato cifrado del vault nunca se pierde (solo la
    copia temporal descifrada que se estaba viendo).
  - **Recomendación (no implementada)**, dos vías independientes, cualquiera cierra el hueco: (a)
    aplicar single-instance real (`tauri-plugin-single-instance`, dependencia menor y muy
    establecida — **requiere aprobación explícita** por la regla 11 de `CLAUDE.md`), que además
    resolvería de un solo golpe el punto de `SQLITE_BUSY`; o (b) si se decide permitir
    multi-instancia deliberadamente, hacer `sweep_stale_temp_files` consciente de antigüedad (solo
    borrar temporales con `mtime` anterior al arranque de este proceso menos un margen, nunca "todo
    lo que matchee el prefijo").
  - **Archivos**: `src-tauri/src/lib.rs`, `src-tauri/src/services/document_temp.rs`, y `Cargo.toml`
    si se opta por la vía (a).

## 13. Preview de imágenes y `data:` URL

Lo que **sí** se verificó por código (sin necesitar hardware):

- `get_document_data_url` descifra en memoria, codifica a base64, y **zeroiza el buffer fuente**
  antes de retornar el `data:` URL — no hay ninguna llamada a `std::fs::write` en ese camino de
  código. Confirmado leyendo `commands/documents.rs` completo.
- El CSP de `tauri.conf.json` (`img-src 'self' asset: https://asset.localhost data:`) permite
  explícitamente `data:` para imágenes, sin abrir la puerta a otros orígenes remotos.

Lo que **no se puede afirmar sin hardware real**, y no se afirma: si WKWebView (macOS) o WebView2
(Windows, basado en Chromium/Edge) persisten en disco, como parte de su propio funcionamiento
interno, una copia del `data:` URL decodificado — vía caché de disco del motor de renderizado,
caché de texturas GPU, snapshots de "pestaña recientemente cerrada"/restauración de sesión, o
volcados de crash del proceso de renderizado. **"No creamos archivo" no es lo mismo que "el
WebView nunca persiste contenido"** — exactamente la distinción que pedía el encargo.

- **VERIFICADO POR CÓDIGO**: la aplicación nunca escribe la imagen descifrada a disco por su
  cuenta; el buffer se zeroiza tras usarse.
- **REQUIERE PRUEBA FÍSICA**: inspección directa, en macOS real y Windows real, de:
  - macOS: `~/Library/Caches/com.jpcaamano.cuadernoclinico/**`, cachés de WKWebView bajo
    `~/Library/WebKit/**` o el contenedor de la app, `~/Library/Saved Application State/**`.
  - Windows: `%LOCALAPPDATA%\com.jpcaamano.cuadernoclinico\EBWebView\**` (perfil de WebView2),
    volcados de crash (`%LOCALAPPDATA%\CrashDumps`), Prefetch.
  - Ambos: memoria del proceso tras cerrar el documento (swap/paginación), para confirmar que el
    `data:` URL no sobrevive más allá de la sesión de visualización activa.
  Este protocolo queda **diseñado, no ejecutado** — se incorpora al Pre-V1 Manual Acceptance Test
  (§30).

## 14. Temporales plaintext

Ya cubierto en detalle en §12 (caso multi-instance). Complementario:

- Ubicación: siempre `std::env::temp_dir()`, nunca `vault/files/` — verificado por código.
- Naming: UUID nuevo + prefijo reconocible `cuaderno-clinico-doc-` — opaco, sin nombre real del
  documento.
- Permisos: no se fija ningún permiso explícito al crear el temporal (`std::fs::write` usa el
  `umask` por defecto del proceso/SO) — ver §15.
- Cleanup: bloqueo manual, auto-lock, `RunEvent::Exit`, y barrido de arranque — los cuatro puntos
  existen y están conectados (verificado en `lib.rs`/`commands/vault.rs`), pero el barrido de
  arranque es ciego respecto a otras instancias en ejecución (hallazgo de §12).
- **Windows — caso específico del encargo ("PDF abierto externamente → Cuaderno Clínico intenta
  eliminar temporal → Windows puede negar `remove_file`")**: confirmado por código que
  `cleanup_all()` usa `let _ = std::fs::remove_file(&path);` — el resultado se ignora
  explícitamente. Si Windows niega el borrado porque el visor externo mantiene el archivo abierto,
  la limpieza de esa entrada específica simplemente no ocurre en ese intento, sin error visible ni
  reintento. **¿El próximo arranque vuelve a intentarlo?** Sí — `sweep_stale_temp_files()` corre en
  cada arranque y volvería a intentar borrar cualquier archivo con el prefijo reconocible que siga
  presente, incluyendo este caso. **¿Existe riesgo de plaintext persistente indefinidamente?**
  Solo mientras la aplicación externa mantenga el archivo abierto y la usuaria no reinicie Cuaderno
  Clínico — no hay reintento *durante* la misma sesión (ningún timer periódico reintenta la
  limpieza), solo al bloquear/cerrar/reabrir. Clasificado como parte del mismo hallazgo B de §12
  (no es un hallazgo nuevo independiente, es una manifestación adicional del mismo diseño).

## 15. Permisos del filesystem

No se encontró ningún código que fije explícitamente permisos POSIX (`std::os::unix::fs::PermissionsExt`)
ni ACLs de Windows sobre `vault/`, `vault.db`, `vault.meta.json`, `vault/files/`, o los temporales.
Todo se crea con los permisos por defecto que el `umask`/perfil del SO asignan al proceso — en la
práctica, en una instalación de un solo usuario con `app_data_dir` bajo el perfil del propio
usuario del SO (macOS/Windows/Linux), esto normalmente ya excluye a otras cuentas del sistema por
la propia jerarquía de directorios del perfil de usuario.

- **Clasificación**: no se encontró ningún hallazgo que amerite ser blocker — para una app
  desktop single-user, depender de los permisos por defecto del directorio de perfil del SO es una
  práctica común y razonable. **D — POST-V1 (hardening opcional)**: fijar explícitamente permisos
  restrictivos (`0700`/`0600` en Unix, ACL heredada correcta en Windows) al crear `vault_dir` y
  cada archivo dentro, como defensa en profundidad adicional para escenarios de disco compartido o
  cuentas de sistema mal configuradas — no requerido para V1 dado el modelo de amenaza.

## 16-17. Backup — consistencia y atomicidad

**Consistencia** (ver también §12): la enumeración de `document_storage_paths` ocurre dentro de la
misma `with_connection` que `VACUUM INTO`, y el invariante write-then-insert de `create_document`
hace que cualquier fila en ese snapshot ya tenga su archivo físico en disco. No hay transacción
distribuida DB+filesystem —no existe tal cosa en general— pero el diseño evita necesitarla:
**ACEPTABLE, con matices**: sólido para el caso single-process; para multi-instance, sigue siendo
sólido para la consistencia snapshot-vs-lista (el invariante no depende de cuántos procesos
escriben), pero dos instancias podrían competir por E/S sobre el mismo `vault_dir` sin ninguna
coordinación adicional más allá del propio locking de SQLite — no se identificó un escenario
concreto de corrupción, solo de errores visibles (`SQLITE_BUSY`).

### Hallazgo BACKUP-1 — `create_backup` puede dejar un `.cclinbackup` parcial en el destino final

**Evidencia** (`src-tauri/src/backup/service.rs`, función `create_backup`): el paso final del
cierre `(|| -> Result<...> { ... })()` es:

```rust
let entry_refs: Vec<(&str, &Path)> = entries.iter().map(|(name, path)| (name.as_str(), path.as_path())).collect();
archive::write_container(dest_path, &entry_refs)?;
```

`write_container` (en `backup/archive.rs`) escribe **directamente sobre `dest_path`** (el destino
final elegido por la usuaria, vía diálogo nativo "Guardar como…"), no sobre un archivo temporal que
luego se renombre atómicamente:

```rust
pub fn write_container(dest: &Path, entries: &[(&str, &Path)]) -> Result<(), ArchiveError> {
    let file = File::create_new(dest)?;         // <- crea el archivo REAL de destino ya aquí
    let mut zip = ZipWriter::new(file);
    ...
    for (entry_name, real_path) in entries {
        zip.start_file(*entry_name, options)?;
        let mut source = BufReader::new(File::open(real_path)?);
        io::copy(&mut source, &mut zip)?;        // <- puede fallar a mitad de camino
    }
    zip.finish()?;
    Ok(())
}
```

Si `io::copy`/`zip.start_file`/`zip.finish()` fallan a mitad de camino (disco lleno, permisos,
crash del proceso), `File::create_new(dest)` **ya creó** el archivo en `dest_path` antes de que el
error ocurra. Al final de `create_backup`, el único cleanup existente es:

```rust
let _ = std::fs::remove_dir_all(&scratch);   // limpia SOLO el área de scratch
result                                        // el error se propaga, dest_path NUNCA se toca
```

**No existe ninguna ruta de código que borre o mueva fuera `dest_path` cuando `write_container`
falla.** El resultado: un `.cclinbackup` parcial, con nombre y ubicación elegidos por la usuaria,
queda en el destino — indistinguible a simple vista (por listado de archivos) de un backup
completado con éxito, hasta que se intenta restaurarlo.

**¿Puede este archivo parcial "pasar" como válido al restaurar?** No — `restore_backup` sí
detectaría el problema (ZIP corrupto/incompleto → `RestoreError::ArchiveUnreadable`, o manifest
ausente/hash no coincidente) y rechazaría el restore explícitamente, sin tocar el vault activo. El
riesgo **no** es que una restauración parcial pase inadvertida — es que la usuaria crea tener un
respaldo válido (el archivo existe, con el nombre que ella eligió) y solo descubra que está roto en
el peor momento posible: cuando de verdad lo necesita.

- **Clasificación**: **B — MUST FIX BEFORE RC**. Backup es el mecanismo de recuperación central del
  proyecto; CLAUDE.md es explícito en que "la base de datos es crítica" y que debe existir "un
  respaldo real del que la información pueda recuperarse de forma segura y verificada". Un backup
  que puede parecer completo sin estarlo, sin que el sistema lo detecte hasta la restauración,
  viola ese principio de forma directa.
- **Recomendación (no implementada)**: aplicar exactamente el mismo patrón que el propio proyecto
  ya usa en otros dos lugares (import de Documentos: temporal + `rename` atómico; restore: staging
  + `rename` atómico) — que `write_container` escriba a `dest_path.tmp` (o a un archivo dentro del
  propio directorio `scratch`) y solo se haga `std::fs::rename` a `dest_path` **después** de que
  `zip.finish()` haya tenido éxito; en cualquier error, limpiar el temporal. Cambio autocontenido,
  sin necesidad de nueva dependencia, consistente con patrones ya probados en el mismo código base.
- **Archivos**: `src-tauri/src/backup/archive.rs` (`write_container`) y/o la orquestación en
  `src-tauri/src/backup/service.rs` (`create_backup`).
- **Migración/dependencia**: ninguna.
- **Tests necesarios**: simular un fallo de `io::copy` a mitad de la escritura del contenedor
  (p. ej. mockeando o truncando una de las fuentes) y verificar que `dest_path` **no** existe tras
  el error — mismo patrón que los tests ya existentes de "no deja archivos huérfanos tras un fallo"
  en otras partes de `backup/service.rs`.

## 18. Restore — atomicidad (punto marcado como crítico por el encargo)

Se examinó la implementación completa de `restore_backup` (`src-tauri/src/backup/service.rs`,
~130 líneas). Hallazgo principal, que **refuta directamente** el riesgo hipotético planteado en el
encargo:

**Los escenarios A ("DB reemplazada correctamente pero files falla") y B ("files reemplazado pero
DB falla") no son posibles con el diseño actual**, porque **no hay dos swaps separados**. La
extracción del `.cclinbackup` (`archive::extract_container`) escribe **todas** las entradas —
`manifest.json`, `vault.db`, `vault.meta.json`, y cada `files/<shard>/<uuid>.enc` — dentro de **un
único** directorio de staging (`staging = staging_root.join(Uuid::new_v4())`). Toda la validación
(manifest, hashes/tamaños de **cada** archivo incluyendo los de Documentos, credencial,
compatibilidad de esquema, migración, `foreign_key_check`) ocurre **sobre ese mismo staging**,
antes de tocar el vault activo. La promoción es **un solo** `std::fs::rename(&staging, vault_dir)`
— una operación de sistema de archivos atómica a nivel de SO (POSIX `rename(2)` / Windows
`MoveFileEx`) que mueve DB + meta + `files/` **como una sola unidad**. No existe ningún punto
intermedio observable en el que exista un `vault_dir` con la DB nueva pero los archivos viejos, o
viceversa.

- **Escenario C (crash entre el `rename` de rescate y el `rename` de promoción)**: ya cubierto por
  el mecanismo preexistente de la Fase 10 (`run_startup_recovery`), que opera sobre el directorio
  completo del vault — indiferente a si contiene solo DB+meta (backups anteriores a Fase 16) o
  también `files/` (backups posteriores). Verificado que los tests dedicados de este mecanismo
  (`run_startup_recovery_restores_the_previous_vault_if_interrupted_between_rescue_and_promote`,
  etc.) siguen en verde en la regresión de esta auditoría (§3).
- **Escenario D (disco lleno durante el restore)**: si ocurre durante `extract_container`
  (escritura a `staging`), el error se propaga antes de tocar `vault_dir` en absoluto —
  `StagingGuard` limpia el staging a medio escribir. El `rename` final (staging → vault_dir) es
  una operación de metadata, no de copia de datos, y no requiere espacio adicional en el mismo
  volumen — no puede fallar por disco lleno una vez que la extracción completa ya tuvo éxito.

**Conclusión**: **ROBUSTA**. La aplicación nunca queda en un "vault híbrido" silencioso — ni por
diseño (un solo staging, un solo swap) ni por los mecanismos de recuperación ya existentes de la
Fase 10, que Fase 16 heredó sin necesitar ningún cambio (tal como el informe de Fase 16 ya
afirmaba, y que esta auditoría confirma con evidencia, no por confianza).

## 19. Restore en otra máquina

Revisado (ya documentado desde Fase 10, reconfirmado aquí): un `.cclinbackup` **nunca** incluye el
`refresh_token`/`access_token` de Google (viven exclusivamente en el keychain/Credential Manager
del sistema operativo vía el crate `keyring`, con features `apple-native`/`windows-native`/
`sync-secret-service` — confirmado en `Cargo.toml`). La contraseña maestra tampoco se persiste en
ningún formato. Separación clara y verificada:

- **Datos clínicos** (pacientes, sesiones, documentos, etc.): viajan íntegros dentro del
  `.cclinbackup`, restaurables en cualquier máquina con la contraseña o el código de recuperación
  de ESE backup.
- **Credenciales externas** (tokens de Google): viven en el keychain/Credential Manager del SO de
  origen — nunca se restauran automáticamente en la máquina destino. Tras restaurar en otra
  máquina, reconectar Google Calendar es una acción manual esperada — la UI ya lo advierte
  (confirmado en `docs/backup-restore.md`, sin cambios en esta fase).

## 20. Cambio de contraseña / recovery — impacto sobre documentos

**Verificado por código, no asumido**: `security::vault_manager::change_password` y
`recover_access` **nunca regeneran la DEK del vault** — solo vuelven a envolver la **misma** DEK
con una KEK nueva (derivada de la contraseña/código nuevo). La prueba existe en el propio código
del proyecto: el test `dek_is_unchanged_by_a_password_change` (en `vault_manager.rs`) compara
explícitamente `dek_before.expose_secret()` contra `dek_after.expose_secret()` tras un cambio de
contraseña y confirma que son idénticos.

**Consecuencia directa para Documentos**: como `wrap_file_key`/`unwrap_file_key` usan la DEK del
vault (no una clave derivada de la contraseña), y esa DEK nunca cambia ante un cambio de
contraseña o una recuperación de acceso, **todo `wrapped_file_dek` ya almacenado sigue siendo
válido después de cambiar la contraseña o de recuperar el acceso** — ningún documento se vuelve
ilegible. **VERIFICADO POR CÓDIGO, con evidencia de test ya existente**, no una suposición nueva de
esta auditoría.

## 21. Auto-lock durante operaciones

Se rastreó el código real, operación por operación, en vez de asumir seguridad:

- **Import (`create_document`)**: el flujo hace dos llamadas `session.with_connection` separadas
  (validación al inicio; `INSERT` al final), con la lectura/cifrado/escritura del archivo **entre
  medio**, sin tener el lock continuamente retenido. Si el auto-lock dispara entre la generación de
  la DEK de archivo y `wrap_file_key`, este último devuelve `Err(Locked)` **antes** de escribir
  nada en disco — sin archivo huérfano. Si dispara **después** de escribir el ciphertext pero
  **antes** del `INSERT` final, el código ya tiene un manejo explícito para ese caso exacto:
  ```rust
  Err(_locked) => {
      let _ = std::fs::remove_file(&abs_path);
      Err(DocumentError::VaultLocked)
  }
  ```
  El ciphertext recién escrito se borra — no queda fila sin archivo ni archivo sin fila.
  **VERIFICADO SEGURO POR CÓDIGO**.
- **Lectura (abrir/exportar)**: `get_document_content` hace un `with_connection` (fetch de la fila)
  seguido de `unwrap_file_key` (que revalida el estado por su cuenta). Si el vault se bloquea entre
  ambos, la operación falla limpiamente con `VaultLocked` — no hay nada que revertir en una
  operación de solo lectura. **VERIFICADO SEGURO**.
- **Backup**: un único `with_connection` al inicio (`VACUUM INTO` + lista de paths); el resto de
  la función (copiar archivos, escribir manifest y ZIP) no depende de que el vault siga
  desbloqueado. Un auto-lock a mitad de la copia de archivos no interrumpe ni corrompe nada.
  **VERIFICADO SEGURO**.
- **Restore**: la propia función llama a `session.lock()` deliberadamente como parte de su propio
  flujo, en el punto exacto donde toda la validación ya pasó. Un auto-lock concurrente en ese
  momento es, en el peor caso, redundante (bloquear un vault ya bloqueado es un no-op).
  **VERIFICADO SEGURO**.

**Conclusión general**: el patrón del proyecto de "revalidar el estado de lock en cada llamada" en
vez de sostener un lock continuo durante toda una operación multi-paso, combinado con la limpieza
explícita ya presente en `create_document`, hace que el auto-lock durante cualquiera de estas
operaciones sea seguro por diseño. No se encontró ningún camino hacia una operación parcial o
corrupción por esta causa.

## 22. Migraciones reales — protocolo de prueba diseñado

`SCHEMA_V1` → `SCHEMA_V9` no se editó en ningún punto de la historia del proyecto (verificado por
`git log -p` sobre `db/migrations.rs` a través de las fases — cada `SCHEMA_VN` se agrega como texto
nuevo, nunca se modifica uno existente). `SCHEMA_V9` (Fase 16) es la única migración que reconstruye
una tabla completa (`documents`) en vez de un `ALTER TABLE` aditivo — justificado porque SQLite no
permite modificar un `CHECK` existente, y verificado que preserva datos previos
(`v9_migration_is_idempotent_and_preserves_v1_document_data`).

**Protocolo físico diseñado (no ejecutado en este entorno headless)**:

1. Compilar (o recuperar) un binario de una fase anterior real del proyecto — candidatos válidos:
   el binario correspondiente al commit `2c179ca` (última fase antes de `SCHEMA_V9`) para probar
   V8→V9, o un binario aún más antiguo para probar una cadena más larga.
2. Con ese binario antiguo: crear un vault de prueba, cargar datos **ficticios** representativos de
   cada vertical (paciente, proceso, sesión, objetivo, pago, plan de seguridad, evaluación,
   formulación — sin documentos, porque `documents` no existía activamente antes de Fase 16).
3. Cerrar la aplicación antigua.
4. Instalar/ejecutar el binario **nuevo** (post `d240990`) apuntando al mismo `app_data_dir`.
5. Confirmar que el arranque migra automáticamente sin intervención manual, sin pérdida de ningún
   dato cargado en el paso 2, y que la nueva pestaña "Documentos" aparece funcional inmediatamente.
6. Verificación de integridad: `PRAGMA foreign_key_check` debe devolver cero filas; cada entidad
   cargada en el paso 2 debe ser legible y editable exactamente como antes.
7. Atención especial a `SCHEMA_V9`: confirmar que la tabla `documents` reconstruida no dejó
   ninguna fila huérfana ni perdió ningún dato que pudiera haber existido (en la práctica, no
   había filas reales de `documents` en ningún entorno antes de Fase 16, así que este paso es
   preventivo, no correctivo).

Este protocolo requiere **hardware real o al menos un entorno con binarios de fases anteriores
conservados** — no se ejecutó en esta auditoría. Se incorpora al Pre-V1 Manual Acceptance Test.

## 23-24. macOS y Windows — requisitos reales de distribución

Inspeccionado `tauri.conf.json` y `Cargo.toml`:

```json
"bundle": { "active": true, "targets": "all", "icon": [ ...icon.icns, icon.ico... ] }
```

- **Compila localmente**: sí, confirmado en esta misma sesión (`cargo build --release` limpio) —
  pero **solo se ha compilado y ejecutado en Linux** en todo el historial de este proyecto (el
  entorno de estas sesiones es un contenedor Linux con Xvfb). **Nunca se ha compilado ni ejecutado
  en macOS o Windows reales en ningún punto de la historia documentada del proyecto.**
- **`.icns`/`.ico`**: presentes en el repositorio — requisito cumplido a nivel de asset.
- **Firma y notarización**: `tauri.conf.json` **no tiene** ninguna sección
  `bundle.macOS.signingIdentity`/`bundle.windows.certificateThumbprint` configurada — la firma de
  código para ambas plataformas está **sin configurar**. Sin firma, macOS Gatekeeper bloqueará la
  app con una advertencia severa ("no se puede verificar el desarrollador") y Windows SmartScreen
  hará lo mismo — ambos utilizables por una usuaria técnica que sabe "abrir de todas formas", pero
  **no aptos para distribución profesional** sin más pasos.
- **Distinción explícita "compila" vs. "puede distribuirse profesionalmente"**: compilar y generar
  un `.app`/`.exe` local es una cosa; producir un DMG/instalador firmado y notariado que una
  usuaria no técnica pueda instalar sin advertencias alarmantes es otra, y **no está resuelta**.

### Acceptance test físico — macOS (diseñado)

A. Build local (`cargo tauri build` en un Mac real, arquitectura Apple Silicon y/o Intel según el
   público objetivo) → confirmar que el binario arranca y las verificaciones ya conocidas de
   SQLCipher/keychain funcionan en hardware real (nunca ejercitadas hasta ahora).
B. Empaquetado en `.app` → confirmar que Tauri genera el bundle sin errores.
C. DMG → confirmar creación y montaje correcto.
D. Firma (`codesign`) con un Developer ID real → verificar `codesign --verify --deep --strict`.
E. Notarización (`xcrun notarytool submit` + `stapler`) → confirmar aceptación de Apple y que
   Gatekeeper no bloquea la apertura en una Mac limpia sin el certificado del desarrollador
   instalado.

### Acceptance test físico — Windows (diseñado, equivalente)

A. Build (`cargo tauri build` en Windows real, x64 al menos).
B. Instalador (MSI o NSIS, según configuración de Tauri) → confirmar generación e instalación
   silenciosa/interactiva correctas.
C. WebView2 → confirmar comportamiento en una máquina **sin** WebView2 preinstalado (la mayoría de
   Windows 11 ya lo trae, pero Windows 10 no siempre) — Tauri puede empaquetarlo o requerirlo como
   prerrequisito; **no verificado en esta auditoría, requiere hardware**.
D. SQLCipher — confirmar que el binario vendorizado (`bundled-sqlcipher-vendored-openssl`) enlaza y
   funciona igual que en Linux (no hay razón teórica para que no, pero nunca se ha probado).
E. Credential Manager — confirmar que `keyring` con la feature `windows-native` persiste y
   recupera el token de Google correctamente.
F. Firma (Authenticode) con un certificado real → confirmar que SmartScreen no bloquea.

Ninguno de estos dos protocolos se ejecutó — ambos **requieren hardware real**, ausente en este
entorno.

## 25. Google Calendar real

**Verificado por código** (sección §6 arriba, confirmado con evidencia adicional): la función
`event_payload` en `src-tauri/src/calendar/client.rs` construye el cuerpo enviado a Google con
**exactamente 3 claves**: `start`, `end`, `summary` — y `summary` es la constante compile-time
`EVENT_SUMMARY` (nunca un valor derivado de datos clínicos). Existe un test dedicado y ya presente
en el proyecto, `event_payload_never_contains_anything_beyond_the_generic_summary_and_the_two_timestamps`,
que fija esta garantía como contrato — cualquier cambio futuro que intentara agregar un campo
tendría que romper ese test primero.

**Lo que falta y requiere prueba física obligatoria** (nunca ejercitado de punta a punta contra la
API real de Google en ningún entorno de este proyecto): OAuth PKCE completo con una cuenta de
Google de prueba real, callback, obtención y persistencia del `refresh_token`, comportamiento tras
reiniciar la aplicación, sincronización real de una cita de ida y vuelta, logout, revocación desde
el lado de Google, y una inspección de red (o de los logs de la API de Google, si están
disponibles del lado de esa cuenta de prueba) confirmando que el payload real coincide exactamente
con lo que el test unitario ya garantiza en código.

## 26. Logging global

Auditado **todo** el proyecto, no solo Documentos, con grep exhaustivo:

```
grep -rn "log::info!|log::warn!|log::error!|log::debug!|log::trace!|println!|eprintln!|dbg!" src-tauri/src/ --include=*.rs
  → 1 coincidencia, y es un COMENTARIO (no una llamada real), en db/connection.rs línea 94.

grep -rn "console\.(log|warn|error|debug|info)" src/ --include=*.tsx --include=*.ts
  → 0 coincidencias en todo el frontend.
```

**No existe ninguna llamada real de logging en todo el backend Rust, y ninguna llamada
`console.*` en todo el frontend TypeScript/React.** `tauri-plugin-log` está registrado pero **solo
en builds de debug** (`cfg!(debug_assertions)`) y, dado que ningún código de la aplicación llama
nunca a `log::*`, no tiene contenido de aplicación que emitir — a lo sumo, eventos internos del
propio framework Tauri/WebView (nombres de comandos IPC, ciclo de vida de ventana), nunca
contenido clínico, porque el código de la aplicación simplemente no genera esas llamadas.

**Conclusión**: no hay ningún leak de nombre/RUT/email/teléfono/nota/diagnóstico/ruta/token/clave
posible vía logging, porque estructuralmente no hay logging de contenido de aplicación en ningún
punto del código. **VERIFICADO, no blocker.**

## 27. Crash tests — protocolo diseñado

No ejecutados en este entorno (headless, sin capacidad de simular crashes de proceso de forma
realista sin herramientas adicionales no disponibles). Protocolo con datos ficticios:

1. **Crash durante import**: `kill -9` al proceso entre el `fs::write` del temporal `.enc.tmp` y el
   `rename` final. Resultado esperado: el `.tmp` queda huérfano en `vault/files/<shard>/`, sin fila
   en `documents` (porque el `INSERT` nunca se alcanzó) — `check_document_consistency` debería
   reportarlo como orphan_ciphertext (el nombre no calza con `<uuid>.enc`, así que en rigor ni
   siquiera contaría como tal salvo que se ajuste el reconciliador para reconocer `.tmp`
   huérfanos — **anotar como brecha menor del reconciliador actual**, ver §31). Ningún dato del
   vault existente se pierde.
2. **Crash durante backup**: `kill -9` a mitad de `write_container`. Con el hallazgo BACKUP-1
   (§17) sin corregir, deja un `.cclinbackup` parcial en el destino — mismo resultado que un fallo
   por disco lleno, mismo hallazgo aplicable.
3. **Crash durante restore**: cubierto por el mecanismo ya existente y ya probado
   `run_startup_recovery` (§18) — recuperación automática al reiniciar, verificada con tests.
4. **Crash con temporal abierto**: cubierto en §12/§14 — barrido de arranque, con la salvedad
   multi-instance ya documentada.
5. **Crash durante migration**: `run_migrations` corre cada `M::up(...)` dentro de su propia
   transacción (comportamiento de `rusqlite_migration`); un crash a mitad de una migración debería
   dejar esa transacción sin commit (rollback automático de SQLite al reabrir), y el
   `PRAGMA user_version` seguiría reflejando la última migración completada — la aplicación
   simplemente reintentaría la migración pendiente en el siguiente arranque. Razonamiento teórico
   sólido (comportamiento estándar de SQLite), **no verificado con un crash real inducido**.

## 28. Disk full

Ya analizado en el contexto de cada operación (§17 backup — hallazgo real; §18 restore — seguro;
§10 import — seguro, la escritura del temporal fallaría limpiamente antes del rename). Consolidado:

| Operación | Comportamiento ante disco lleno |
|---|---|
| Import de documento | `fs::write` del temporal falla → error explícito, `rename` nunca se alcanza, sin fila en DB. Seguro. |
| Backup | **Hallazgo BACKUP-1** — puede dejar un `.cclinbackup` parcial en el destino. |
| Restore | Seguro — la extracción a staging falla limpiamente antes de tocar el vault activo; el `rename` final no requiere espacio adicional. |
| Migración | No verificado con un escenario real de disco lleno a mitad de una migración — teóricamente protegido por las transacciones de SQLite, sin confirmación empírica. |

## 29. Corrupción controlada

La mayoría de estos escenarios **ya tienen tests dedicados y verificados en la regresión de esta
auditoría** (§3): ciphertext alterado, ciphertext faltante, `vault.db` alterado, backup truncado,
manifest alterado — todos cubiertos por la suite existente de `backup::service` y
`services::documents`/`services::document_crypto`, y todos fallan explícitamente (nunca inventan
recuperación automática), consistente con la exigencia del encargo. El único caso no cubierto por
un test dedicado explícito es "archivo extra inesperado" dentro de `vault/files/` no referenciado
por ninguna fila — cubierto indirectamente por `check_document_consistency` (lo reportaría como
`orphan_ciphertext_count`), pero sin un test que ejercite específicamente ese escenario end-to-end
contra un backup. **D — POST-V1**: agregar ese test específico es barato y de bajo riesgo, no
bloquea nada por sí solo.

## 30. Pre-V1 Manual Acceptance Test — protocolo único consolidado

Ningún caso de este protocolo se ejecutó en este entorno (WebKitGTK no renderiza en este contenedor
— limitación ya documentada y heredada de fases anteriores). Se consolida aquí, por primera vez,
en un único documento reproducible, todo lo pendiente acumulado a través de las fases más lo nuevo
de esta auditoría:

**AUTH**: crear vault, unlock, lock, auto-lock (verificar temporización real), recovery (código de
recuperación real, fijar nueva contraseña).

**PATIENTS**: crear, editar, archivar, restaurar, región/comuna (catálogo completo).

**AGENDA**: cita nueva, editar, cancelar, bloqueo personal, sincronización real con Google Calendar
(cuenta de prueba — ver §25).

**SESSIONS**: crear, notas, versiones múltiples (confirmar que versiones previas quedan intactas),
historial.

**GOALS**: ciclo completo de estados, indicadores, vínculo con sesiones.

**PAYMENTS**: condonado, atrasado (vencimiento derivado), vínculo con sesión.

**CONTINUITY**: preparación de sesión, tarea entre sesiones, transición de estados.

**TREATMENT EPISODES**: crear, pausar, cerrar, reabrir, reingreso (proceso nuevo sin heredar el
anterior).

**CLOSURE**: cierre con motivo, resolución de sesiones futuras, historial de cierres anulados.

**SAFETY PLAN**: crear, actualizar (nueva versión desde el vigente), contactos.

**ASSESSMENTS**: catálogo de instrumentos, administración, evolución longitudinal.

**FORMULATION**: crear, nueva versión, historial de solo lectura, proceso cerrado bloquea nueva
versión, reingreso sin copia automática.

**DOCUMENTS**: importar PDF ficticio, importar imagen ficticia, previsualizar imagen (modal
in-app), abrir externamente (PDF), exportar copia, archivar, restaurar, proceso cerrado permite
documento nuevo, paciente archivado lo bloquea, bloqueo/desbloqueo con documentos presentes,
reinicio completo de la aplicación con persistencia confirmada, **y el caso nuevo de esta
auditoría: lanzar una segunda instancia mientras la primera tiene un temporal de Documentos
abierto, confirmar si sobrevive o desaparece** (para validar o descartar el hallazgo de §12 en
hardware real).

**BACKUP**: crear, verificar (`inspect_backup`), restaurar, confirmar que documentos (activos y
archivados) quedan incluidos y recuperables — **y el caso nuevo: inducir un fallo a mitad de
`create_backup` (p. ej. llenando el disco deliberadamente en un volumen de prueba) y confirmar si
queda un archivo parcial en el destino**, para validar el hallazgo BACKUP-1 en la práctica antes de
decidir si corregirlo.

**STATISTICS**: distribución geográfica con datos ficticios.

## 31. Privacy Acceptance Test

Protocolo con marcadores sintéticos únicos por sesión de prueba (nunca datos clínicos reales, por
regla 3 de `CLAUDE.md`). Ejemplo de convención ya usada en fases anteriores:
`XYZFASE17PRERC<algo-único>`.

Tras usar la aplicación con ese marcador sembrado en nombre de paciente, notas, descripción de
documento, etc., buscarlo en:

- **Logs**: ya verificado estructuralmente imposible (§26) — no requiere prueba física adicional
  más allá de una confirmación puntual si se desea.
- **Filesystem**: nombres de archivo de `vault/files/*` (deben ser solo UUIDs), `storage_path` en
  texto plano en cualquier archivo fuera de SQLCipher.
- **Temp**: contenido de cualquier temporal de Documentos mientras esté abierto (ESPERADO que el
  marcador aparezca ahí — es plaintext deliberado y temporal, ver §14) y confirmar su ausencia tras
  la limpieza.
- **WebView cache**: ver §13 — requiere hardware real.
- **Nombres de archivo**: en cualquier punto del árbol de `app_data_dir`.
- **Título de ventana**: nunca debe mostrar el marcador.
- **Portapapeles**: tras cualquier acción de copiar/exportar.
- **Google Calendar**: el evento espejo (si se sincroniza una cita del paciente marcador) — el
  marcador **nunca** debe aparecer ahí, ya garantizado estructuralmente por `event_payload` (§25).

**Apariciones ESPERADAS** (no son leaks): dentro de los bytes cifrados de `vault.db`/`.enc` (no
legibles sin la clave), y dentro de un temporal plaintext de Documentos **mientras está
legítimamente abierto** para visualización externa.

**Apariciones que SÍ serían LEAK**: cualquier otra — logs, nombres de archivo físicos, caché de
WebView persistente tras cerrar el documento, Google Calendar, portapapeles sin acción explícita
de la usuaria, temporal que sobrevive tras bloquear/cerrar la aplicación.

## 32. Matriz de validación

| | macOS | Windows |
|---|---|---|
| Clean install | DISEÑADO (§23/§24) | DISEÑADO |
| Create vault | DISEÑADO | DISEÑADO |
| Restart | DISEÑADO | DISEÑADO |
| Unlock | DISEÑADO | DISEÑADO |
| Clinical workflow | DISEÑADO (§30) | DISEÑADO |
| Documents | DISEÑADO (§30, incluye caso multi-instance) | DISEÑADO |
| Backup | DISEÑADO (incluye caso BACKUP-1) | DISEÑADO |
| Restore | DISEÑADO | DISEÑADO |
| OAuth (Google Calendar) | DISEÑADO (§25) | DISEÑADO |
| Update | NO DISEÑADO — no existe updater (§27) | NO DISEÑADO |
| Uninstall/reinstall | NO DISEÑADO — comportamiento esperado sin definir (§28) | NO DISEÑADO |
| Privacy inspection | DISEÑADO (§31) | DISEÑADO |
| Crash recovery | DISEÑADO (§27) | DISEÑADO |

**Ninguna celda tiene estado PROBADO** — este proyecto nunca se ha ejecutado en macOS ni Windows
reales en ningún punto de su historia. Esta es, en términos objetivos, la brecha más ancha hacia
una V1 operacional.

## 33. Criterios objetivos para RC1 Desktop

No se usan porcentajes como único criterio. Un build puede llamarse **RC1 Desktop** cuando, de
forma verificable (no autodeclarada):

1. Todos los hallazgos **A — BLOCKER V1** de esta auditoría están resueltos (no hay ninguno, ver
   §34 — hoy no hay ningún blocker que impida *empezar* el camino a RC1, pero sí hay B's que deben
   resolverse antes de *declarar* RC1).
2. Todos los hallazgos **B — MUST FIX BEFORE RC** están resueltos: REG-1 (test flaky), CRYPTO-1
   (separación de dominio de la DEK), la brecha de `sweep_stale_temp_files` multi-instance (§12),
   y BACKUP-1 (§17).
3. `cargo test --release` en 100% verde, sin ningún test no determinista conocido.
4. El build se ha compilado, instalado y ejecutado **al menos una vez en hardware real** de macOS
   y de Windows, con el Pre-V1 Manual Acceptance Test (§30) ejecutado de punta a punta en ambos, sin
   fallas de tipo BLOCKER/MUST FIX descubiertas en el proceso.
5. El Privacy Acceptance Test (§31) se ejecutó al menos una vez por plataforma sin leaks.
6. El binario está firmado (aunque sea con un certificado de desarrollo/self-signed para pruebas
   internas) — no necesariamente notarizado todavía para RC1.
7. No se requiere updater para RC1 — una build de prueba se redistribuye manualmente entre
   quienes prueban.

## 34. Criterios objetivos para V1 Desktop (uso profesional)

Más exigente que RC1:

1. Todo lo de RC1, más:
2. Firma **y** notarización completas en macOS; firma Authenticode válida en Windows — sin
   advertencias de Gatekeeper/SmartScreen en una instalación limpia.
3. Mecanismo de actualización funcional (§27) — al menos manual-pero-seguro (verificación de
   integridad del instalador nuevo), idealmente automático.
4. El Pre-V1 Manual Acceptance Test (§30) y el Privacy Acceptance Test (§31) ejecutados **más de
   una vez**, por más de una persona si es posible, en ambas plataformas, sin hallazgos nuevos de
   categoría A o B.
5. Comportamiento de desinstalación (§28) decidido explícitamente y documentado (qué pasa con el
   vault, el keychain, los tokens de Google) — no implementado sin aprobación, pero sí **decidido**
   antes de V1, para que la usuaria sepa qué esperar.
6. CRYPTO-1 resuelto (no solo documentado) — para V1 de uso profesional con datos clínicos reales,
   la separación de dominio criptográfico deja de ser "deseable" y pasa a ser exigible.
7. Al menos un ciclo completo de migración real probado en hardware (§22) documentado con
   evidencia (capturas, logs de la propia migración), no solo diseñado.

## 35. Features que pueden esperar a post-V1

Ver tabla completa en §5. Resumen de la preferencia explícita de la usuaria ("no retrasar RC salvo
dependencia funcional real"): **ninguna de las features evaluadas** (Línea temporal, Export
general, Formulación visual, Recordatorios, Biblioteca, Herramientas clínicas) tiene una
dependencia funcional real que bloquee RC1 o V1 — todas son candidatas legítimas a V1.1 o post-V1,
confirmado por la ausencia de cualquier código en el resto del proyecto que las referencie o
dependa de ellas (verificado por grep de sus nombres conceptuales contra todo `src-tauri/src` y
`src/`, sin resultados).

## 36. Export general — evaluación explícita

`Backup ≠ Export` (regla 6 de `CLAUDE.md`) se mantiene intacta: Documentos ya permite "Exportar
copia" **individual** (Fase 16) — una acción consciente, explícita, con advertencia, que saca un
archivo a la vez fuera del cifrado del vault. Un "Export general" (exportar toda la información del
paciente, o de toda la práctica, en un formato abierto) es un concepto distinto y más amplio, sin
ningún diseño previo en ningún documento del proyecto (confirmado — no existe ningún
`Plan-Export-...md` ni sección de `ARCHITECTURE.md` dedicada).

**¿Es blocker de V1?** No se encontró ninguna dependencia funcional que lo exija — ninguna otra
funcionalidad del proyecto necesita que exista un Export general para funcionar correctamente.
**No se asume que sí sea blocker únicamente porque sería valioso.** Clasificación: **RECOMENDABLE
V1.1** (mismo criterio que Línea temporal) — requeriría su propia fase de diseño explícita antes de
empezar (qué formato, qué alcance, si incluye documentos descifrados) dado que es una decisión de
producto y de privacidad no trivial (regla 11 de `CLAUDE.md`: "una funcionalidad que no puede
implementarse de forma segura con la arquitectura actual" exige detenerse — un Export general que
decida sacar documentos descifrados en bloque merece su propia auditoría de diseño, no una
implementación improvisada dentro de otra fase).

## 37. Clasificación consolidada de todos los hallazgos

| # | Hallazgo | Prioridad | Archivos | Migración | Dependencia | Tests |
|---|---|---|---|---|---|---|
| REG-1 | Test flaky `resolve_within_files_root_rejects_an_uppercase_shard` | **B** | `services/document_crypto.rs` (test) | No | No | Ninguno nuevo — fijar el existente |
| CRYPTO-1 | DEK del vault reutilizada sin separación de dominio (SQLCipher + AES-GCM wrap) | **B** | `security/session.rs` | No | Sí (`hkdf`, requiere aprobación) | Roundtrip con subclave + verificación de incompatibilidad con envoltorios antiguos |
| CRYPTO-2 | Ausencia de AAD (informativo, sin riesgo real) | **E** | — | No | No | No |
| CRYPTO-3 | Zeroización no exception-safe de `file_dek` en `create_document` | **C** | `services/documents.rs` | No | No | Test de que `file_dek` se zeroiza incluso si `encrypt_document` falla |
| SYMLINK-1 | `create_backup` sigue symlinks al copiar ciphertexts (`fs::copy`) | **C** | `backup/service.rs` | No | No | Test con un symlink plantado en `vault/files/` antes de respaldar |
| MULTI-1 | `sweep_stale_temp_files` puede borrar un temporal legítimo de otra instancia | **B** | `lib.rs`, `services/document_temp.rs`, `Cargo.toml` (opcional) | No | Opcional (`tauri-plugin-single-instance`, requiere aprobación) | Test de barrido consciente de antigüedad, o E2E de single-instance |
| BACKUP-1 | `create_backup` puede dejar un `.cclinbackup` parcial en el destino final ante un fallo a mitad de escritura | **B** | `backup/archive.rs`, `backup/service.rs` | No | No | Simular fallo a mitad de `write_container`, confirmar que `dest_path` no existe tras el error |
| UPDATER-1 | Sin mecanismo de actualización | **A (para V1, no para RC1)** | — (fase futura) | No | Sí (`tauri-plugin-updater`, requiere aprobación) | — |
| PERM-1 | Sin permisos explícitos restrictivos en `vault/*` | **D** | `security/vault_manager.rs` o `lib.rs` | No | No | — |
| CONSIST-1 | Reconciliador no distingue explícitamente un `.tmp` huérfano de una importación interrumpida | **D** | `services/documents.rs` | No | No | Test de importación interrumpida simulada |
| PLATFORM-1 | Cero ejecución en hardware real macOS/Windows en toda la historia del proyecto | **F — requiere hardware** | — | No | No | Todo §30-§32 |
| CALENDAR-1 | Cero prueba física contra la API real de Google | **F — requiere hardware** | — | No | No | §25 |
| WEBVIEW-1 | Comportamiento real de caché de WKWebView/WebView2 con `data:` URLs no verificado | **F — requiere hardware** | — | No | No | §13 |

## 38. Top 10 pendientes más importantes antes de V1 (por riesgo real, no por atractivo)

1. **BACKUP-1** — un backup puede quedar parcial sin que el sistema lo detecte hasta que ya es
   tarde. Es el hallazgo de mayor riesgo real de esta auditoría porque toca directamente el único
   mecanismo de recuperación ante pérdida total.
2. **MULTI-1** — pérdida de una copia temporal legítima por una acción cotidiana (doble apertura de
   la app), sin necesitar ningún atacante.
3. **CRYPTO-1** — reutilización de la DEK del vault sin separación de dominio. Riesgo real bajo hoy,
   pero es la brecha de disciplina criptográfica más significativa encontrada, y barata de cerrar.
4. **PLATFORM-1** — cero validación física en macOS/Windows en toda la historia del proyecto. No es
   un "bug", pero es la incertidumbre más grande sobre si el producto funciona de verdad fuera de
   este contenedor Linux.
5. **REG-1** — test no determinista en la suite de seguridad de path traversal; mina la confianza
   de la cifra de tests como señal.
6. **CALENDAR-1** — la integración con Google nunca se ejerció contra la API real; el código está
   bien diseñado (verificado), pero "nunca se probó de verdad" es un riesgo operacional real para
   una feature que toca un servicio externo.
7. **UPDATER-1** — sin mecanismo de actualización, cualquier corrección futura (incluidas las de
   esta misma lista) no puede llegar a una instalación ya hecha sin reinstalación manual.
8. **SYMLINK-1** — vector de exfiltración vía backup si un atacante ya tiene escritura local; bajo
   pero real, y barato de cerrar.
9. **WEBVIEW-1** — no se puede afirmar con certeza que el `data:` URL de una imagen no deja rastro
   en la caché del motor de renderizado en hardware real.
10. **CRYPTO-3** — zeroización no exception-safe de una clave de archivo en una ventana muy angosta;
    el de menor riesgo real de los diez, pero completa la lista de higiene criptográfica pendiente.

## 39. Roadmap propuesto hasta V1

No se asume de antemano cuántas fases harán falta — se ordena por dependencia real:

**Fase A — Hardening pre-RC (código, sin hardware)**
- Objetivo: cerrar los cuatro hallazgos **B** (REG-1, CRYPTO-1, MULTI-1, BACKUP-1).
- Alcance: exclusivamente esos cuatro — ninguna feature nueva.
- Requiere código: sí. Requiere hardware real: no.
- Condición de cierre: los cuatro hallazgos resueltos, regresión completa 100% verde y
  determinista (múltiples ejecuciones consecutivas sin flakiness), sin ningún warning nuevo.
- Decisiones que necesitan aprobación previa: si se opta por `tauri-plugin-single-instance` y/o
  `hkdf` como dependencias nuevas (regla 11 de `CLAUDE.md`).

**Fase B — Validación física macOS**
- Objetivo: primera ejecución real del proyecto en macOS en toda su historia.
- Alcance: acceptance test completo de §23, más Pre-V1 Manual Acceptance Test (§30) y Privacy
  Acceptance Test (§31) en macOS.
- Requiere código: probablemente ajustes menores descubiertos en el proceso (no anticipables).
  Requiere hardware real: sí, obligatorio.
- Condición de cierre: matriz de §32 con la columna macOS en estado PROBADO, sin hallazgos A/B
  nuevos.

**Fase C — Validación física Windows**
- Mismo patrón que Fase B, para Windows (§24).
- Condición de cierre: columna Windows de la matriz en PROBADO.

**Fase D — Google Calendar real**
- Objetivo: primera prueba de punta a punta contra la API real de Google.
- Requiere hardware: no necesariamente (puede hacerse en este mismo tipo de entorno con acceso a
  red saliente y una cuenta de prueba), pero si Fases B/C ya están en curso, es más eficiente
  hacerlo junto a ellas.
- Condición de cierre: OAuth, sync, logout, revocación verificados; payload real confirmado
  idéntico al que el test unitario ya garantiza.

**Fase E — Firma, notarización, empaquetado profesional**
- Objetivo: pasar de "compila" a "puede distribuirse profesionalmente" en ambas plataformas.
- Requiere: certificados de desarrollador (macOS y Windows), decisión de negocio sobre quién los
  gestiona — **esto es una decisión que requiere aprobación explícita de la usuaria, no técnica**.
- Condición de cierre: instalación limpia sin advertencias de Gatekeeper/SmartScreen.

**Fase F — Updater**
- Objetivo: mecanismo de actualización, al menos manual-pero-verificado.
- Requiere código y dependencia nueva (`tauri-plugin-updater` u otro) — **requiere aprobación**.
- Condición de cierre: una actualización real, probada en al menos una plataforma, preserva el
  vault existente sin pérdida de datos.

**Punto de decisión — RC1**: alcanzable al cierre de Fase A + al menos una corrida completa de
Fase B o C (no ambas obligatoriamente para RC1, sí para V1).

**Punto de decisión — V1**: exige A, B, C, D y al menos una decisión explícita sobre E (aunque sea
"con certificado de desarrollo por ahora, notarización pública diferida") y sobre F.

## 40. Estimación actualizada

- **Núcleo clínico** (funcionalidad clínica implementada): **completo** — sin brechas conocidas de
  funcionalidad clínica no implementada tras el cierre de Documentos (Fase 16). Ninguna fase nueva
  de funcionalidad clínica es necesaria para V1.
- **V1 clínica** (núcleo + continuidad + pagos + agenda + documentos): **~95%** — los hallazgos
  pendientes son de robustez/hardening (§37), no de funcionalidad faltante.
- **V1 operacional macOS**: **~15-20%** — el código existe y compila en Linux, pero cero validación
  física; toda la Fase B del roadmap está por delante.
- **V1 operacional Windows**: **~15-20%** — mismo razonamiento, toda la Fase C por delante.
- **RC Desktop** (Fase A + al menos una plataforma validada): alcanzable en un número acotado de
  fases bien definidas (A + B **o** C), sin depender de ninguna funcionalidad clínica nueva.
- **Visión multiplataforma futura** (iOS/iPadOS + Sync): sin cambios respecto a estimaciones
  anteriores del proyecto, **~25-30%** — correctamente fuera de alcance de este ciclo, por diseño
  (reglas 6 y 7 de `CLAUDE.md`).

**Explicación de la brecha principal**: el proyecto tiene una base de código e ingeniería de
seguridad notablemente madura para su etapa (cripto revisada con rigor, tests exhaustivos,
disciplina de migraciones), pero **nunca se ha ejecutado fuera de un contenedor Linux headless**.
La brecha no es de funcionalidad — es de **evidencia empírica en las plataformas objetivo reales**.

## 41-46. (Cubiertos arriba: features ausentes §5/§35, Export §36, Top 10 §38, roadmap §39, estimaciones §40)

## 47. Respuesta obligatoria

**"¿Debemos dejar de agregar features ahora y entrar en cierre operacional de V1?"**

# SÍ.

**Justificación**: el núcleo clínico está funcionalmente completo tras el cierre de Documentos —
no hay ninguna vertical clínica pendiente que bloquee nada. La brecha real y medible del proyecto
ya no es "qué le falta construir" sino "qué tan cierto es que lo ya construido funciona fuera de
este contenedor Linux" (cero validación física en macOS/Windows en toda la historia del proyecto)
y "cuán sólidos son los mecanismos de recuperación que la usuaria daría por sentado en una
emergencia real" (el hallazgo BACKUP-1 es precisamente ese tipo de sorpresa que solo aparece
cuando ya es tarde). Agregar una vertical clínica nueva (Línea temporal, Export, Formulación
visual) en este momento no reduce ninguno de esos dos riesgos — solo los pospone. La recomendación
es entrar en el roadmap de §39 (Fase A de hardening, seguida de validación física real en ambas
plataformas) antes de retomar funcionalidad clínica nueva.

## 48. Decisiones que requieren aprobación explícita antes de cualquier implementación

1. Introducir `hkdf` como dependencia nueva (CRYPTO-1) — regla 11 de `CLAUDE.md`.
2. Introducir `tauri-plugin-single-instance` (o decidir la alternativa de barrido consciente de
   antigüedad sin dependencia nueva) para MULTI-1 — regla 11 de `CLAUDE.md`.
3. Estrategia de firma/notarización (Fase E del roadmap) — quién gestiona los certificados, costo,
   cronograma — decisión de negocio, no técnica.
4. Mecanismo de updater a adoptar (Fase F) — dependencia nueva, requiere aprobación.
5. Comportamiento deseado de desinstalación (§28) — qué debe pasar con el vault, el keychain, los
   tokens de Google — decisión de producto que hoy no está definida en ningún documento del
   proyecto.
6. Orden real del roadmap (§39) — específicamente si Fase B (macOS) o Fase C (Windows) va primero,
   según qué hardware esté disponible primero.

## 49. Estado final del árbol de trabajo

```
git status --porcelain=2
→ (sin salida — sin código productivo modificado)
```

Confirmado explícitamente al terminar esta auditoría: **cero cambios de código** en
`src-tauri/src/` ni en `src/` — todo lo relatado en este documento se investigó por lectura de
código, greps y ejecución de comandos de solo lectura (tests, clippy, build, lint), nunca por
edición. El único archivo nuevo de esta sesión es este mismo informe.

---

## Versionado de este informe

Este documento se versiona en un **commit puramente documental**, sin mezclar ningún cambio de
código, siguiendo la política ya establecida del proyecto (informes y auditorías se versionan como
artefactos de trazabilidad). Push normal a `claude/cuaderno-clinico-desktop-udijjq`, sin force
push. El hash final del commit se registra en el mensaje de confirmación entregado junto con este
informe.

---

## STOP

Esta auditoría termina aquí. **No se implementó, ni se implementará hasta recibir aprobación
explícita, ninguno de los hallazgos anteriores.** No se inició hardening, no se tocó macOS, no se
tocó Windows, no se implementó Línea temporal, no se implementó Export, no se implementó ninguna
feature nueva. Se espera aprobación explícita de este informe antes de cualquier cambio posterior.
