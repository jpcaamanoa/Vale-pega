//! Apertura de la base de datos cifrada.
//!
//! Diseño (Fase 1.2, ver `docs/ARCHITECTURE.md` sección 5 y `docs/sqlcipher.md`):
//!
//! - La clave se aplica en **modo raw key** (`PRAGMA key = "x'<64 hex>'"`), nunca
//!   como frase de paso. Esto evita el KDF interno (PBKDF2) de SQLCipher, que es
//!   más débil que Argon2id. La derivación real de la clave a partir de la
//!   contraseña maestra (Argon2id + cifrado por sobres) es responsabilidad del
//!   módulo `security`, que se implementa en la Fase 1.4 — este módulo solo
//!   define el punto de integración (`VaultKey`) que lo recibirá.
//! - El binario se compila exclusivamente con la feature
//!   `bundled-sqlcipher-vendored-openssl` de `rusqlite`. No existe ninguna ruta
//!   de código que use SQLite sin cifrar: si SQLCipher no está realmente
//!   presente, o la clave es incorrecta, `open_vault` devuelve un error
//!   explícito. Nunca hay un "sigue funcionando en modo plano" silencioso.
//! - `PRAGMA cipher_version` prueba que el binario está enlazado con SQLCipher
//!   (una build sin SQLCipher no reconoce esa pragma y no devuelve filas).
//!   Es independiente de si la clave es correcta: no toca páginas cifradas.
//! - Leer `sqlite_master` prueba que la clave es correcta: con clave
//!   incorrecta, o archivo corrupto, SQLCipher no puede autenticar/desencriptar
//!   la primera página y la consulta falla. SQLCipher autentica cada página con
//!   HMAC por diseño, así que "clave incorrecta" y "archivo corrupto" son
//!   indistinguibles para el driver — no se puede prometer más precisión que
//!   esa, y no lo intentamos.

// Fase 1.2: este módulo todavía no está conectado a ningún comando Tauri (eso
// llega en las Fases 1.4/1.5, cuando exista un flujo real de "crear/desbloquear
// vault"). Hasta entonces solo lo ejercitan los tests de abajo, así que el
// compilador vería `dead_code` en el build normal si no se silencia aquí.
#![allow(dead_code)]

use std::fmt;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use zeroize::Zeroize;

/// Longitud de la clave cruda (256 bits), tal como la usará SQLCipher en modo
/// raw key y tal como la entregará el futuro DEK de la Fase 1.4.
pub const VAULT_KEY_LEN: usize = 32;

/// Clave cruda de 256 bits para SQLCipher.
///
/// No deriva nada por sí misma: es un contenedor que evita que la clave se
/// imprima por accidente (`Debug` la redacta) y que se zeroiza al soltarse.
/// En la Fase 1.4, el módulo de seguridad construirá esto a partir del DEK
/// desenvuelto (Argon2id + cifrado por sobres) en vez de bytes de prueba.
pub struct VaultKey([u8; VAULT_KEY_LEN]);

#[derive(Debug)]
pub struct VaultKeyError {
    got_len: usize,
}

impl fmt::Display for VaultKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "la clave debe tener exactamente {VAULT_KEY_LEN} bytes, se recibieron {}",
            self.got_len
        )
    }
}

impl std::error::Error for VaultKeyError {}

impl VaultKey {
    pub fn new(bytes: [u8; VAULT_KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// Punto de integración para la Fase 1.4: construye la clave a partir de
    /// un buffer de longitud variable (p. ej. el DEK ya desenvuelto), fallando
    /// explícitamente si la longitud no es la esperada en vez de truncar o
    /// rellenar en silencio.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, VaultKeyError> {
        if bytes.len() != VAULT_KEY_LEN {
            return Err(VaultKeyError {
                got_len: bytes.len(),
            });
        }
        let mut buf = [0u8; VAULT_KEY_LEN];
        buf.copy_from_slice(bytes);
        Ok(Self(buf))
    }

    fn as_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// Nunca se debe poder imprimir la clave por accidente (logs, panics, `dbg!`).
impl fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultKey").field("bytes", &"<redacted>").finish()
    }
}

