//! Contenedor físico `.cclinbackup`: un ZIP sin comprimir (`Stored`) — el
//! contenido ya es SQLCipher cifrado (alta entropía), así que comprimirlo
//! no ahorra espacio de forma apreciable y solo costaría tiempo de CPU sin
//! beneficio real. El crate `zip` (versión consolidada, sin red ni
//! telemetría) se usa exclusivamente para empaquetar/desempaquetar bytes —
//! nunca como mecanismo de cifrado: la confidencialidad del contenido la
//! sigue dando SQLCipher/AES-GCM, no el contenedor.
//!
//! Funciones puras sobre rutas de archivo. Sin conocimiento de vault, de
//! manifest, ni de Tauri.

use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug)]
pub enum ArchiveError {
    Io(io::Error),
    Zip(zip::result::ZipError),
    /// Una entrada del ZIP intenta escribir fuera del directorio de
    /// destino (p. ej. `../../etc/passwd`) — rechazado explícitamente antes
    /// de escribir un solo byte, nunca confiando en que el nombre de una
    /// entrada de ZIP sea una ruta relativa segura.
    UnsafeEntryPath(String),
}
impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveError::Io(e) => write!(f, "error de E/S: {e}"),
            ArchiveError::Zip(e) => write!(f, "error de contenedor: {e}"),
            ArchiveError::UnsafeEntryPath(p) => write!(f, "ruta interna del respaldo no es segura: {p}"),
        }
    }
}
impl std::error::Error for ArchiveError {}
impl From<io::Error> for ArchiveError {
    fn from(e: io::Error) -> Self {
        ArchiveError::Io(e)
    }
}
impl From<zip::result::ZipError> for ArchiveError {
    fn from(e: zip::result::ZipError) -> Self {
        ArchiveError::Zip(e)
    }
}

/// Calcula el SHA-256 de un archivo leyéndolo en bloques (nunca carga el
/// archivo completo en memoria — `vault.db` puede crecer con el tiempo).
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Empaqueta `entries` (ruta relativa dentro del contenedor, ruta real en
/// disco) en un único archivo ZIP sin comprimir en `dest`. `dest` no debe
/// existir todavía — igual que `VACUUM INTO`, no sobrescribe en silencio.
pub fn write_container(dest: &Path, entries: &[(&str, &Path)]) -> Result<(), ArchiveError> {
    let file = File::create_new(dest)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for (entry_name, real_path) in entries {
        zip.start_file(*entry_name, options)?;
        let mut source = BufReader::new(File::open(real_path)?);
        io::copy(&mut source, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

/// Extrae todas las entradas de `archive` hacia `dest_dir` (que debe existir
/// y estar vacío — lo crea/limpia quien llama). Devuelve las rutas
/// relativas efectivamente extraídas, para que quien llama pueda
/// contrastarlas contra el manifest sin volver a tocar el filesystem.
pub fn extract_container(archive: &Path, dest_dir: &Path) -> Result<Vec<String>, ArchiveError> {
    let file = File::open(archive)?;
    let mut zip = ZipArchive::new(BufReader::new(file))?;
    let mut extracted = Vec::with_capacity(zip.len());

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(ArchiveError::UnsafeEntryPath(entry.name().to_string()));
        };
        let out_path = dest_dir.join(&relative);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&out_path)?;
        io::copy(&mut entry, &mut out)?;
        out.flush()?;
        extracted.push(relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cc-archive-test-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_and_extracts_a_round_trip() {
        let dir = temp_dir("round-trip");
        let src_a = dir.join("a.txt");
        let src_b = dir.join("b.txt");
        std::fs::write(&src_a, b"contenido A").unwrap();
        std::fs::write(&src_b, b"contenido B, un poco mas largo").unwrap();

        let container = dir.join("out.zip");
        write_container(&container, &[("a.txt", &src_a), ("nested/b.txt", &src_b)]).unwrap();

        let dest = dir.join("extracted");
        std::fs::create_dir_all(&dest).unwrap();
        let mut extracted = extract_container(&container, &dest).unwrap();
        extracted.sort();
        assert_eq!(extracted, vec!["a.txt", "nested/b.txt"]);

        assert_eq!(std::fs::read(dest.join("a.txt")).unwrap(), b"contenido A");
        assert_eq!(std::fs::read(dest.join("nested/b.txt")).unwrap(), b"contenido B, un poco mas largo");
    }

    #[test]
    fn refuses_to_overwrite_an_existing_destination() {
        let dir = temp_dir("no-overwrite");
        let container = dir.join("out.zip");
        std::fs::write(&container, b"ya existe").unwrap();

        let src = dir.join("a.txt");
        std::fs::write(&src, b"x").unwrap();

        let err = write_container(&container, &[("a.txt", &src)]).unwrap_err();
        assert!(matches!(err, ArchiveError::Io(_)));
    }

    #[test]
    fn sha256_is_stable_and_detects_changes() {
        let dir = temp_dir("sha256");
        let path = dir.join("f.bin");
        std::fs::write(&path, b"contenido original").unwrap();
        let hash1 = sha256_file(&path).unwrap();

        let hash1_again = sha256_file(&path).unwrap();
        assert_eq!(hash1, hash1_again);

        std::fs::write(&path, b"contenido modificado").unwrap();
        let hash2 = sha256_file(&path).unwrap();
        assert_ne!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn compression_method_is_stored_not_deflated() {
        // Contenido muy compresible (todo ceros): si se usara Deflate el
        // archivo resultante sería mucho más chico que el contenido
        // original. Con Stored, el tamaño del ZIP debe ser al menos el
        // tamaño del contenido (más el overhead fijo del formato).
        let dir = temp_dir("stored-not-deflated");
        let src = dir.join("zeros.bin");
        std::fs::write(&src, vec![0u8; 100_000]).unwrap();

        let container = dir.join("out.zip");
        write_container(&container, &[("zeros.bin", &src)]).unwrap();

        let container_size = std::fs::metadata(&container).unwrap().len();
        assert!(container_size >= 100_000, "el contenedor no debería ser más chico que el contenido con Stored");
    }
}
