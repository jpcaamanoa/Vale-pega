# Integración SQLite + SQLCipher (Fase 1.2)

Este documento registra exactamente qué se integró, con qué versiones, cómo se
compila/enlaza, cómo se aplica la clave y cómo se verificó que es real
SQLCipher y no SQLite sin cifrar. Es el complemento técnico de la sección 5 de
`docs/ARCHITECTURE.md`.

## Versiones exactas (verificadas, no supuestas)

| Componente | Versión | Fuente |
|---|---|---|
| `rusqlite` | `0.40.2` | `src-tauri/Cargo.lock` |
| `libsqlite3-sys` (vendoriza SQLite+SQLCipher) | `0.38.2` | `src-tauri/Cargo.lock` |
| SQLCipher (amalgama fuente vendorizada dentro de `libsqlite3-sys`) | `4.14.0 community` | Leído directamente del código fuente vendorizado (`CIPHER_VERSION_NUMBER`/`CIPHER_VERSION_BUILD` en `sqlcipher/sqlite3.c`) **y** confirmado en tiempo de ejecución vía `PRAGMA cipher_version;` en los tests |
| SQLite (base sobre la que corre SQLCipher) | `3.51.3` | `#define SQLITE_VERSION` en el mismo archivo vendorizado |
| `openssl-src` (OpenSSL vendorizado, provee la criptografía a SQLCipher) | `300.6.1+3.6.3` → OpenSSL `3.6.3` | `src-tauri/Cargo.lock` |
| `zeroize` | `1.9.0` | `src-tauri/Cargo.lock` |
| Rust | `1.94.1` | `rustc --version` en este entorno |

No se supuso ninguna de estas versiones: se confirmaron leyendo el código
fuente vendorizado y ejecutando el binario compilado.

## Cómo se compila y enlaza

`src-tauri/Cargo.toml`:

```toml
rusqlite = { version = "0.40.2", features = ["bundled-sqlcipher-vendored-openssl"] }
zeroize = "1.9"
```

La feature `bundled-sqlcipher-vendored-openssl` hace tres cosas a la vez:

1. Compila desde cero la amalgama de SQLite **parcheada con el código de
   SQLCipher** (no es SQLite + una librería externa: es un único archivo C con
   el motor de cifrado integrado).
2. Compila OpenSSL `3.6.3` desde su fuente (vendorizado por `openssl-src`) y lo
   enlaza estáticamente como proveedor criptográfico de SQLCipher — así no
   depende de que la máquina de destino tenga OpenSSL instalado, que era
   justamente el problema que esta decisión buscaba evitar (ver
   `docs/ARCHITECTURE.md`, sección 2).
3. Todo queda estáticamente enlazado en el binario final de la aplicación.

**Punto estructural importante:** el proyecto usa *exclusivamente* la feature
`bundled-sqlcipher-vendored-openssl`. No se activó `bundled` (SQLite plano) en
ningún lado. Esto significa que el binario compilado **no contiene una copia
de SQLite sin cifrar** — no es que el código "elija" usar la versión cifrada
en tiempo de ejecución, es que la versión sin cifrar no existe en el binario.
No hay una ruta de código que pueda hacer fallback silencioso a texto plano
porque no hay a qué hacer fallback.

Requisitos de compilación usados en este entorno (Linux): `build-essential`,
`perl`, `make`, `pkg-config` (para compilar OpenSSL desde fuente y la
amalgama C de SQLCipher). Ver limitaciones por plataforma más abajo.

## Cómo se aplica la clave

Implementado en `src-tauri/src/db/connection.rs` (`open_vault`). Puntos de
diseño:

- **Modo raw key, no frase de paso.** La clave se aplica como
  `PRAGMA key = "x'<64 caracteres hex>'";`, es decir, 32 bytes (256 bits) ya
  derivados, no una contraseña que SQLCipher tenga que procesar con su propio
  KDF (PBKDF2). Esto es intencional: la Fase 1.4 derivará la clave real con
  Argon2id (más resistente a fuerza bruta con GPU que el PBKDF2 interno de
  SQLCipher) y se la entregará a este módulo ya lista. Este módulo **no sabe
  nada de contraseñas** — solo sabe recibir 32 bytes y aplicarlos.