impl Drop for VaultKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug)]
pub enum VaultError {
    /// Error de bajo nivel de SQLite no relacionado con la clave (p. ej. no se
    /// pudo abrir el archivo por permisos).
    Sqlite(rusqlite::Error),
    /// El binario no respondió a `PRAGMA cipher_version`: no está enlazado con
    /// SQLCipher. Fallo explícito — nunca se continúa en modo sin cifrar.
    NotSqlCipher,
    /// Clave incorrecta, o archivo dañado/no es una base de datos SQLCipher
    /// válida. Ver nota de diseño arriba sobre por qué no se distinguen.
    WrongKeyOrCorrupt,
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::Sqlite(e) => write!(f, "error de SQLite: {e}"),
            VaultError::NotSqlCipher => write!(
                f,
                "el binario no está enlazado con SQLCipher (PRAGMA cipher_version no respondió)"
            ),
            VaultError::WrongKeyOrCorrupt => {
                write!(f, "clave incorrecta, o la base de datos está dañada o no es válida")
            }
        }
    }
}

impl std::error::Error for VaultError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VaultError::Sqlite(e) => Some(e),
            VaultError::NotSqlCipher | VaultError::WrongKeyOrCorrupt => None,
        }
    }
}

impl From<rusqlite::Error> for VaultError {
    fn from(e: rusqlite::Error) -> Self {
        VaultError::Sqlite(e)
    }
}

