# Plan Fase 17 — Hardening Pre-RC Desktop (pendiente de aprobación)

**Este documento es solo auditoría y planificación. No se implementó ningún cambio de código,
test, dependencia, migración ni configuración.** Documento único y autocontenido, pensado para
entregarse completo a un tercero sin reconstruir contexto de mensajes anteriores.

Alcance exclusivo de la fase que este plan describe: los cuatro hallazgos **B — MUST FIX BEFORE
RC** de `Auditoria-Pre-RC-Desktop-post-Fase-16.md` (REG-1, CRYPTO-1, MULTI-1, BACKUP-1). Ninguna
funcionalidad clínica nueva, ninguna de las features explícitamente excluidas por el encargo, y
ninguno de los hallazgos C/D/F se incorpora al alcance de implementación futura salvo que este
mismo plan demuestre que es estrictamente necesario para resolver correctamente uno de los cuatro.

---

## 1. Baseline real

Ejecutado literalmente, sin asumir ningún hash de informes anteriores:

```
git rev-parse HEAD                                          → 07a75c3baed9eb37e638cf7b63c7686b30191076
git branch --show-current                                    → claude/cuaderno-clinico-desktop-udijjq
git status --porcelain=2                                     → (sin salida — árbol de trabajo limpio)
git rev-parse origin/claude/cuaderno-clinico-desktop-udijjq  → 07a75c3baed9eb37e638cf7b63c7686b30191076
```

```
07a75c3 (HEAD -> claude/cuaderno-clinico-desktop-udijjq, origin/...) docs: agregar auditoría pre-RC Desktop post-Fase 16
d240990 docs: agregar informe de cierre de Fase 16
4f7c5a1 Fase 16: documentos y adjuntos clínicos cifrados
2c179ca docs: agregar auditoría y plan pendiente de aprobación de Fase 16
0845654 docs: agregar informe de cierre de Fase 15
...
```

Respuestas a las 7 preguntas del encargo:

1. **HEAD local real**: `07a75c3`.
2. **Rama actual**: `claude/cuaderno-clinico-desktop-udijjq`.
3. **HEAD remoto**: `07a75c3` — idéntico.
4. **Divergencia local/remoto**: ninguna.
5. **¿La Auditoría Pre-RC quedó versionada?** Sí — `07a75c3`, `docs: agregar auditoría pre-RC
   Desktop post-Fase 16`. `git show --stat 07a75c3` confirma **un único archivo**,
   `Auditoria-Pre-RC-Desktop-post-Fase-16.md`, 1183 inserciones, **cero cambios en `src-tauri/` ni
   `src/`** — commit puramente documental.
6. **¿Working tree limpio?** Sí.
7. **¿Código productivo posterior a `4f7c5a1` no declarado?** No. Los dos commits posteriores
   (`d240990`, `07a75c3`) son ambos exclusivamente documentales.

**Conclusión**: no se encontró ningún código productivo inesperado. No fue necesario detenerse.
Baseline real de esta fase: `07a75c3`.

## 2. Regresión inicial real (ejecutada, no asumida)

| Comando | Resultado real |
|---|---|
| `cargo test --release` | **765 passed; 1 FAILED** — mismo resultado que la auditoría anterior |
| `cargo clippy --release --all-targets` | 0 warnings |
| `cargo build --release` | limpio |
| `npm run build` | limpio, sin errores TS |
| `npm run lint` | 25 warnings / 0 errors |
| `git diff --check` | limpio |

## 3. Confirmación o refutación de REG-1

**Reconfirmado contra el código actual**, sin modificar nada. Se reejecutó el test aislado 6 veces
adicionales en esta sesión: 4 `ok`, 2 `FAILED` — mismo patrón probabilístico que documentó la
auditoría anterior. Una de las ejecuciones fallidas capturó evidencia directa:

```
thread '...resolve_within_files_root_rejects_an_uppercase_shard' panicked at
src/services/document_crypto.rs:429:57:
called `Result::unwrap_err()` on an `Ok` value:
"/vault/files-root/73/7386bf37-dced-4e35-b706-a76771eda3e0.enc"
```

El shard capturado es `73` — dos dígitos, cero letras — confirmando **exactamente** el mecanismo
diagnosticado: `"73".to_uppercase() == "73"`, el "path inválido" que el test construye es en
realidad válido, y `resolve_within_files_root` lo acepta correctamente. **El diagnóstico de la
auditoría anterior sigue siendo válido palabra por palabra.** No hay ningún bypass de seguridad en
la función productiva — el defecto es exclusivamente de la construcción de datos del test.

## 4. Plan recomendado para REG-1

**Corrección mínima, sin tocar código productivo:**

Reemplazar el UUID aleatorio del test por un UUID **fijo y literal**, elegido deliberadamente para
que sus dos primeros caracteres sean una letra hexadecimal (nunca ambos dígitos), eliminando la
dependencia de `Uuid::new_v4()` en este test específico — sin afectar ningún otro test que sí deba
seguir usando aleatoriedad real (como `resolve_within_files_root_rejects_shard_mismatch_with_the_uuid_itself`,
que verifica un caso distinto y donde la aleatoriedad del UUID no compromete el resultado esperado).

```rust
#[test]
fn resolve_within_files_root_rejects_an_uppercase_shard() {
    let root = Path::new("/vault/files-root");
    // UUID fijo y determinista — deliberadamente elegido con una letra hex en el shard
    // (nunca dos dígitos) para que "poner en mayúsculas" produzca siempre un valor
    // genuinamente distinto del shard válido en minúsculas. Ver Plan-Fase-17 §3-4 para
    // el diagnóstico completo de por qué un UUID aleatorio hacía este test no determinista.
    let id: Uuid = "ab3f1234-dced-4e35-b706-a76771eda3e0".parse().unwrap();
    let shard_upper = id.to_string()[0..2].to_uppercase(); // "AB"
    let bad = format!("files/{shard_upper}/{id}.enc");
    let err = resolve_within_files_root(root, &bad).unwrap_err();
    assert!(matches!(err, StoragePathError::InvalidFormat));
}
```

- **Preserva exactamente el propósito original**: sigue comprobando que un shard con letras hex en
  mayúscula se rechaza — no se relaja ni se elimina ninguna aserción.
- **No se modifica `resolve_within_files_root`** — no existe ningún defecto productivo que
  corregir; modificarla sería una regresión de alcance no autorizada.
- **Alternativa descartada**: generar en un bucle hasta obtener un shard con letra
  (`loop { let id = Uuid::new_v4(); if !shard_is_all_digits { break; } }`) — funcionalmente
  correcta pero innecesariamente más compleja que un literal fijo para un caso que no necesita
  variar entre ejecuciones. Se prefiere el literal por simplicidad y legibilidad.
- **Demostración de ausencia de flakiness**: ejecutar el test aislado repetidamente
  (`cargo test --release resolve_within_files_root_rejects_an_uppercase_shard -- --exact`) al menos
  20 veces consecutivas, todas `ok`, más una ejecución completa de `cargo test --release` (766/766
  esperado) al menos 3 veces consecutivas antes de dar por cerrado el hallazgo.
- **Archivo único**: `src-tauri/src/services/document_crypto.rs` (módulo `#[cfg(test)]`
  únicamente).
- **Sin dependencia, sin migración, sin test nuevo** — se corrige el test existente in situ.

## 5. Confirmación o refutación de CRYPTO-1

**Confirmado por lectura directa del código actual** (sin cambios desde la auditoría anterior):

- `src-tauri/src/db/connection.rs::open_vault`: `PRAGMA key = "x'<64 hex>'"` — la DEK del vault
  (32 bytes) se entrega a SQLCipher como raw key.
- `src-tauri/src/security/session.rs::wrap_file_key`/`unwrap_file_key`: usan
  `session.dek.expose_secret()` **directamente** como clave `Aes256Gcm` para envolver/desenvolver
  las DEKs de archivo de Documentos — el mismo material de 32 bytes que SQLCipher usa como raw key.
- `src-tauri/src/security/envelope.rs::wrap_dek`/`unwrap_dek`: uso **distinto y no problemático** —
  aquí la DEK es el *plaintext* que se envuelve con una KEK derivada de la contraseña/código de
  recuperación vía Argon2id (`security/kdf.rs`). No hay solapamiento de key-material con el
  hallazgo.
- La DEK cruda del vault **no cruza** la frontera de `security::session` hacia
  `services::document_crypto` — confirmado leyendo ambos métodos completos: solo se le pide a la
  sesión "envuelve esta clave" / "desenvuelve este envoltorio", nunca "dame la DEK".
- Las DEKs de archivo son aleatorias e independientes por documento (`generate_file_dek`,
  `getrandom`) — este punto está bien y no se toca.
- `documents.wrapped_file_dek`/`wrap_nonce` almacenan la DEK de archivo envuelta según el esquema
  de Fase 16 — sin ninguna columna de versión de wrapping todavía.

**Confirmado: no existe hoy separación de dominio criptográfico entre "clave raw de SQLCipher" y
"clave de envoltura de DEKs de archivo".** El hallazgo de la auditoría es válido.

## 6. Análisis criptográfico de separación de dominio

El problema de ingeniería es preciso: dos consumidores criptográficos distintos (SQLCipher como
cifrador de páginas de base de datos; AES-256-GCM como envoltura de claves de archivo) reciben
**el mismo material de bits** sin ninguna derivación que los distinga. La solución conceptualmente
correcta es una función de derivación de claves con separación de dominio — HKDF es el estándar de
facto para "tengo una clave/secreto de alta entropía y necesito derivar una o más subclaves
independientes para propósitos distintos, sin debilitar ninguna" (RFC 5869).

**Por qué HKDF y no otra cosa**:
- El material de entrada (la DEK del vault) ya es una clave criptográfica de 256 bits de entropía
  completa (generada por `getrandom`, nunca una contraseña de baja entropía) — es exactamente el
  caso de uso para el que HKDF fue diseñado (a diferencia de Argon2id, que existe para el caso
  contrario: derivar una clave a partir de una contraseña de *baja* entropía, con costo
  computacional deliberado). Usar Argon2id aquí sería incorrecto y derrocharía CPU sin ganancia de
  seguridad.
- HKDF permite un parámetro `info` (cadena de contexto) que provee la separación de dominio en sí
  misma: `HKDF-Expand(PRK, info="cuaderno-clinico:file-key-wrap:v1", L=32)` produce una subclave
  que es criptográficamente independiente de cualquier otra subclave derivada con un `info`
  distinto de la misma DEK — exactamente la propiedad que falta hoy.
- El `salt` de HKDF-Extract puede omitirse (`None`) de forma segura cuando el material de entrada
  ya es una clave de alta entropía uniformemente aleatoria (no una contraseña) — es el uso estándar
  documentado del propio RFC 5869 y de implementaciones de referencia (p. ej. Noise Protocol,
  WireGuard) en este mismo escenario.

**Zeroización de la subclave derivada**: el buffer de 32 bytes que produce `Hkdf::expand(...)`
debe tratarse con la misma higiene que cualquier otro material de clave del proyecto — zeroizado
inmediatamente después de construir el `Aes256Gcm` a partir de él (mismo patrón ya usado para el
`plaintext` intermedio de `unwrap_file_key` en Fase 16).

**Contexto y versionado exactos propuestos**:
- Cadena `info` de HKDF: `b"cuaderno-clinico:file-key-wrap:v1"` — versiona el **esquema de
  derivación en sí** (si en el futuro cambiara el algoritmo de derivación sin cambiar el propósito,
  se bump a `v2`, `v3`, etc., en esta cadena).