- La pragma se ejecuta con `execute_batch` sobre un `String` construido a
  mano, no con un parámetro ligado (`?1`) de rusqlite. Es deliberado: SQLCipher
  exige la sintaxis literal `x'...'` en el texto mismo de la instrucción; si se
  pasara como parámetro ligado normal, el driver la trataría como una cadena
  SQL corriente y la sintaxis de clave raw dejaría de reconocerse.
- `VaultKey` (el tipo que envuelve los 32 bytes) implementa `Drop` con
  `zeroize()` (crate `zeroize`, no una implementación propia) y su `Debug` está
  redactado a propósito — nunca se puede imprimir la clave por accidente en un
  log o un panic. Ver `docs/ARCHITECTURE.md` sección 13.A (minimización de
  exposición).
- `VaultKey::from_slice(&[u8])` es el punto de integración ya preparado para
  la Fase 1.4: cuando exista el DEK desenvuelto (Argon2id + cifrado por
  sobres), se construirá un `VaultKey` a partir de él con esta misma función,
  sin tocar `connection.rs`.

## Cómo se verificó que es SQLCipher real (no SQLite sin cifrar)

Dos verificaciones independientes, ambas dentro de `open_vault`, en este
orden:

1. **`PRAGMA cipher_version;` debe devolver una fila.** Una build de SQLite
   sin SQLCipher no reconoce esa pragma en absoluto y no devuelve ninguna
   fila (SQLite ignora pragmas desconocidas en silencio, no lanza error). Si
   no hay fila, se devuelve `VaultError::NotSqlCipher` — fallo explícito, la
   aplicación nunca continúa como si nada. Test:
   `reports_a_real_sqlcipher_version_via_pragma`, que además valida que la
   versión reportada empieza con `"4."` (coincide con la versión `4.14.0`
   confirmada en el código fuente).
2. **Una lectura real de `sqlite_master` debe funcionar con la clave
   correcta y fallar con cualquier otra.** Esto no es una suposición teórica:
   al correr los tests con una clave incorrecta, SQLCipher registra en sus
   propios logs internos (capturados en la corrida real de `cargo test`):

   ```
   ERROR CORE sqlcipher_page_cipher: hmac check failed for pgno=1
   ERROR CORE sqlite3Codec: error decrypting page 1 data: 1
   ERROR CORE sqlcipher_codec_ctx_set_error 1
   ```

   Es decir, el propio motor de SQLCipher confirma que intentó autenticar la
   página 1 con HMAC y falló — evidencia directa de que la verificación de
   clave incorrecta es real, no simulada.
3. **Inspección de bytes en disco.** El test
   `encrypted_file_does_not_start_with_the_plain_sqlite_header` escribe un
   paciente de prueba con un nombre reconocible, cierra la conexión, y luego
   lee el archivo `.db` directamente del disco (sin pasar por SQLite/rusqlite
   en absoluto) para comprobar dos cosas: que los primeros 16 bytes **no** son
   el encabezado estándar `"SQLite format 3\0"`, y que la cadena de texto del
   nombre del paciente no aparece en ninguna parte del archivo. Esta es la
   prueba más directa de "los datos no quedan legibles como SQLite
   convencional desde fuera de la aplicación": es literalmente abrir el
   archivo como bytes crudos, igual que lo haría un atacante con acceso al
   disco.

## Resultado de los tests (Fase 1.2)

`cargo test` en `src-tauri/`, 11/11 tests en verde:

| Test | Qué prueba |
|---|---|
| `creates_and_writes_to_a_new_encrypted_vault` | Crear un vault nuevo y escribir/leer datos reales |
| `reports_a_real_sqlcipher_version_via_pragma` | `PRAGMA cipher_version` responde y es `4.x` |
| `closing_and_reopening_with_the_correct_key_preserves_data` | Cerrar la conexión y reabrir con la misma clave conserva los datos |
| `rejects_the_wrong_key_after_the_vault_has_real_data` | Una clave distinta a la usada para crear el vault es rechazada |
| `encrypted_file_does_not_start_with_the_plain_sqlite_header` | El archivo en disco no es SQLite en texto plano, ni contiene el dato sensible en claro |
| `rejects_a_corrupt_or_invalid_file` | Un archivo que no es una base de datos válida es rechazado (mismo camino de error que clave incorrecta — ver limitación abajo) |
| `debug_never_prints_the_key_bytes` | El `Debug` de `VaultKey` nunca expone los bytes de la clave |
| `from_slice_rejects_wrong_length` / `from_slice_accepts_correct_length` / `empty_key_from_slice_of_zero_length_is_rejected` | El punto de integración para el futuro DEK (Fase 1.4) valida longitud explícitamente, sin truncar ni rellenar en silencio |

No hay ningún test que dependa de "parece que funciona": cada uno verifica un
comportamiento observable (dato leído, error devuelto, bytes en disco, log de
SQLCipher).

## Limitaciones y notas honestas

- **"Clave incorrecta" y "archivo corrupto" son indistinguibles.** SQLCipher
  autentica cada página con HMAC por diseño; ambos casos fallan exactamente
  de la misma manera (fallo de autenticación de la página 1). `VaultError`
  refleja esto con una sola variante (`WrongKeyOrCorrupt`) en vez de fingir
  una precisión que el driver no tiene. La UI de la Fase 1.4 deberá mostrar un
  mensaje que cubra ambos casos ("no se pudo abrir: contraseña incorrecta o
  archivo dañado"), no uno que asuma cuál de los dos ocurrió.
- **No se implementó Argon2id ni cifrado por sobres en esta fase**, tal como
  se pidió explícitamente. `VaultKey` es un contenedor de 32 bytes sin opinión
  sobre de dónde vienen; hoy los tests usan bytes fijos de prueba. La Fase 1.4
  reemplaza esos bytes de prueba por el DEK real desenvuelto, sin tener que
  tocar este módulo.
- **Linux (este entorno de desarrollo/CI):** requiere `build-essential`,
  `perl`, `make` y `pkg-config` instalados para compilar OpenSSL y la amalgama
  de SQLCipher desde fuente. Quedan documentados aquí porque no vienen
  instalados por defecto en una imagen mínima de Ubuntu — se instalaron
  explícitamente en este entorno para poder compilar y correr los tests.
- **macOS:** la misma feature (`bundled-sqlcipher-vendored-openssl`) debería
  compilar sin depender de Homebrew/OpenSSL del sistema, que es justamente el
  punto de "vendored" — pero **esto no se ha probado todavía en una máquina
  macOS real** en este proyecto; solo se verificó en este entorno Linux
  headless. Se recomienda confirmarlo la primera vez que se compile en tu Mac,
  antes de asumir que el comportamiento es idéntico.
- **Windows:** mismo caso — la feature vendoriza OpenSSL precisamente para
  evitar depender de vcpkg o de una instalación manual de OpenSSL en Windows,
  pero tampoco se ha compilado todavía en un entorno Windows real. Un riesgo
  conocido en Windows con crates que compilan C/C++ es la necesidad de las
  "Herramientas de compilación de Visual Studio" (MSVC build tools); esto se
  validará cuando corresponda empaquetar para Windows (Fase 8), y se avisará
  de inmediato si aparece un problema de compatibilidad real en vez de
  asumir que "debería funcionar".
- **Tiempo de compilación:** compilar OpenSSL desde fuente añade
  aproximadamente 1–2 minutos a un build limpio (medido en este entorno). Es
  el costo de no depender de OpenSSL del sistema; no afecta el tiempo de
  arranque de la aplicación ya compilada.
- **No se guarda la contraseña maestra** porque en esta fase no existe todavía
  el concepto de contraseña — solo bytes de clave cruda que ni siquiera se
  registran en logs (ver `debug_never_prints_the_key_bytes`) y se zeroizan al
  soltarse (`Drop` de `VaultKey`). La garantía equivalente para la contraseña
  real se construye en la Fase 1.4 sobre esta misma base.