/// Abre (o crea, si no existe) la base de datos cifrada en `path` usando
/// `key`, y verifica de extremo a extremo que el resultado es realmente una
/// conexión SQLCipher operativa antes de devolverla.
///
/// Nunca degrada a SQLite sin cifrar: si cualquiera de las verificaciones
/// falla, se devuelve `Err` explícito.
pub fn open_vault(path: &Path, key: &VaultKey) -> Result<Connection, VaultError> {
    let conn = Connection::open(path)?;

    // La clave se aplica en modo raw key ANTES de cualquier lectura o
    // escritura real de páginas. `execute_batch` (no un parámetro ligado) es
    // intencional: SQLCipher exige la sintaxis literal `x'<hex>'` en el texto
    // de la propia PRAGMA, no un valor de cadena de SQL corriente.
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", key.as_hex()))?;

    // (1) ¿Este binario está realmente enlazado con SQLCipher? Una build sin
    // SQLCipher no reconoce `cipher_version` y no devuelve ninguna fila (no
    // lanza error) — lo tratamos como fallo explícito.
    let cipher_version: Option<String> = conn
        .query_row("PRAGMA cipher_version;", [], |row| row.get(0))
        .optional()?;
    if cipher_version.is_none() {
        return Err(VaultError::NotSqlCipher);
    }

    // (2) ¿La clave es correcta? Forzamos una lectura real del esquema: con
    // clave incorrecta, o archivo corrupto, SQLCipher no puede
    // autenticar/desencriptar la primera página y esto falla.
    conn.query_row("SELECT count(*) FROM sqlite_master;", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|_| VaultError::WrongKeyOrCorrupt)?;

    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cuaderno-clinico-test-{}-{}",
            std::process::id(),
            name
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir.join("vault.db")
    }

    fn key(byte: u8) -> VaultKey {
        VaultKey::new([byte; VAULT_KEY_LEN])
    }

    #[test]
    fn debug_never_prints_the_key_bytes() {
        let k = key(0xAB);
        let debug_output = format!("{k:?}");
        assert!(!debug_output.contains("ab"));
        assert!(!debug_output.contains("171")); // 0xAB en decimal, por si acaso
        assert_eq!(debug_output, "VaultKey { bytes: \"<redacted>\" }");
    }

    #[test]
    fn from_slice_rejects_wrong_length() {
        let err = VaultKey::from_slice(&[0u8; 10]).unwrap_err();
        assert_eq!(err.got_len, 10);
    }

    #[test]
    fn from_slice_accepts_correct_length() {
        let bytes = [7u8; VAULT_KEY_LEN];
        let k = VaultKey::from_slice(&bytes).unwrap();
        assert_eq!(k.as_hex(), "07".repeat(VAULT_KEY_LEN));
    }

    #[test]
    fn creates_and_writes_to_a_new_encrypted_vault() {
        let path = temp_db_path("create-write");
        let conn = open_vault(&path, &key(0x11)).expect("debería crear el vault");

        conn.execute_batch(
            "CREATE TABLE test_patients (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO test_patients (id, name) VALUES ('p1', 'Paciente de prueba');",
        )
        .expect("debería poder escribir en el vault recién creado");

        let name: String = conn
            .query_row("SELECT name FROM test_patients WHERE id = 'p1'", [], |r| r.get(0))
            .expect("debería poder leer lo que se acaba de escribir");
        assert_eq!(name, "Paciente de prueba");
    }

    #[test]
    fn reports_a_real_sqlcipher_version_via_pragma() {
        let path = temp_db_path("cipher-version");
        let conn = open_vault(&path, &key(0x22)).unwrap();
        let version: String = conn
            .query_row("PRAGMA cipher_version;", [], |r| r.get(0))
            .expect("PRAGMA cipher_version debe responder: este binario debe estar enlazado con SQLCipher");
        assert!(!version.trim().is_empty());
        // Documentado también en docs/sqlcipher.md junto a las demás versiones exactas.
        assert!(version.starts_with("4."), "versión inesperada de SQLCipher: {version}");
    }

    #[test]
    fn closing_and_reopening_with_the_correct_key_preserves_data() {
        let path = temp_db_path("reopen-correct-key");
        {
            let conn = open_vault(&path, &key(0x33)).unwrap();
            conn.execute_batch(
                "CREATE TABLE t (v TEXT); INSERT INTO t (v) VALUES ('persistido');",
            )
            .unwrap();
        } // la conexión se cierra aquí (Drop)

        let conn = open_vault(&path, &key(0x33)).expect("debería reabrir con la misma clave");
        let value: String = conn.query_row("SELECT v FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(value, "persistido");
    }

    #[test]
    fn rejects_the_wrong_key_after_the_vault_has_real_data() {
        let path = temp_db_path("reject-wrong-key");
        {
            let conn = open_vault(&path, &key(0x44)).unwrap();
            conn.execute_batch("CREATE TABLE t (v TEXT); INSERT INTO t (v) VALUES ('secreto');")
                .unwrap();
        }

        let err = open_vault(&path, &key(0x55)).expect_err("una clave distinta debe fallar");
        assert!(matches!(err, VaultError::WrongKeyOrCorrupt));
    }

    #[test]
    fn encrypted_file_does_not_start_with_the_plain_sqlite_header() {
        let path = temp_db_path("no-plaintext-header");
        {
            let conn = open_vault(&path, &key(0x66)).unwrap();
            conn.execute_batch(
                "CREATE TABLE patients (id TEXT PRIMARY KEY, name TEXT);
                 INSERT INTO patients (id, name) VALUES ('p1', 'Nombre Sensible');",
            )
            .unwrap();
        }

        let bytes = fs::read(&path).unwrap();
        const SQLITE_PLAINTEXT_HEADER: &[u8] = b"SQLite format 3\0";
        assert!(bytes.len() >= SQLITE_PLAINTEXT_HEADER.len());
        assert_ne!(
            &bytes[..SQLITE_PLAINTEXT_HEADER.len()],
            SQLITE_PLAINTEXT_HEADER,
            "el archivo en disco no debe empezar con el encabezado estándar de SQLite sin cifrar"
        );

        // El nombre del paciente tampoco debe aparecer en texto plano en ningún
        // punto del archivo.
        let contains_plaintext_name = bytes
            .windows(b"Nombre Sensible".len())
            .any(|w| w == b"Nombre Sensible");
        assert!(!contains_plaintext_name, "el contenido cifrado no debe contener el dato en texto plano");
    }

    #[test]
    fn rejects_a_corrupt_or_invalid_file() {
        let path = temp_db_path("corrupt-file");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"esto no es una base de datos SQLite ni SQLCipher, son bytes cualquiera")
            .unwrap();
        drop(f);

        let err = open_vault(&path, &key(0x77)).expect_err("un archivo inválido debe fallar");
        assert!(matches!(err, VaultError::WrongKeyOrCorrupt));
    }

    #[test]
    fn empty_key_from_slice_of_zero_length_is_rejected() {
        let err = VaultKey::from_slice(&[]).unwrap_err();
        assert_eq!(err.got_len, 0);
    }
}