- Esto es **distinto y no debe confundirse** con la columna de esquema `key_wrap_version` en la
  base de datos (§10), que versiona **qué método de wrapping usa cada documento** (legado sin HKDF
  vs. nuevo con HKDF) — dos ejes de versionado independientes, cada uno con su propio propósito.

## 7. Comparación de estrategias de compatibilidad/migración legacy

**Restricción arquitectónica que descarta ejecutar esto como una migración SQL pura**: las
migraciones de esquema (`db::run_migrations`) corren sobre una `Connection` cruda, **sin acceso a
ningún secreto criptográfico** — ni la DEK del vault ni ninguna sesión de `security::` existen
todavía en ese punto del arranque (`run_migrations` se llama dentro de `open_vault`, antes de que
exista cualquier `VaultSession`). Esto es una separación de capas deliberada del proyecto (la capa
de esquema no conoce material criptográfico) y **no debe romperse** para esta fase. Por lo tanto,
cualquier alternativa que requiera desenvolver/reenvolver DEKs de archivo reales solo puede
ejecutarse en la capa de **servicio**, con el vault ya desbloqueado — nunca como parte de
`SCHEMA_VN`.

### Alternativa A — Migración eager (al desbloquear, o mediante acción explícita)

Recorrer todos los documentos tras el primer desbloqueo posterior a la actualización: para cada
fila, desenvolver con el método legado (DEK del vault directa), volver a envolver con la subclave
HKDF, actualizar `wrapped_file_dek`/`wrap_nonce`/`key_wrap_version` en una única sentencia `UPDATE`
por documento (atómica por fila gracias al propio motor SQLite — o bien, sin fila, o bien las tres
columnas actualizadas juntas).

| Criterio | Evaluación |
|---|---|
| Seguridad | Máxima — cero documentos permanecen en el esquema débil una vez completada. |
| Compatibilidad hacia atrás | Requiere lógica de lectura dual **solo durante la ventana de migración** (que puede ser larga si hay muchos documentos o si el proceso se interrumpe repetidamente). |
| Riesgo de pérdida de datos | Bajo si cada `UPDATE` es atómico por fila (lo es) — pero el conjunto de documentos puede quedar en un estado mixto v1/v2 indefinidamente si el proceso se interrumpe repetidamente sin completar. |
| Atomicidad | Por fila, sí. Del conjunto completo, no — no hay (ni debería inventarse) una transacción que abarque cientos de operaciones de cifrado. |
| Comportamiento ante crash | Reanudable: al reiniciar, se reintenta con los documentos que sigan en v1. Requiere que el código de lectura soporte AMBAS versiones de todas formas, indefinidamente, porque nunca se puede garantizar que la migración termine. |
| Comportamiento ante auto-lock | Si el auto-lock dispara a mitad del recorrido, la migración se detiene limpiamente (sin DEK, no puede continuar) y se reanuda en el siguiente desbloqueo — no hay corrupción, pero sí una necesidad de un "runner" que se pueda interrumpir y reanudar de forma segura, complejidad adicional no trivial. |
| Impacto sobre backup/restore | Un backup tomado a mitad de la migración captura una mezcla v1/v2 — debe seguir siendo restaurable y legible (exige que el código YA soporte ambas versiones, lo cual quita valor a la premisa de "eager" como forma de simplificar el resto del sistema). |
| Cambio de contraseña/recovery | Sin impacto directo — la DEK del vault no cambia con estas operaciones (§20 de la auditoría, reconfirmado). |
| Migraciones | Ninguna de esquema más allá de la columna `key_wrap_version` — la migración de *datos* es lógica de aplicación, no `SCHEMA_VN`. |
| Complejidad | **Alta** — requiere un "runner" de migración con progreso, reanudación, posiblemente una UI de progreso o al menos telemetría interna, y coexistencia con auto-lock/multi-instance durante la ventana. |
| Deuda técnica | Baja a largo plazo (una vez migrados todos, se podría eventualmente retirar el soporte legacy) pero alta a corto plazo por la maquinaria de migración en sí. |
| Facilidad de test | Media-baja — hay que simular interrupciones a mitad de recorrido, reanudación, idempotencia. |
| Rollback | No hay downgrade real — una vez reenvuelto con HKDF, no tiene sentido volver atrás salvo revertir la columna, lo cual no revierte el wrapping ya hecho (iría contra el criterio de "nunca degradar seguridad ya aplicada"). |
| Backup antiguo restaurado después de introducir el esquema nuevo | Sus documentos llegan en v1 — la migración eager los recogería en el siguiente ciclo, pero mientras tanto deben ser legibles igual (again, exige soporte dual de todas formas). |

### Alternativa B — Migración lazy (al acceder)

Al abrir/leer un documento v1, además de descifrarlo con el método legado, aprovechar la sesión ya
desbloqueada para reenvolverlo inmediatamente con HKDF y persistir el cambio antes de devolver el
resultado.

| Criterio | Evaluación |
|---|---|
| Seguridad | Migra solo lo que efectivamente se usa — documentos nunca vueltos a abrir permanecen en v1 indefinidamente (peor que A en cobertura final). |
| Compatibilidad hacia atrás | Igual que A: requiere soporte dual indefinidamente, porque nunca hay garantía de que todo termine migrado. |
| Riesgo de pérdida de datos | Bajo, mismo razonamiento por fila que A. |
| Atomicidad | Por fila, sí — pero ahora la operación de "lectura" deja de ser efectos-libres: `get_document_content` (nominalmente un accesor de solo lectura) pasa a escribir en la base de datos como efecto secundario. Esto es una sorpresa de diseño — mezcla una operación de lectura con una de escritura oculta, contrario al principio de menor sorpresa y a la separación de responsabilidades ya usada en el resto del proyecto (`repositories::documents` es SQL puro, `services::documents` separa explícitamente lectura de escritura en funciones distintas). |
| Comportamiento ante crash | Si el proceso muere justo después de descifrar pero antes/durante el `UPDATE` de reenvoltura, el documento simplemente permanece en v1 — no hay corrupción, pero tampoco progreso garantizado. |
| Comportamiento ante auto-lock | Ninguna interacción especial — la operación ya requiere el vault desbloqueado para la lectura en sí. |
| Impacto sobre backup/restore | Mismo problema que A: mezcla v1/v2 persistente. |
| Cambio de contraseña/recovery | Sin impacto. |
| Migraciones | Igual que A. |
| Complejidad | Media — más simple que A (no requiere un "runner" separado), pero introduce el problema conceptual de una lectura con efecto secundario de escritura, que complica el razonamiento sobre concurrencia/multi-instance (¿qué pasa si dos instancias leen y reenvuelven el mismo documento casi simultáneamente? Ver §12). |
| Deuda técnica | Persiste indefinidamente — documentos nunca abiertos nunca migran. |
| Facilidad de test | Media — hay que probar que abrir un documento legado lo migra, y que abrirlo de nuevo no lo vuelve a migrar innecesariamente (idempotencia), y el caso de dos instancias compitiendo. |
| Rollback | Mismo razonamiento que A. |
| Backup antiguo restaurado | Mismo razonamiento que A — además, si nunca se abren esos documentos tras restaurar, quedan en v1 para siempre. |

### Alternativa C — Compatibilidad dual permanente y versionada (recomendada)

Agregar `key_wrap_version` a `documents` (§10). `wrap_file_key` (renombrado conceptualmente, ver
§8) siempre envuelve DEKs de archivo **nuevas** con HKDF (v2). `unwrap_file_key` recibe la versión
de la fila y despacha al método correspondiente: v1 usa la DEK del vault directamente (preserva
Fase 16 byte por byte, para siempre); v2 usa la subclave HKDF. **Ninguna reenvoltura ocurre nunca
de forma automática** — los documentos de Fase 16 permanecen en v1 indefinidamente, a menos que en
una fase futura separada se decida ofrecer una herramienta explícita de remigración opcional (fuera
del alcance de Fase 17).

| Criterio | Evaluación |
|---|---|
| Seguridad | Documentos nuevos (post-Fase-17) obtienen la separación de dominio completa. Documentos legado retienen el riesgo ya calificado como bajo por la auditoría (§CRYPTO-1: "no existe un ataque conocido y demostrado") — riesgo residual aceptado explícitamente, no oculto. |
| Compatibilidad hacia atrás | **Total y permanente por diseño** — nunca hay una ventana de migración que pueda quedar a medias, porque no hay migración de datos en absoluto. |
| Riesgo de pérdida de datos | **Mínimo de las tres alternativas** — no hay ninguna operación de reenvoltura que pueda fallar a mitad de camino sobre datos ya existentes. |
| Atomicidad | Trivial — cada documento, desde el momento en que se crea, tiene una versión fija que nunca cambia. |
| Comportamiento ante crash | Sin ninguna ventana de migración, no hay nada que un crash pueda dejar a medias respecto a este hallazgo. |
| Comportamiento ante auto-lock | Sin interacción — el auto-lock nunca interrumpe una "migración" porque no existe. |
| Impacto sobre backup/restore | **El más simple de razonar**: cada fila es autodescriptiva (`key_wrap_version` viaja con la fila dentro de `vault.db`, que a su vez viaja íntegro dentro de `.cclinbackup` vía `VACUUM INTO`, sin cambios adicionales necesarios en `backup::service`). Un backup antiguo de Fase 16, restaurado después de introducir este esquema, contiene documentos con `key_wrap_version = 1` (o, si la columna no existía aún en ese backup, la migración aditiva de esquema post-restore la agrega con su `DEFAULT 1` — ver §10) — se abren con el método legado sin ninguna intervención especial. |
| Cambio de contraseña/recovery | Sin impacto — ninguna de las dos operaciones toca `wrapped_file_dek` de ningún documento, en ninguna versión. |
| Migraciones | Una sola migración aditiva y simple (`SCHEMA_V10`, §10) — sin reconstrucción de tabla. |
| Complejidad | **La menor de las tres** — un `match key_wrap_version { 1 => ..., 2 => ... }` dentro de una sola función, sin ningún proceso en segundo plano, sin runner, sin estado de progreso. |
| Deuda técnica | Documentada explícitamente y acotada: "los documentos de Fase 16 seguirán en el esquema legado hasta que se apruebe una migración explícita futura" — deuda consciente y visible, no oculta. |
| Facilidad de test | **La mayor de las tres** — cada rama (v1, v2) es una función pura testeable de forma aislada y determinista, sin necesidad de simular interrupciones de un proceso en segundo plano. |
| Rollback | No aplica en el sentido de "deshacer" — pero tampoco hace falta, porque nunca se modifica un documento existente. Instalar una versión anterior de la aplicación seguiría pudiendo leer documentos v1 sin problema (son bit-idénticos a Fase 16); no podría leer documentos v2 nuevos, que es un límite esperable y razonable de cualquier evolución de formato hacia adelante. |
| Backup antiguo restaurado | Ya cubierto arriba — es el caso mejor manejado de las tres alternativas porque no depende de que ninguna migración de datos haya podido o no completarse antes del backup. |

### Alternativa D — ¿existe algo mejor?

Se evaluó explícitamente si existe una alternativa que combine lo mejor de A/B (documentos legado
eventualmente actualizados) sin la complejidad de una migración en segundo plano: **una
herramienta de remigración explícita, iniciada conscientemente por la usuaria** (p. ej. un botón
futuro "Actualizar cifrado de documentos antiguos" en Ajustes), en vez de un proceso automático
silencioso. Esto captura el valor de seguridad de A sin los riesgos de un proceso en segundo plano
no solicitado, pero **es, en esencia, la Alternativa A con un disparador manual en vez de
automático** — misma complejidad de implementación, mismo análisis de atomicidad/crash/auto-lock.
Se documenta aquí como la extensión natural de C hacia el futuro, **explícitamente fuera del
alcance de Fase 17** (el encargo pide resolver el hallazgo B, no maximizar la cobertura de
seguridad de documentos legado más allá de lo que el hallazgo exige).

## 8. Recomendación concreta para CRYPTO-1

**Se recomienda la Alternativa C** (compatibilidad dual permanente y versionada), por ser la que
mejor satisface, simultáneamente, las diez prioridades no negociables del encargo (§4 de este
plan) — en particular "no perder datos", "no volver ilegibles datos ya existentes" y "no debilitar
criptografía existente" — con la menor complejidad y la menor superficie de riesgo nueva.

**Diseño concreto**:

- `security/session.rs` (único archivo tocado dentro de `security/*`, mismo alcance ya autorizado
  en Fase 16):
  - Nueva constante `pub const CURRENT_KEY_WRAP_VERSION: u8 = 2;` y `pub const LEGACY_KEY_WRAP_VERSION: u8 = 1;` (o un enum `pub enum KeyWrapVersion { Legacy = 1, DomainSeparated = 2 }` — a decidir en el detalle de implementación, ambas opciones son válidas).
  - `wrap_file_key` **siempre** produce envoltorios v2 (usa la subclave HKDF) — nunca se vuelve a
    escribir un envoltorio v1 desde este punto en adelante.
  - `unwrap_file_key(&self, wrapped: &WrappedFileKey, version: KeyWrapVersion) -> Result<FileKey, UnwrapFileKeyError>`
    — firma con un parámetro nuevo que indica qué método usar: `Legacy` usa la DEK del vault
    directamente (código ya existente de Fase 16, sin cambios); `DomainSeparated` deriva la
    subclave HKDF antes de desenvolver.
  - Subclave derivada mediante `Hkdf::<Sha256>::new(None, session.dek.expose_secret())`, luego
    `.expand(b"cuaderno-clinico:file-key-wrap:v1", &mut okm)`, zeroizando `okm` inmediatamente
    después de construir el `Aes256Gcm`.
  - La DEK cruda del vault sigue sin exponerse fuera de este archivo — no se agrega
    `get_vault_dek()` ni equivalente.
- `services::documents.rs`: al crear un documento nuevo, persistir `key_wrap_version =
  CURRENT_KEY_WRAP_VERSION` (2). Al leer (`get_document_content`), leer `key_wrap_version` de la
  fila y pasarlo a `unwrap_file_key`.
- `repositories::documents.rs`: agregar la columna a `Document`/`NewDocumentRow` y a las consultas
  `SELECT`/`INSERT` correspondientes.

## 9. Dependencia propuesta para CRYPTO-1 — HKDF

**DEPENDENCIA NUEVA — REQUIERE APROBACIÓN.**

1. **Crate exacto**: `hkdf`.
2. **Versión propuesta**: `0.12` (última serie estable de la familia RustCrypto al momento de este
   plan; se fijará la versión exacta disponible en el índice de crates.io al momento de la
   implementación real, siguiendo el mismo criterio de pineo exacto ya usado para
   `aes-gcm = "0.11.1"`/`argon2 = "0.6.0"`).
3. **Ecosistema/mantenedor**: RustCrypto (`RustCrypto/KDFs`) — el mismo grupo que mantiene
   `aes-gcm`, `sha2`, `argon2`, `zeroize`, todos ya presentes y ya confiados por el proyecto. No es
   una organización nueva a evaluar — es una extensión directa de una confianza ya otorgada.
4. **Compatibilidad**: `hkdf` depende de `hmac` y del trait `digest::Digest` — ambos ya forman
   parte del árbol de dependencias transitivas del proyecto hoy (vía `sha2`/`aes-gcm`), así que el
   impacto real en el árbol de compilación es marginal (posiblemente cero crates nuevos más allá de
   `hkdf` mismo, si `hmac` ya está presente transitivamente — a confirmar con `cargo tree` en el
   momento de la implementación).
5. **Por qué es necesaria**: CLAUDE.md exige "sin criptografía propia: solo librerías consolidadas
   (RustCrypto, etc.)" — implementar HKDF a mano (aunque el algoritmo es simple) sería exactamente
   el tipo de criptografía casera que la regla prohíbe. `hkdf` es la implementación canónica de
   referencia de RFC 5869 dentro de la misma familia de crates ya adoptada.
6. **Alternativas sin dependencia nueva evaluadas y descartadas**:
   - Derivar la subclave con `Sha256::digest(vault_dek || contexto)` (concatenación simple) — **no
     es HKDF**, carece de sus garantías formales de independencia entre subclaves derivadas con
     distinto contexto, y construir esa combinación a mano es, otra vez, criptografía propia no
     auditada — descartado explícitamente.
   - Usar Argon2id (ya presente) para esta derivación — semánticamente incorrecto (Argon2id existe
     para material de baja entropía con costo computacional deliberado; aplicarlo a una clave ya
     aleatoria de 256 bits no aporta seguridad y sí latencia innecesaria en cada apertura de
     documento) — descartado.
7. **Riesgo de supply chain / mantenimiento**: bajo — mismo perfil de riesgo que las dependencias
   RustCrypto ya presentes (organización con historial largo, sin incidentes de seguridad
   conocidos, cero dependencias de red/build scripts sospechosos, licencia MIT/Apache-2.0 dual
   igual que el resto del ecosistema RustCrypto ya usado).
8. **Features necesarias**: ninguna feature opcional — uso de la API por defecto (`Hkdf::<Sha256>`).
9. **Impacto en binario**: despreciable — el crate es de pocas líneas, sin `unsafe`, sin
   dependencias de sistema.
10. **Archivos que cambiarían**: `src-tauri/Cargo.toml` (una línea nueva), `src-tauri/Cargo.lock`
    (regenerado por `cargo build`), `src-tauri/src/security/session.rs` (uso).

**No se ejecutó `cargo add` ni se modificó `Cargo.toml`/`Cargo.lock` en esta sesión.**

## 10. Schema/migración propuesta para CRYPTO-1

**`SCHEMA_V10`** (nueva, aditiva — `SCHEMA_V1`–`V9` sin cambios):

```sql
ALTER TABLE documents ADD COLUMN key_wrap_version INTEGER NOT NULL DEFAULT 1
  CHECK (key_wrap_version IN (1, 2));
```

- **Por qué es necesaria**: para que cada documento sea autodescriptivo de qué método de
  envoltura usar al desenvolver su DEK de archivo — sin esto, `unwrap_file_key` no sabría qué rama
  aplicar.
- **Qué representa exactamente**: `1` = envoltura legada de Fase 16 (DEK del vault usada
  directamente como clave AES-256-GCM); `2` = envoltura con separación de dominio vía HKDF (Fase
  17 en adelante).
- **Valores posibles**: exactamente `1` o `2`, forzado por `CHECK` — ningún otro valor es válido,
  evitando que una fila corrupta o mal escrita produzca un valor ambiguo.
- **Valor para filas existentes de Fase 16**: `1` (vía `DEFAULT 1`, aplicado automáticamente por
  SQLite a todas las filas ya existentes en el momento de la migración — sin necesidad de un
  `UPDATE` explícito).
- **Valor para documentos nuevos**: `2`, escrito explícitamente por `repositories::documents::insert_document`
  en el momento de la creación — nunca depende del `DEFAULT` de la columna para las filas nuevas
  (mismo criterio que otras columnas del proyecto donde el default de SQL es solo la red de
  seguridad para filas legado, no la fuente de verdad para filas nuevas).
- **Por qué NO se reutiliza `documents.format_version`**: esa columna representa, desde Fase 16, la
  versión del **formato físico del ciphertext** (`CCD1`, header + nonce + tag) — un concepto
  completamente distinto de "qué clave se usó para envolver la DEK de ese archivo". Ambos podrían
  evolucionar de forma independiente en el futuro (p. ej. `format_version = 2` con streaming, sin
  que eso implique nada sobre el wrapping, o viceversa) — mezclarlos en una sola columna volvería
  ambiguo el significado de ambas y violaría el principio ya aplicado explícitamente en Fase 16
  respecto de `sha256_plaintext`: "el nombre de una columna debe conservar su significado". Se
  descarta explícitamente esta opción.
- **¿Por qué no un `ALTER TABLE` requiere reconstrucción de tabla, como `SCHEMA_V9`?** A diferencia
  de `SCHEMA_V9` (que modificaba el `CHECK` de una columna **ya existente**, `category`, algo que
  SQLite no permite vía `ALTER TABLE`), `SCHEMA_V10` **agrega una columna nueva con su propio
  `CHECK` nuevo** — SQLite sí permite `ALTER TABLE ADD COLUMN` con `CHECK`/`DEFAULT` propios de esa
  misma columna nueva, siempre que el valor por defecto sea una constante (lo es: `1`). No se
  requiere ninguna reconstrucción de tabla.
- **Cómo se migra una DB V9 existente**: la migración corre una vez, aditivamente, sobre la tabla
  `documents` ya existente — todas las filas actuales (con datos reales o de prueba) reciben
  `key_wrap_version = 1` automáticamente, preservando el resto de sus columnas sin cambios.
- **Cómo se comporta una DB recién creada (V1→última)**: `rusqlite_migration` aplica la cadena
  completa en orden (`V1` → … → `V9` crea `documents` con la forma de Fase 16 → `V10` le agrega
  `key_wrap_version`) — el resultado final es idéntico, sea que la base exista desde antes o se
  cree desde cero, mismo criterio ya verificado para todas las migraciones anteriores del
  proyecto.
- **Idempotencia**: `rusqlite_migration` ya garantiza que cada migración se aplica exactamente una
  vez (registrada en `PRAGMA user_version`) — sin comportamiento nuevo que verificar más allá de
  los tests estándar ya usados para cada `SCHEMA_VN` anterior.
- **Compatibilidad con backups antiguos**: un backup de Fase 16 restaurado en una instalación con
  `SCHEMA_V10` ya aplicada pasa por `db::run_migrations` durante el propio flujo de `restore_backup`
  (paso ya existente, sin cambios) — su `vault.db` (en `SCHEMA_V9`) se migra a `V10` como parte de
  esa restauración, y sus documentos reciben `key_wrap_version = 1` automáticamente, exactamente
  como se espera.
- **Tests de migración a diseñar**: columnas presentes desde el arranque; `key_wrap_version`
  default en `1` para una fila creada antes de `V10`; idempotencia y preservación de datos
  anteriores a `V10` (mismo patrón que los tests ya existentes para `V9`); un documento nuevo
  insertado después de `V10` puede fijar `key_wrap_version = 2` explícitamente.

## 11. Tests de CRYPTO-1

Todos con datos ficticios, sin datos clínicos reales, cubriendo exactamente los 18 casos pedidos:

| # | Test | Capa |
|---|---|---|
| 1 | Roundtrip de una DEK nueva usando la subclave HKDF (v2) | `security::session` |
| 2 | Dos DEKs de archivo distintas producen envoltorios v2 independientes | `security::session` |
| 3 | Nonces de envoltura siguen siendo independientes tras el cambio | `security::session` |
| 4 | Vault bloqueado rechaza `wrap_file_key` | `security::session` |
| 5 | Vault bloqueado rechaza `unwrap_file_key` (ambas versiones) | `security::session` |
| 6 | Nonce de envoltura manipulado falla la autenticación (v2) | `security::session` |
| 7 | `wrapped_file_dek` manipulado falla la autenticación (v2) | `security::session` |
| 8 | DEK del vault incorrecta falla el desenvolvimiento (v2) | `security::session` |
| 9 | La subclave derivada se zeroiza (verificación estructural análoga a los tests ya existentes de `FileKey`/`VaultKey`) | `security::session` |
| 10 | **Compatibilidad legacy**: un `WrappedFileKey` fijo, generado una única vez con el algoritmo exacto de Fase 16 (valores hardcodeados como fixture permanente), se desenvuelve correctamente con `unwrap_file_key(..., KeyWrapVersion::Legacy)` | `security::session` |
| 11 | Un documento con `key_wrap_version = 1` (creado antes de esta fase, simulado en el test) se abre correctamente tras "actualizar" el código | `services::documents` |
| 12 | Un documento nuevo (creado después de esta fase) usa `key_wrap_version = 2` y se abre correctamente | `services::documents` |
| 13 | Cambio de contraseña no afecta la legibilidad de un documento v1 ni de uno v2 | `services::documents` (integración con `vault_manager`) |
| 14 | Recovery no afecta la legibilidad de un documento v1 ni de uno v2 | ídem |
| 15 | Backup que incluye documentos v1 y v2 mezclados se restaura y ambos siguen siendo legibles | `backup::service` |
| 16 | Un backup "antiguo" (simulado con `key_wrap_version` ausente antes de `V10`, o con valor `1` tras la migración) se restaura en la versión nueva y sus documentos siguen siendo legibles | `backup::service` |
| 17 | *(Solo aplica si en el futuro se implementara una migración de datos — no aplica a la Alternativa C recomendada, que no migra datos; se documenta como N/A explícito, no omitido por descuido)* | — |
| 18 | *(Mismo criterio que 17 — N/A bajo la Alternativa C, documentado explícitamente)* | — |

Los tests 17-18 del encargo (fallo/crash a mitad de migración; idempotencia de reintento) asumen
un diseño con migración de datos activa (Alternativas A/B). Bajo la Alternativa C recomendada, no
existe tal migración, así que estos dos casos son **estructuralmente inaplicables** — se documentan
como N/A explícito en vez de omitirse en silencio, tal como exige el encargo.

## 12. Confirmación o refutación de MULTI-1

**Confirmado por lectura directa** (`src-tauri/src/lib.rs`, `Cargo.toml`, `tauri.conf.json`): no
existe ningún mecanismo de single-instance hoy — ni `tauri-plugin-single-instance` en
dependencias, ni ninguna clave `"plugins"` relacionada en `tauri.conf.json`, ni ningún lock
file/flock manual en el código. **Sigue siendo posible ejecutar múltiples instancias
simultáneamente** apuntando al mismo `app_data_dir`.

Verificación adicional hecha en esta sesión (no estaba en la auditoría anterior): el flujo OAuth de
Google Calendar (`src-tauri/src/calendar/oauth.rs`) recibe el callback mediante un
**`TcpListener` en `127.0.0.1` con puerto efímero, dentro del mismo proceso** que abrió el
navegador — **no** usa un esquema de URL personalizado ni ningún mecanismo de activación que
dependa de lanzar una segunda instancia. Esto significa que introducir single-instance real **no
tendría ninguna interacción con el flujo OAuth** — son mecanismos completamente ortogonales.

## 13. Comparación single-instance vs. multi-instancia segura

### Alternativa A — Single-instance real (`tauri-plugin-single-instance`)

- **Compatibilidad con Tauri 2.11.3** (versión exacta en `Cargo.toml`): el plugin
  `tauri-plugin-single-instance` tiene una línea de versiones `2.x` mantenida oficialmente por el
  equipo de Tauri para la v2 del framework — compatible en principio; la versión exacta a pinear
  se confirmaría en el momento de la implementación contra el registro de crates.io.
- **macOS/Windows/Linux**: el plugin es multiplataforma por diseño — en macOS y Windows intercepta
  el segundo lanzamiento y reenvía sus argumentos (`argv`) y directorio de trabajo al proceso ya
  corriendo, vía un callback registrado en `.setup()`. En Linux (relevante para este entorno de
  desarrollo/CI) usa un mecanismo equivalente basado en un socket/lock local.
- **Al intentar abrir una segunda instancia**: el proceso nuevo termina inmediatamente (nunca llega
  a `tauri::Builder::default().run(...)` completo) tras reenviar su invocación al primero — el
  callback del plugin en el proceso original recibe esa invocación y decide qué hacer.
- **¿Se enfoca/reabre la ventana existente?** Es responsabilidad del callback que se registre —
  el patrón estándar es `window.set_focus()`/`window.show()` sobre la ventana principal ya
  existente. Requiere código explícito (pocas líneas) en `lib.rs`.
- **Interacción con vault bloqueado/desbloqueado**: ninguna — el plugin actúa antes de que el
  segundo proceso llegue a inicializar `VaultSession`; el estado del vault en el proceso original
  no se ve afectado en absoluto.
- **Impacto en temporales**: **resuelve MULTI-1 de raíz** — si nunca puede existir una segunda
  instancia con su propio `sweep_stale_temp_files()`, el escenario completo deja de ser posible.
- **Impacto en SQLite/SQLCipher**: resuelve también, como efecto colateral, el punto de
  `SQLITE_BUSY` entre instancias documentado en la auditoría (§12 de la Auditoría Pre-RC) — con una
  sola instancia posible, nunca hay dos conexiones concurrentes al mismo `vault.db`.
- **Interacción con el callback OAuth**: ninguna, confirmado arriba.
- **Orden correcto del plugin durante `setup()`**: debe registrarse **antes** que cualquier otra
  lógica de `setup()` que dependa de asumir que este es el único proceso vivo (en particular, antes
  de `run_startup_recovery`/`sweep_stale_temp_files`/`VaultSession::new`) — el patrón estándar de
  Tauri es registrar el plugin de single-instance como el primer `.plugin(...)` del `Builder`.
- **Tests automatizables**: el comportamiento de "reenviar argv y salir" del plugin en sí es del
  propio crate (ya probado upstream, no se re-testea). Lo que sí es responsabilidad del proyecto y
  testeable: que el callback registrado efectivamente enfoca la ventana — parcialmente testeable
  con un arnés de integración de Tauri, con limitaciones conocidas en un entorno headless.
- **Qué requiere necesariamente prueba física**: el comportamiento visual real de enfocar/traer al
  frente la ventana en macOS y Windows reales (WebKitGTK/Xvfb en este entorno no permite validar
  interacción de ventanas del gestor de ventanas real).
- **Dependencia**: **DEPENDENCIA NUEVA — REQUIERE APROBACIÓN** (`tauri-plugin-single-instance`, más
  el paquete `@tauri-apps/plugin-single-instance` si se expone algo al frontend — probablemente no
  es necesario para este caso, ya que la lógica vive enteramente en Rust).

### Alternativa B — Mantener multi-instancia, sweep seguro sin dependencia nueva

Analizado explícitamente cada elemento pedido:

- **PID en nombre/metadata**: viable — el nombre del temporal podría incluir el PID del proceso
  que lo creó (`cuaderno-clinico-doc-<pid>-<uuid>`). Permite que el barrido de arranque **de una
  instancia nueva** distinga sus propios temporales (no debería tener ninguno, al ser un arranque
  limpio) de los de un proceso anterior — pero **no basta por sí solo**: un PID por sí solo no dice
  si el proceso original sigue vivo o no.
- **Detección de proceso vivo/muerto por PID**: en Unix, `kill(pid, 0)` (señal 0, sin efecto,
  reporta si el proceso existe) es el mecanismo estándar — pero requiere código específico de
  plataforma (`libc` o un crate como `sysinfo`, que sería, otra vez, una dependencia nueva) y sufre
  del problema clásico de **reutilización de PID**: si el proceso original murió y el SO reasignó
  ese mismo PID a un proceso completamente distinto (poco probable pero no imposible en sesiones
  largas), la detección "está vivo" daría un falso positivo. En Windows, el equivalente
  (`OpenProcess`) tiene la misma limitación de fondo.
- **Directorio temporal por proceso**: en vez de nombres con prefijo plano en el directorio
  temporal compartido del sistema, cada instancia podría crear su propio subdirectorio
  (`temp_dir()/cuaderno-clinico-<pid-o-uuid-de-sesión>/`). Esto **elimina el problema de raíz**: el
  barrido de arranque de una instancia nueva nunca necesitaría tocar el subdirectorio de otra
  instancia, porque cada una tiene el suyo — el barrido de arranque solo necesitaría limpiar
  subdirectorios que YA existían antes de que este proceso arrancara (es decir, de una ejecución
  anterior, no de una instancia hermana actual). El desafío pasa a ser: ¿cómo distinguir "un
  subdirectorio de una instancia anterior ya terminada" de "un subdirectorio de una instancia
  hermana actualmente viva"? Vuelve a depender de una señal de vida.
- **Timestamp/mtime**: usar la antigüedad del archivo/directorio (p. ej. "más viejo que el momento
  de arranque de este proceso, menos un margen") es más robusto que un barrido ciego por nombre,
  pero tiene un defecto de fondo señalado correctamente por el encargo: **un temporal legítimo
  puede permanecer abierto por mucho tiempo** (una usuaria puede dejar un PDF abierto en un visor
  externo durante horas mientras trabaja) — un umbral de tiempo fijo inevitablemente sería
  arbitrario: demasiado corto, borra temporales legítimos de sesiones largas; demasiado largo, deja
  de resolver el caso real de "abro una segunda instancia por accidente cinco segundos después".
- **Archivo lock/lease**: la opción más robusta sin dependencia nueva — cada instancia, al arrancar,
  intenta adquirir un lock exclusivo (p. ej. vía `std::fs::File::create_new` sobre un archivo lock
  dedicado en `app_data_dir` — **no** en el directorio temporal del sistema, para no interferir con
  el barrido de temporales de Documentos) que declara "soy la instancia activa". Si el lock ya
  existe, esta instancia puede: (a) negarse a arrancar por completo (equivalente funcional a single
  instance, pero implementado a mano en vez de con el plugin — más superficie propia que mantener
  para el mismo resultado), o (b) arrancar en un "modo secundario" consciente de que no debe barrer
  temporales ajenos (bandera en memoria: `is_primary_instance = false` → `sweep_stale_temp_files()`
  se **omite por completo** si esta instancia no es la primaria). La opción (b) preserva
  multi-instancia genuina (ambas ventanas funcionan) a costa de que el barrido de arranque solo lo
  haga la primera instancia que arrancó — funcionalmente correcto, pero el archivo de lock en sí
  necesita su propia lógica de "¿el dueño del lock sigue vivo?" para no quedar huérfano tras un
  crash (mismo problema de fondo que la detección por PID, aplicado ahora al lock en vez de al
  temporal).
- **Windows file locking / semántica de `unlink` en macOS/Linux**: ya analizado en la auditoría
  anterior (§14) — sigue vigente, sin cambios: en Windows, borrar un archivo abierto por otro
  proceso sin `FILE_SHARE_DELETE` falla silenciosamente hoy (`let _ =`); en Unix, `unlink` sobre un
  archivo abierto no rompe al proceso que lo tiene abierto, pero invalida cualquier reapertura por
  ruta.
- **Privacidad si un crash deja plaintext**: el barrido de arranque (`sweep_stale_temp_files`) ya
  cubre este caso hoy — cualquier alternativa de B debe preservar esa cobertura, no debilitarla.
- **Limpieza de basura histórica**: cualquier diseño de "directorio por proceso" necesita, en algún
  momento, limpiar subdirectorios de instancias ya terminadas hace mucho — mismo problema de fondo
  que el barrido actual, solo que aplicado a directorios en vez de archivos individuales.

**Conclusión de la Alternativa B**: cada mejora incremental (PID en nombre, directorio por
proceso, timestamp) reduce la probabilidad del problema pero **ninguna combinación sin un
mecanismo de detección de vida confiable (lock file con verificación de dueño vivo) lo elimina por
completo** — y ese mecanismo de detección de vida termina siendo, en la práctica, una
reimplementación parcial y menos probada de exactamente lo que `tauri-plugin-single-instance` ya
resuelve de forma madura y mantenida por el propio equipo de Tauri.

## 14. Recomendación concreta para MULTI-1

**Se recomienda la Alternativa A (single-instance real vía `tauri-plugin-single-instance`)**,
sujeta a aprobación de la dependencia nueva, por las siguientes razones, en orden de peso:

1. Resuelve el hallazgo **de raíz** (elimina la precondición "puede existir una segunda instancia")
   en vez de mitigar sus síntomas.
2. Resuelve, como efecto colateral sin costo adicional, el punto de `SQLITE_BUSY` entre instancias
   ya observado en la auditoría anterior.
3. La Alternativa B, en su versión más robusta (lock file con detección de dueño vivo), termina
   siendo una reimplementación parcial de la misma idea con más superficie propia de código que
   mantener y testear, para un resultado equivalente o peor.
4. No hay interacción negativa con el flujo OAuth (confirmado directamente en esta sesión).
5. Respeta la prioridad no negociable #3 del encargo ("nunca borrar un temporal legítimo de otra
   instancia") de la forma más categórica posible: si nunca hay una segunda instancia, nunca puede
   ocurrir.

**Si la usuaria prefiere no introducir la dependencia nueva**, la alternativa de respaldo
recomendada dentro de B es el **archivo de lock en `app_data_dir` con modo secundario consciente**
(opción "b" descrita arriba: la segunda instancia arranca pero nunca ejecuta
`sweep_stale_temp_files()` ni el barrido de arranque, y advierte en la UI que ya hay una sesión
activa) — preserva multi-instancia genuina a cambio de una pieza pequeña y acotada de lógica propia
(un archivo lock + una bandera en memoria), sin necesitar detección de PID vivo si se acepta que el
lock puede quedar huérfano tras un crash (en cuyo caso, el siguiente arranque simplemente se
autodeclara primario de nuevo, sobrescribiendo el lock — comportamiento seguro porque la única
consecuencia de una detección errónea es, en el peor caso, dos instancias creyéndose ambas
"secundarias" tras una secuencia de crashes específica, lo cual solo significa que el barrido de
arranque no corre esa vez — nunca un borrado indebido).

## 15. Dependencia propuesta para MULTI-1

**DEPENDENCIA NUEVA — REQUIERE APROBACIÓN**, si se opta por la Alternativa A:

1. **Crate exacto**: `tauri-plugin-single-instance`.
2. **Versión propuesta**: línea `2.x`, versión exacta a confirmar contra crates.io en el momento
   de la implementación (compatible con `tauri = "2.11.3"` ya presente).
3. **Ecosistema/mantenedor**: mantenido oficialmente por el equipo de Tauri (`tauri-apps`), mismo
   nivel de confianza que `tauri-plugin-dialog`/`tauri-plugin-log` ya presentes en el proyecto.
4. **Compatibilidad**: diseñado específicamente para Tauri v2, sin requerir cambios de arquitectura
   del resto de la aplicación.
5. **Por qué es necesaria**: es la única forma de resolver MULTI-1 de raíz en vez de mitigarlo
   (§14).
6. **Alternativas sin dependencia**: Alternativa B, con las limitaciones analizadas en §13 —
   sí es viable, pero como plan B, no como primera recomendación.
7. **Riesgo de supply chain/mantenimiento**: bajo — mismo mantenedor que dos plugins ya adoptados
   por el proyecto.
8. **Features requeridas**: ninguna feature opcional más allá de la API por defecto.
9. **Impacto en binario**: despreciable.
10. **Archivos que cambiarían**: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`,
    `src-tauri/src/lib.rs` (registro del plugin + callback de enfoque de ventana),
    `src-tauri/capabilities/*.json` si el plugin requiere una entrada de capacidades (a confirmar
    en el momento de la implementación).

**No se ejecutó `cargo add` ni se modificó ningún archivo de configuración en esta sesión.**

## 16. Tests de MULTI-1

Si se aprueba la Alternativa A:

- Test de que el callback de single-instance está registrado (verificable a nivel de código/
  configuración, no de comportamiento real de ventanas).
- **Automatizable ahora**: ninguno de comportamiento real de enfoque de ventana (requiere un
  gestor de ventanas real).
- **Requiere prueba física**: lanzar una segunda instancia real en macOS/Windows y confirmar que
  la ventana existente se enfoca y que `sweep_stale_temp_files()` nunca corre una segunda vez.

Si se aprueba la Alternativa B (respaldo):

1. El lock se adquiere exitosamente en el primer arranque.
2. Un segundo arranque (mismo proceso de test, simulando una segunda instancia) detecta el lock
   existente y se marca a sí mismo como no-primario.
3. La instancia no-primaria nunca invoca `sweep_stale_temp_files()`.
4. Tras "matar" la primera instancia (liberar su lock explícitamente en el test) y arrancar una
   tercera, esta se convierte en primaria.
5. Un lock huérfano (simulando un crash: archivo de lock presente sin proceso vivo detrás) es
   reclamado de forma segura por el siguiente arranque, sin borrar temporales de forma indebida en
   el proceso.

## 17. Confirmación o refutación de BACKUP-1

**Confirmado por lectura directa** (`src-tauri/src/backup/service.rs`,
`src-tauri/src/backup/archive.rs`, sin cambios desde la auditoría anterior):
`archive::write_container(dest, entries)` llama `File::create_new(dest)` como su **primera
operación** — el archivo final ya existe en el filesystem, en la ruta elegida por la usuaria, antes
de que se haya escrito una sola entrada del ZIP. Si `io::copy`/`zip.start_file`/`zip.finish()`
fallan después de ese punto (disco lleno, permisos, crash del proceso), `create_backup` propaga el
error sin ninguna ruta de código que borre o mueva `dest_path` — solo limpia su propio directorio
de `scratch`, nunca el destino final. **El hallazgo es válido y sigue vigente.**

## 18. Diseño de backup atómico

**Propiedad exigida, formalizada como invariante**:

```
ANTES DE ÉXITO:        dest_path no existe.
DURANTE LA ESCRITURA:  solo existe un temporal, con un nombre que nunca podría
                        confundirse con un .cclinbackup completo (ni por
                        extensión ni por ubicación esperada de un backup).
TRAS zip.finish() OK:  rename atómico del temporal a dest_path — recién aquí
                        aparece el archivo final.
ANTE CUALQUIER ERROR:  temporal eliminado (best-effort), dest_path sigue sin existir.
```

**Alternativa evaluada y descartada — B (temporal dentro de `scratch`, luego mover al destino)**:
`scratch` vive bajo `vault_dir.parent()/backup-scratch` — casi siempre en el mismo volumen que
`app_data_dir`, **no necesariamente el mismo volumen que `dest_path`**, que la usuaria puede elegir
libremente (USB, disco externo, carpeta de red). `std::fs::rename` **no** hace fallback automático
a copiar+borrar cuando el origen y el destino están en dispositivos distintos — falla
explícitamente (`ErrorKind` de "cruza dispositivos" en Unix; error equivalente en Windows). Promover
desde `scratch` al destino elegido por la usuaria sería, en el caso común de un backup hacia un USB
o una carpeta de red, un `rename` entre volúmenes distintos que **fallaría siempre**, rompiendo la
función completa para ese caso de uso extremadamente común (hacer un backup hacia almacenamiento
externo es, si acaso, el caso de uso *más* esperable para un backup). **Descartada explícitamente
por esta razón.**

**Alternativa recomendada — A (temporal hermano del destino, mismo directorio)**:

```
<dest_path>.tmp-<uuid>          — mismo directorio que dest_path, por lo tanto
                                   garantizado en el mismo volumen que dest_path.
```

1. Verificar `dest_path.exists()` → si existe, `DestinationAlreadyExists` (igual que hoy).
2. Construir la ruta temporal hermana: `dest_path` con un sufijo `.tmp-<uuid-nuevo>` añadido al
   nombre de archivo (nunca simplemente `dest_path.tmp`, para permitir que dos instancias
   escribiendo hacia el mismo directorio no colisionen entre sí, aunque apunten al mismo
   `dest_path` final — cada una usa un UUID distinto).
3. Ejecutar exactamente el mismo proceso de escritura de hoy (`ZipWriter` + entradas +
   `zip.finish()`), pero contra la ruta temporal, no contra `dest_path`.
4. Si cualquier paso falla: eliminar el temporal (best-effort, mismo criterio que la limpieza de
   `scratch` ya existente) y propagar el error — `dest_path` nunca se tocó.
5. Si todo tiene éxito: opcionalmente, `file.sync_all()` sobre el archivo temporal antes de
   cerrarlo (durabilidad ante un corte de energía justo después del `rename` — costo bajo,
   beneficio real, patrón estándar de "reemplazo atómico de archivo" usado por bases de datos y
   editores de texto).
6. **Revalidación defensiva inmediatamente antes del `rename`**: volver a comprobar
   `dest_path.exists()` — si apareció entre el paso 1 y este punto (otra instancia, u otra
   aplicación, creó un archivo ahí mientras tanto), abortar con `DestinationAlreadyExists` en vez
   de sobrescribirlo silenciosamente (`std::fs::rename` en Rust **sí sobrescribe** un destino
   existente tanto en Unix como en Windows por defecto — sin esta revalidación, se perdería la
   garantía de "nunca sobrescribir en silencio" que la función ya tiene hoy vía `File::create_new`).
7. `std::fs::rename(&tmp_path, dest_path)` — atómico porque ambas rutas están garantizadas en el
   mismo directorio (mismo volumen).
8. Si el `rename` en sí falla (por la razón que sea, incluyendo la carrera del paso 6 si dos
   instancias llegan al mismo instante exacto): el error se propaga explícitamente, el temporal
   se limpia, `dest_path` no queda en un estado ambiguo — o bien no existe, o bien contiene un
   backup completo de alguna de las dos instancias en carrera (nunca uno parcial, porque el
   contenido completo ya estaba escrito en el temporal antes de intentar el `rename`).

**Sobre la carrera residual del paso 6-8 entre dos instancias apuntando al mismo `dest_path`**: se
documenta honestamente como una ventana estrecha que **no se elimina por completo** (ninguna
combinación portátil de "verificar + renombrar" sin una primitiva de "crear-o-fallar" atómica a
nivel de rename lo permite en Rust estándar de forma multiplataforma) — pero se acota su
consecuencia: en el peor caso, dos instancias escribiendo backups hacia el mismo nombre exacto en
el mismo instante exacto (una coincidencia de nombre elegida por la usuaria en ambas ventanas, algo
ya inusual) puede terminar con el backup de una sobrescribiendo el de la otra — **nunca con un
archivo parcial o corrupto**, que es la propiedad que BACKUP-1 exige cerrar. Esta ventana residual
es ortogonal al hallazgo original y de una clase de riesgo completamente distinta (colisión de
nombre elegido por la usuaria, no corrupción de contenido).

**Formato de `.cclinbackup` sin cambios**: esta corrección es puramente de **cuándo y dónde** se
escribe el ZIP, nunca de **qué** contiene — `manifest.json`/`vault.db`/`vault.meta.json`/`files/*`
siguen siendo exactamente las mismas entradas, en el mismo formato, con el mismo
`backup_format_version`. Backups ya creados (antes de esta corrección) **siguen restaurándose sin
ningún cambio** porque `restore_backup` nunca dependió de cómo se escribió el archivo, solo de su
contenido final.

## 19. Tests de BACKUP-1

| # | Test |
|---|---|
| 1 | Backup exitoso crea exactamente `dest_path` — ningún archivo `.tmp-*` sobrante en el directorio destino tras el éxito. |
| 2 | Mientras el contenedor no ha terminado de escribirse (simulado deteniendo la escritura a mitad, p. ej. con una fuente que falla en la N-ésima entrada), `dest_path` no existe todavía. |
| 3 | Fallo durante la lectura/copia de una entrada (archivo de origen corrupto/inaccesible simulado) no deja `dest_path`. |
| 4 | Fallo durante `zip.finish()` (simulado, p. ej. escribiendo a un destino de solo lectura tras crear el temporal) no deja `dest_path`. |
| 5 | El temporal se limpia tras un error, cuando el sistema de archivos lo permite. |
| 6 | Un `dest_path` preexistente nunca se sobrescribe (test ya existente, debe seguir pasando sin cambios). |
| 7 | Dos intentos de backup sobre el mismo `dest_path` (simulados secuencialmente en el test, dado que un test unitario no puede forzar una carrera real de hilos/procesos de forma determinista): el segundo debe fallar limpiamente si el primero ya completó, nunca corromper el resultado del primero. |
| 8 | Backup con documentos activos — sigue funcionando exactamente igual que hoy (test de regresión, no nuevo). |
| 9 | Backup con documentos archivados — ídem. |
| 10 | Backup con un ciphertext faltante sigue rechazándose explícitamente (test ya existente, `BackupError::MissingDocumentFile` — debe seguir pasando). |
| 11 | Restauración de un `.cclinbackup` creado **antes** de este cambio (fixture generado con el código actual, conservado como bytes fijos si es necesario) sigue funcionando exactamente igual tras el cambio. |
| 12 | Un backup nuevo (creado con el código corregido) sigue pasando `inspect_backup` y `restore_backup` sin cambios de comportamiento observable. |
| 13 | Si el `rename` final falla (simulado, p. ej. permisos), el error es explícito (`BackupError` de un tipo claro) y la función nunca reporta éxito. |
| 14 | Verificación explícita de que, en cualquier punto intermedio de una escritura simulada como fallida, **ningún archivo con el nombre exacto de `dest_path`** llega a existir en el sistema de archivos — la aserción central que demuestra que BACKUP-1 está cerrado. |

**Refactor mínimo necesario para poder simular fallos**: `write_container` (o una función interna
nueva y de alcance mínimo dentro de `backup/archive.rs`) necesitaría aceptar la ruta de destino
real como un parámetro ya calculado (la ruta temporal), en vez de decidir internamente escribir
directo al `dest` recibido — esto es, en la práctica, exactamente el cambio que BACKUP-1 exige
(separar "a qué ruta se escribe" de "cuál es el destino final"), así que no es una ampliación de
superficie más allá de lo que el propio hallazgo requiere corregir. Para simular fallos a mitad de
`io::copy` sin herramientas externas, se propone un helper de test que escriba entradas desde un
`Read` que falla deliberadamente después de N bytes (un `struct FailingReader` local al módulo de
tests, sin dependencia nueva) — patrón ya usado implícitamente por otros tests de error del
proyecto que fuerzan condiciones de fallo sin infraestructura externa.

## 20. Interacciones entre los cuatro fixes

1. **¿Single-instance afecta los tests de backup concurrente?** Si se aprueba la Alternativa A de
   MULTI-1, el test #7 de BACKUP-1 ("dos intentos sobre el mismo destino") deja de ser un escenario
   realista *entre instancias* (nunca puede haber una segunda instancia) — pero sigue siendo
   válido *dentro de la misma instancia* (dos operaciones de backup disparadas casi
   simultáneamente desde la misma UI, si la interfaz lo permitiera) o como prueba de la robustez
   del propio mecanismo de `rename` independientemente de si multi-instancia está prevenida a otro
   nivel. Se mantiene el test igual, documentando que cubre ambos escenarios.
2. **¿El nuevo `key_wrap_version` debe incluirse automáticamente en `VACUUM INTO` y por tanto en
   backup?** Sí, automáticamente y sin ningún cambio en `backup::service` — `VACUUM INTO` copia la
   tabla `documents` completa tal cual existe en ese instante, columna por columna, así que
   `key_wrap_version` viaja dentro del snapshot sin que `create_backup` necesite saber que esa
   columna existe. Mismo patrón ya verificado en la transición V7→V8→V9 (solo se tocaron literales
   de test de `schema_version`, nunca lógica de negocio de backup).
3. **¿Restaurar un backup legacy requiere una migración automática antes de abrir documentos?**
   No, bajo la Alternativa C recomendada — `db::run_migrations` (ya invocado dentro del flujo de
   `restore_backup` existente) migra el `vault.db` restaurado hasta `SCHEMA_V10` automáticamente
   como parte del proceso ya existente, agregando `key_wrap_version = 1` a sus documentos vía el
   `DEFAULT` de la columna — sin ningún paso nuevo de "migración de documentos" separado.
4. **¿Una migración de key wrapping podría interactuar con auto-lock?** Bajo la Alternativa C, no
   existe tal migración de datos, así que esta interacción **no aplica** — se documenta como
   descartada por diseño, no como un riesgo pendiente.
5. **¿Un backup puede ocurrir mientras existen documentos legacy aún no migrados?** Bajo la
   Alternativa C, esta pregunta pierde su premisa: los documentos "legacy" (v1) no están "pendientes
   de migrar" — son un estado permanente y válido, no una fase transitoria. Un backup los incluye
   con su propio `key_wrap_version = 1`, sin ambigüedad.
6. **¿Cambio de contraseña/recovery durante o después de "la migración" afecta el wrapping?** Bajo
   la Alternativa C, no hay ventana de migración que proteger — ya verificado en el resto de este
   plan (§20 de la auditoría, reconfirmado) que ninguna de las dos operaciones toca
   `wrapped_file_dek` de ningún documento en ninguna versión.
7. **¿Una segunda instancia podría iniciar una migración simultáneamente?** No aplica, mismo
   motivo que el punto 4 — no hay proceso de migración de datos que una segunda instancia pudiera
   disparar en paralelo.
8. **¿Un crash durante "migración" + restore posterior mantiene una ruta de recuperación segura?**
   No aplica directamente (sin migración de datos), pero el diseño de BACKUP-1 (temporal hermano +
   rename atómico) sí garantiza, de forma general y sin relación con CRYPTO-1, que un crash durante
   la creación de un backup nunca deja un archivo que aparente ser un respaldo completo —
   propiedad que se mantiene sin importar qué mezcla de `key_wrap_version` tuvieran los documentos
   en ese backup.

**Invariantes explícitos resultantes** (no solo "esto debería funcionar"):

- **I1**: todo documento, en cualquier momento de su existencia, tiene exactamente un
  `key_wrap_version` fijo desde su creación, que nunca cambia — no existe estado "a medias".
- **I2**: `unwrap_file_key` siempre puede determinar de forma no ambigua qué algoritmo usar,
  consultando únicamente la columna `key_wrap_version` de la fila del documento.
- **I3**: ningún archivo en la ubicación exacta de `dest_path` de un backup existe hasta que el
  contenedor completo ha terminado de escribirse exitosamente y ha sido promovido mediante un
  único `rename` atómico dentro del mismo volumen.
- **I4**: la DEK cruda del vault nunca cruza la frontera de `security::session`, en ninguna de las
  dos versiones de wrapping.
- **I5**: ningún test existente pierde cobertura ni se debilita como consecuencia de estos cuatro
  cambios — cada uno se extiende o se corrige en su propio módulo, sin tocar aserciones de otros
  verticales.

## 21. Threat model relevante (reconfirmado, sin cambios respecto de la auditoría)

Aplicación de escritorio local, de un solo usuario, sin componente de servidor — los cuatro
hallazgos se abordan bajo ese modelo, sin sobredimensionar el riesgo hacia escenarios de
servidor/multiusuario/red que el proyecto no tiene ni planea tener en esta fase (CLAUDE.md regla 7:
multiplataforma futura, no multiusuario).

## 22. Archivos exactos que se propone modificar (implementación futura, no en esta sesión)

**REG-1**:
- `src-tauri/src/services/document_crypto.rs` (solo el test).

**CRYPTO-1**:
- `src-tauri/src/security/session.rs` (HKDF, versión de wrapping, dispatch legacy/nuevo).
- `src-tauri/src/db/migrations.rs` (`SCHEMA_V10`).
- `src-tauri/src/repositories/documents.rs` (columna nueva en el struct y en las consultas).
- `src-tauri/src/services/documents.rs` (persistir `CURRENT_KEY_WRAP_VERSION` al crear; leer y
  despachar `key_wrap_version` al abrir).
- `src-tauri/Cargo.toml`/`Cargo.lock` (dependencia `hkdf`, sujeta a aprobación).
- `docs/documents.md` (documentar el esquema de versionado de wrapping).

**MULTI-1** (si se aprueba la Alternativa A):
- `src-tauri/src/lib.rs` (registro del plugin, callback de enfoque de ventana, orden de setup).
- `src-tauri/Cargo.toml`/`Cargo.lock` (dependencia `tauri-plugin-single-instance`, sujeta a
  aprobación).
- `src-tauri/capabilities/*.json` (posible entrada nueva, a confirmar).

**MULTI-1** (si se aprueba la Alternativa B de respaldo, sin dependencia):
- `src-tauri/src/lib.rs` (lock file, bandera de instancia primaria).
- `src-tauri/src/services/document_temp.rs` (condicionar el barrido a ser instancia primaria).

**BACKUP-1**:
- `src-tauri/src/backup/archive.rs` (`write_container` o función interna equivalente, ruta
  temporal + rename).
- `src-tauri/src/backup/service.rs` (orquestación del temporal hermano en `create_backup`).

**Documentación** (ver también §26):
- `docs/documents.md`, `docs/backup-restore.md`, `docs/ARCHITECTURE.md` (fila de fase +
  correcciones puntuales).

## 23. Archivos que explícitamente NO deben tocarse

Confirmado que ninguno de los cuatro fixes exige tocar lo siguiente — si la implementación futura
descubriera que sí hace falta, corresponde **detenerse y pedir aprobación específica** antes de
proceder, tal como exige el encargo:

- `src-tauri/src/security/envelope.rs` — el mecanismo de envoltura del DEK del vault (contraseña/
  recovery vía Argon2id) no se toca; CRYPTO-1 opera exclusivamente sobre `wrap_file_key`/
  `unwrap_file_key` en `session.rs`.
- `vault.meta.json` / su formato — sin cambios; no está involucrado en ninguno de los cuatro
  hallazgos.
- Argon2id / `security/kdf.rs` — sin cambios; la derivación de la KEK a partir de la contraseña no
  se toca.
- Flujo de `recovery`/`recover_access` — sin cambios de lógica; solo se verifica (sin modificar)
  que sigue preservando la legibilidad de documentos de ambas versiones.
- Formato físico `CCD1` (`document_crypto.rs::encrypt_document`/`decrypt_document`,
  `FILE_MAGIC`/`FILE_FORMAT_VERSION`) — sin cambios; CRYPTO-1 es sobre el wrapping de la DEK de
  archivo, no sobre el cifrado del contenido en sí.
- Formato del contenedor `.cclinbackup` (`manifest.json`, `BACKUP_FORMAT_VERSION`,
  `BackupManifest`) — sin cambios; BACKUP-1 es sobre *cuándo/dónde* se escribe el archivo, no sobre
  su contenido.
- `src-tauri/src/calendar/*` — ninguno de los cuatro hallazgos lo involucra.
- `src-tauri/src/db/connection.rs` — se **lee** para el diagnóstico (confirma cómo SQLCipher recibe
  la DEK), pero no se modifica.
- `SCHEMA_V1`–`SCHEMA_V9` — ningún hallazgo requiere editarlas; `SCHEMA_V10` es aditiva y nueva.

## 24. Migraciones

Una sola, aditiva: `SCHEMA_V10` (§10). Ninguna otra migración se requiere para los cuatro
hallazgos. `SCHEMA_V1`–`V9` permanecen exactamente como están.

## 25. Dependencias

Dos candidatas, **ninguna aprobada todavía**:

1. `hkdf` (RustCrypto) — para CRYPTO-1, ver §9.
2. `tauri-plugin-single-instance` (Tauri oficial) — para MULTI-1 (Alternativa A recomendada), ver
   §15. Con una alternativa de respaldo sin dependencia nueva disponible (§13, Alternativa B) si la
   usuaria prefiere no introducirla.

BACKUP-1 y REG-1 **no requieren ninguna dependencia nueva**.

## 26. Documentación a actualizar (si se aprueba la implementación)

- **`docs/documents.md`**: agregar una sección sobre `key_wrap_version`, explicando la distinción
  entre formato físico (`CCD1`/`format_version`) y esquema de wrapping (`key_wrap_version`),
  documentando ambos algoritmos (legacy v1, HKDF v2) y la decisión explícita de no migrar
  documentos existentes en esta fase (deuda técnica consciente).
- **`docs/backup-restore.md`**: documentar el mecanismo de escritura atómica del backup (temporal
  hermano + rename), y aclarar que el formato del contenedor no cambió.
- **`docs/ARCHITECTURE.md`**: fila de la Fase 17 en la tabla de fases, y cualquier corrección
  puntual directamente relevante (mismo criterio ya aplicado en el cierre de Fase 16: nunca corregir
  incidentalmente documentación histórica no relacionada).
- **`CLAUDE.md`**: no se identificó ninguna razón real para modificarlo — ninguna regla histórica
  necesita cambiar para cerrar estos cuatro hallazgos.

## 27. Riesgos

- **CRYPTO-1**: riesgo residual aceptado explícitamente de que los documentos legacy (v1) sigan sin
  separación de dominio criptográfica indefinidamente, salvo que una fase futura separada apruebe
  una migración opcional. Debe quedar documentado, no oculto.
- **MULTI-1**: si se elige la Alternativa B de respaldo en vez de single-instance real, persiste un
  residuo de la clase de problema (un lock huérfano tras crashes específicos podría, en un caso
  límite ya descrito, dejar el barrido de arranque sin correr una vez) — de impacto mucho menor que
  el hallazgo original, pero no absolutamente cero.
- **BACKUP-1**: la ventana residual de colisión de nombre entre dos instancias/operaciones
  apuntando exactamente al mismo `dest_path` en el mismo instante (§18) — de una clase de riesgo
  distinta (colisión de nombre, no corrupción) y de probabilidad muy baja.
- **General**: introducir dos dependencias nuevas (si ambas se aprueban) aumenta la superficie de
  mantenimiento del proyecto — mitigado por ser ambas de mantenedores ya confiados (RustCrypto,
  Tauri oficial).

## 28. Compatibilidad hacia atrás

- **REG-1**: total — es un cambio de test únicamente.
- **CRYPTO-1**: total, por diseño de la Alternativa C — ningún documento existente cambia de
  formato ni de wrapping.
- **MULTI-1**: total — no cambia ningún comportamiento del vault, de Documentos, ni de Backup/
  Restore; solo agrega (o no) la posibilidad de una segunda instancia.
- **BACKUP-1**: total — el formato `.cclinbackup` no cambia; backups ya creados restauran sin
  ningún cambio de comportamiento.

## 29. Compatibilidad de backups antiguos

Confirmada explícitamente para los cuatro hallazgos: ningún backup existente, de ninguna fase
anterior (incluyendo los que no tienen `files/` en absoluto, de antes de Fase 16), deja de
restaurarse como consecuencia de esta fase. `SCHEMA_V10` es aditiva, aplicada automáticamente
durante el flujo de migración ya existente dentro de `restore_backup`; el cambio de BACKUP-1 no
toca el formato del contenedor en absoluto.

## 30. Comportamiento ante crash

- **CRYPTO-1** (Alternativa C): sin ventana de migración, no hay nada que un crash pueda dejar a
  medias respecto a este hallazgo específicamente.
- **MULTI-1**: un crash de la instancia "primaria" (bajo la Alternativa B de respaldo) deja un
  lock huérfano, reclamado de forma segura por el siguiente arranque (§14). Bajo la Alternativa A,
  un crash de la única instancia posible se comporta exactamente igual que hoy (sin cambios).
- **BACKUP-1**: un crash a mitad de escritura dentro del temporal hermano deja un archivo
  `.tmp-<uuid>` huérfano junto al destino elegido, **nunca** un `dest_path` parcial — exactamente
  la propiedad que se buscaba. El temporal huérfano queda para limpieza manual o, opcionalmente, un
  futuro barrido (fuera de alcance de esta fase, ver §18).

## 31. Cambio de contraseña / recovery

Reconfirmado explícitamente para CRYPTO-1 (único hallazgo relacionado): ninguna de las dos
operaciones toca `wrapped_file_dek`/`wrap_nonce`/`key_wrap_version` de ningún documento — la DEK
del vault no cambia con ninguna de las dos operaciones (ya verificado con evidencia de test
existente, `dek_is_unchanged_by_a_password_change`), así que ambos esquemas de wrapping (v1
directo, v2 vía HKDF sobre la misma DEK) siguen produciendo la misma subclave/comportamiento antes
y después de cualquiera de las dos operaciones.

## 32. Plan de regresión

Antes de dar por cerrada la implementación futura (no en esta sesión): `cargo test --release`
100% verde y determinista (ejecutado al menos 3 veces consecutivas sin fallas intermitentes),
`cargo clippy --release --all-targets` 0 warnings, `cargo build --release` limpio, `npm run build`
limpio, `npm run lint` sin errores nuevos ni categorías de warning nuevas, `git diff --check`
limpio — comparado explícitamente contra el baseline de este plan (§2).

## 33. Plan de prueba manual

**AUTOMATIZABLE AHORA** (no requiere hardware macOS/Windows):
- Ninguno de los casos de esta fase es puramente automatizable de punta a punta en este entorno,
  porque WebKitGTK no renderiza aquí (limitación ya documentada) — pero la lógica de negocio de
  los cuatro hallazgos sí queda cubierta por tests automatizados (§11, §16, §19).

**REQUIERE HARDWARE REAL macOS/WINDOWS**:
- Lanzar una segunda instancia con un PDF temporal abierto, confirmar que (A) la ventana existente
  se enfoca sin crear una segunda, o (B) el temporal de la primera sigue existiendo si se optó por
  la alternativa de respaldo.
- Crear un backup exitoso y confirmarlo restaurable.
- Inducir un fallo de backup (p. ej. desconectar un USB a mitad de la escritura, o llenar el disco
  deliberadamente en un volumen de prueba) y confirmar que **no** aparece un `.cclinbackup` final
  parcial en el destino.
- Restaurar un backup creado **antes** de Fase 17 (con documentos de Fase 16, `key_wrap_version`
  implícito en `1` tras la migración) y abrir esos documentos.
- Cambiar la contraseña y volver a abrir esos mismos documentos.
- Recuperar acceso vía código de recuperación y volver a abrir esos mismos documentos.
- Crear un documento nuevo después de Fase 17 y confirmar que usa el esquema v2.
- Backup + restore de una mezcla de documentos legacy (v1) y nuevos (v2) en el mismo vault.
- Bloquear/desbloquear el vault durante una operación de backup en curso.

**No se inventará ningún resultado de estas pruebas manuales** — quedan diseñadas aquí, pendientes
de ejecución real en hardware, consistente con la política ya establecida del proyecto.

## 34. Documentación a actualizar

Ver §26 (sección consolidada arriba).

## 35. Criterios objetivos de aceptación de Fase 17

Retomando y precisando los 22 criterios del encargo:

1. REG-1 determinista — confirmado por 20+ ejecuciones aisladas consecutivas sin fallas.
2. `cargo test --release` 100% verde.
3. El test corregido se ejecuta repetidamente (mínimo 20 veces) sin flakiness.
4. CRYPTO-1 tiene separación de dominio real para documentos nuevos (v2), vía HKDF con contexto
   fijo, verificado por los tests de §11.
5. Ningún documento legacy de Fase 16 queda ilegible — verificado por el test de compatibilidad
   con fixture fijo (§11, test 10) y por el diseño mismo de la Alternativa C (§7).
6. Un backup antiguo de Fase 16 se restaura y sus documentos se abren — verificado por test (§11,
   test 16).
7. Cambio de contraseña preserva todos los documentos, legacy y nuevos — verificado (§11, test 13).
8. Recovery preserva todos los documentos, legacy y nuevos — verificado (§11, test 14).
9. MULTI-1 cerrado sin posibilidad razonable de que una segunda instancia borre un temporal
   legítimo de la primera — cerrado de raíz (Alternativa A) o mitigado con lock consciente
   (Alternativa B), según lo que se apruebe.
10. BACKUP-1 cerrado: ningún error durante la creación puede dejar un archivo final que parezca un
    backup exitoso — verificado por el test central (§19, test 14).
11. Backups antiguos siguen restaurándose — verificado (§19, test 11).
12. Restore sigue usando staging + promoción segura — sin cambios en `restore_backup`, verificado
    por regresión.
13. No se modifica el formato `CCD1` — confirmado, fuera de alcance (§23).
14. No se expone la DEK del vault — confirmado por diseño (§8, I4 en §20).
15. No aparece plaintext nuevo fuera de los lugares ya permitidos — ninguno de los cuatro fixes
    introduce un nuevo temporal plaintext (BACKUP-1 nunca maneja plaintext; MULTI-1 no cambia qué
    se escribe, solo cuántas instancias pueden coexistir; CRYPTO-1 opera sobre claves, no sobre
    contenido).
16. `cargo clippy --release --all-targets` 0 warnings.
17. `cargo build --release` limpio.
18. `npm run build` limpio.
19. `npm run lint` sin errores nuevos ni categorías de warning nuevas.
20. `git diff --check` limpio.
21. Ningún test existente eliminado, ignorado ni debilitado — el único test tocado (REG-1) se
    corrige preservando su propósito exacto, nunca se relaja.
22. Ninguna feature nueva mezclada en el diff — confirmado por el alcance estrictamente acotado de
    este plan (§22-23).

## 36. Orden recomendado de implementación futura

1. **REG-1** primero — el más simple, sin dependencias de los otros tres, restaura la confianza en
   la suite de tests antes de tocar nada más.
2. **BACKUP-1** segundo — autocontenido, sin dependencia nueva, sin interacción arquitectónica con
   los otros dos.
3. **MULTI-1** tercero — depende de una decisión de aprobación (A vs. B) que puede tomar más tiempo
   de decidir; conviene tenerla resuelta antes de escribir código.
4. **CRYPTO-1** último — el de mayor superficie (migración de esquema + dependencia nueva +
   cambios en tres capas) y el que más se beneficia de que los otros tres ya estén cerrados y
   verificados en verde, para aislar cualquier regresión que pudiera aparecer.

## 37. Decisiones que requieren la aprobación de la usuaria

1. **CRYPTO-1**: aprobar la Alternativa C (compatibilidad dual permanente y versionada) como
   estrategia, en vez de A o B.
2. **CRYPTO-1**: aprobar la dependencia `hkdf` (RustCrypto).
3. **CRYPTO-1**: aprobar el nombre y la cadena de contexto exactos (`key_wrap_version`,
   `"cuaderno-clinico:file-key-wrap:v1"`) o proponer alternativas.
4. **CRYPTO-1**: aceptar explícitamente el riesgo residual de que los documentos legacy permanezcan
   en el esquema v1 indefinidamente, salvo una fase futura separada de remigración opcional.
5. **MULTI-1**: elegir entre la Alternativa A (single-instance real, con la dependencia
   `tauri-plugin-single-instance`) o la Alternativa B de respaldo (lock file sin dependencia
   nueva, preservando multi-instancia genuina).
6. **BACKUP-1**: confirmar que el diseño de temporal hermano + rename atómico (Alternativa A de
   §18) es aceptable, incluyendo la ventana residual de colisión de nombre descrita.
7. **General**: confirmar el orden de implementación propuesto (§36), o indicar uno distinto.

## 38. Recomendación final

Se recomienda aprobar: REG-1 tal como está diseñado (sin alternativa real que considerar); BACKUP-1
tal como está diseñado (Alternativa A, temporal hermano); CRYPTO-1 con la Alternativa C
(compatibilidad dual versionada) y la dependencia `hkdf`; y MULTI-1 con la Alternativa A
(`tauri-plugin-single-instance`) como primera preferencia, por resolver el problema de raíz y
como beneficio colateral cerrar también el punto de `SQLITE_BUSY` ya documentado — con la
Alternativa B disponible como respaldo válido si se prefiere evitar la dependencia nueva o se
considera valioso preservar multi-instancia como capacidad del producto.

## 39. Preguntas explícitas para la usuaria antes de implementar

1. ¿Aprueba la Alternativa C para CRYPTO-1, aceptando el riesgo residual documentado para
   documentos legacy?
2. ¿Aprueba agregar `hkdf` (RustCrypto) como dependencia nueva?
3. ¿Prefiere single-instance real (`tauri-plugin-single-instance`, dependencia nueva) o el
   respaldo sin dependencia para MULTI-1? Si prefiere single-instance: ¿confirma que renunciar a
   la posibilidad de multi-instancia es aceptable como decisión de producto?
4. ¿Aprueba el diseño de temporal hermano + rename atómico para BACKUP-1, incluyendo la ventana
   residual de colisión de nombre descrita en §18?
5. ¿Aprueba el orden de implementación propuesto (§36), o prefiere otro?
6. ¿Desea que se incluya, en esta misma fase o en una futura, una herramienta opcional de
   remigración de documentos legacy v1→v2 (Alternativa D, §7), o queda explícitamente diferida sin
   fecha?

---

# PROPUESTA DE BLOQUES DE IMPLEMENTACIÓN

**Ninguno de estos bloques se ejecuta todavía.** Se entregan para que la aprobación futura pueda
ser tan granular como la usuaria prefiera (aprobar todos, algunos, o pedir cambios a uno en
particular sin afectar a los demás).

### Bloque 1 — REG-1: test determinista

- **Objetivo**: eliminar el flakiness de `resolve_within_files_root_rejects_an_uppercase_shard`.
- **Archivos**: `src-tauri/src/services/document_crypto.rs` (test únicamente).
- **Cambios**: reemplazar el UUID aleatorio por un literal fijo con letra hex en el shard (§4).
- **Tests**: el propio test corregido, ejecutado 20+ veces consecutivas.
- **Riesgos**: ninguno — cambio de alcance mínimo, sin tocar código productivo.
- **Criterio de cierre**: 20/20 ejecuciones aisladas en verde, más 3 corridas completas de
  `cargo test --release` en verde consecutivas.
- **Dependencia de bloques anteriores**: ninguna — puede ejecutarse de forma completamente
  independiente.

### Bloque 2 — BACKUP-1: escritura atómica

- **Objetivo**: garantizar que `create_backup` nunca deja un `.cclinbackup` parcial en el destino
  final.
- **Archivos**: `src-tauri/src/backup/archive.rs`, `src-tauri/src/backup/service.rs`.
- **Cambios**: temporal hermano del destino + revalidación defensiva + `rename` atómico (§18);
  refactor mínimo de `write_container` para aceptar la ruta de escritura real como parámetro
  separado del destino final, habilitando la simulación de fallos en tests.
- **Tests**: los 14 de §19.
- **Riesgos**: la ventana residual de colisión de nombre entre dos escrituras simultáneas al mismo
  `dest_path` exacto (§18) — documentada, no eliminable de forma portátil.
- **Criterio de cierre**: los 14 tests en verde; los tests de regresión de backup/restore ya
  existentes (Fase 10/16) siguen en verde sin modificación; un `.cclinbackup` generado antes de
  este bloque sigue restaurándose sin cambios.
- **Dependencia de bloques anteriores**: ninguna.

### Bloque 3 — MULTI-1: decisión de arquitectura (single-instance o lock consciente)

- **Objetivo**: cerrar la posibilidad de que una segunda instancia borre un temporal legítimo de
  la primera.
- **Archivos**: si Alternativa A: `src-tauri/src/lib.rs`, `Cargo.toml`/`Cargo.lock`,
  `capabilities/*.json`. Si Alternativa B: `src-tauri/src/lib.rs`,
  `src-tauri/src/services/document_temp.rs`.
- **Cambios**: según la alternativa aprobada (§13-14).
- **Tests**: según §16 (distintos para cada alternativa).
- **Riesgos**: si A, ninguno más allá de la aprobación de la dependencia; si B, el residuo de lock
  huérfano descrito en §27.
- **Criterio de cierre**: el escenario "segunda instancia con temporal abierto en la primera" deja
  de poder borrar ese temporal, verificado por test (B) o por imposibilidad estructural (A) más
  prueba manual pendiente en hardware.
- **Dependencia de bloques anteriores**: ninguna — pero requiere la decisión de la usuaria (§37,
  punto 5) antes de empezar a escribir código, dado que A y B son mutuamente excluyentes.

### Bloque 4 — CRYPTO-1: migración de esquema (`SCHEMA_V10`)

- **Objetivo**: introducir la columna `key_wrap_version` sin tocar ningún dato existente.
- **Archivos**: `src-tauri/src/db/migrations.rs`.
- **Cambios**: `SCHEMA_V10` aditiva (§10).
- **Tests**: columnas presentes desde el arranque; default correcto para filas preexistentes;
  idempotencia; preservación de datos anteriores a V10 (mismo patrón que tests de V9).
- **Riesgos**: ninguno — migración puramente aditiva, sin reconstrucción de tabla.
- **Criterio de cierre**: los tests de migración en verde; una base V9 real (con documentos de
  prueba) migra a V10 sin pérdida de ninguna columna ni fila existente.
- **Dependencia de bloques anteriores**: ninguna, pero debe preceder al Bloque 5.

### Bloque 5 — CRYPTO-1: separación de dominio en `security::session`

- **Objetivo**: introducir HKDF y el despacho legacy/nuevo en `wrap_file_key`/`unwrap_file_key`.
- **Archivos**: `src-tauri/src/security/session.rs`, `Cargo.toml`/`Cargo.lock` (dependencia
  `hkdf`).
- **Cambios**: ver §8 — nuevo enum/constantes de versión, `wrap_file_key` siempre produce v2,
  `unwrap_file_key` recibe la versión y despacha.
- **Tests**: los 9 primeros de §11 (los que viven enteramente en `security::session`).
- **Riesgos**: es el cambio de mayor sensibilidad de los cinco bloques — cualquier error aquí
  podría, en el peor caso, afectar la legibilidad de documentos nuevos (nunca de los legacy, que
  no se tocan). Mitigado por la cobertura de tests exhaustiva ya diseñada.
- **Criterio de cierre**: los 9 tests en verde; el test de compatibilidad con el fixture legacy
  fijo (test 10 de §11) pasa sin cambios en el comportamiento v1.
- **Dependencia de bloques anteriores**: requiere el Bloque 4 completado (la columna debe existir
  antes de que el código de servicio pueda leerla/escribirla).

### Bloque 6 — CRYPTO-1: integración en `services`/`repositories::documents`

- **Objetivo**: que la creación y apertura de documentos usen correctamente `key_wrap_version`.
- **Archivos**: `src-tauri/src/repositories/documents.rs`, `src-tauri/src/services/documents.rs`.
- **Cambios**: persistir `CURRENT_KEY_WRAP_VERSION` al crear; leer y despachar al abrir.
- **Tests**: tests 11-16 de §11 (integración).
- **Riesgos**: bajo, dado que el Bloque 5 ya validó la lógica criptográfica en aislamiento — este
  bloque es principalmente de fontanería (plumbing) entre capas ya bien establecidas en el
  proyecto.
- **Criterio de cierre**: los 6 tests de integración en verde; un documento creado con el código
  anterior a esta fase (simulado con `key_wrap_version = 1` en el fixture) se abre correctamente;
  un documento creado con el código de esta fase se abre correctamente; backup+restore de una
  mezcla de ambos preserva la legibilidad de los dos.
- **Dependencia de bloques anteriores**: requiere el Bloque 5 completado.

### Bloque 7 — Documentación y cierre

- **Objetivo**: actualizar la documentación afectada y producir el informe de cierre de Fase 17.
- **Archivos**: `docs/documents.md`, `docs/backup-restore.md`, `docs/ARCHITECTURE.md`.
- **Cambios**: ver §26.
- **Tests**: ninguno nuevo — este bloque es puramente documental.
- **Riesgos**: ninguno.
- **Criterio de cierre**: documentación coherente con el código final; informe de cierre siguiendo
  el formato obligatorio de `CLAUDE.md` §10.
- **Dependencia de bloques anteriores**: requiere los Bloques 1-6 completados.

---

## STOP

Este plan termina aquí. **No se implementó ningún hallazgo, no se agregó HKDF, no se cambió el
wrapping, no se migraron documentos, no se agregó single-instance, no se cambió
`sweep_stale_temp_files`, no se cambió backup, no se agregaron dependencias, no se modificaron
migraciones, no se implementó ninguna feature.** Se espera la revisión y decisión explícita de la
usuaria sobre las preguntas de §39 antes de recibir autorización para implementar la Fase 17.
