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
use uuid::Uuid;
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
///
/// BACKUP-1 (hardening pre-RC, Fase 17): el contenido se construye
/// primero en un archivo **temporal hermano** de `dest` — mismo
/// directorio, y por lo tanto garantizado en el mismo filesystem/volumen,
/// incluso cuando `dest` está en un pendrive USB o una unidad de red — y
/// solo se promueve a `dest` mediante `rename` una vez que el ZIP se
/// escribió y sincronizó a disco por completo. Así, un fallo en cualquier
/// punto de la construcción (E/S, un archivo fuente que desaparece, etc.)
/// nunca deja en `dest` un archivo parcial que aparente ser un
/// `.cclinbackup` exitoso: en el peor caso, lo que puede quedar huérfano
/// es el temporal (con un nombre y extensión claramente distintos de
/// `dest`, nunca el `.cclinbackup` final), y este código intenta
/// eliminarlo de todas formas ante cualquier error.
///
/// Se investigó si existe en `std` (u otra dependencia ya presente en el
/// proyecto) una primitiva de "promover solo si `dest` no existe todavía"
/// verdaderamente atómica en Windows, macOS y Linux a la vez. No la hay:
/// `std::fs::rename` reemplaza un destino existente en ambas plataformas
/// en vez de fallar, y `std::fs::hard_link` sí falla si el destino ya
/// existe pero exige que el filesystem soporte hard links — cosa que
/// FAT32 y exFAT (los formatos más comunes en pendrives USB, el destino
/// más habitual de un respaldo) no soportan, así que basar la promoción
/// en hard links rompería backups a USB en el caso común en vez de
/// arreglar el caso raro. Por eso se mantiene una comprobación defensiva
/// de `dest.exists()` inmediatamente antes de la promoción (además de la
/// que ya hace quien llama a esta función) y un `rename` final: queda una
/// ventana de carrera residual, extremadamente pequeña, entre esa
/// comprobación y el `rename` — ver `docs/backup-restore.md` para el
/// detalle honesto de este límite conocido, deliberadamente no resuelto
/// con criptografía propia, código `unsafe` ni una dependencia nueva.
pub fn write_container(dest: &Path, entries: &[(&str, &Path)]) -> Result<(), ArchiveError> {
    if dest.exists() {
        return Err(ArchiveError::Io(io::Error::from(io::ErrorKind::AlreadyExists)));
    }
    let parent = dest.parent().ok_or_else(|| {
        ArchiveError::Io(io::Error::new(io::ErrorKind::InvalidInput, "el destino del contenedor no tiene directorio padre"))
    })?;
    let tmp_name = match dest.file_name() {
        Some(name) => format!(".{}.tmp-{}", name.to_string_lossy(), Uuid::new_v4()),
        None => format!(".cclinbackup.tmp-{}", Uuid::new_v4()),
    };
    let tmp_path = parent.join(tmp_name);

    if let Err(e) = write_container_to(&tmp_path, entries) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    // Comprobación defensiva inmediatamente antes de la promoción (ver la
    // documentación de la función sobre la carrera residual que esto no
    // elimina del todo, solo reduce).
    if dest.exists() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(ArchiveError::Io(io::Error::from(io::ErrorKind::AlreadyExists)));
    }
    std::fs::rename(&tmp_path, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        ArchiveError::Io(e)
    })
}

/// Escribe el ZIP completo en `tmp_path` (que no debe existir todavía) y
/// sincroniza su contenido a disco (`sync_all`) antes de devolver el
/// control — para que, en el momento en que `write_container` promueve
/// este archivo a `dest` mediante `rename`, sus bytes ya estén
/// durablemente en disco y no solo en el caché de escritura del SO.
fn write_container_to(tmp_path: &Path, entries: &[(&str, &Path)]) -> Result<(), ArchiveError> {
    let file = File::create_new(tmp_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for (entry_name, real_path) in entries {
        zip.start_file(*entry_name, options)?;
        let mut source = BufReader::new(File::open(real_path)?);
        io::copy(&mut source, &mut zip)?;
    }
    let file = zip.finish()?;
    file.sync_all()?;
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

    /// BACKUP-1: si una de las fuentes desaparece a mitad de la construcción del contenedor
    /// (aquí, simplemente nunca existió), `dest` nunca debe llegar a existir — a diferencia del
    /// comportamiento anterior a Fase 17, donde `dest` se creaba de entrada con `create_new` y
    /// quedaba en disco, a medio escribir, si una entrada posterior fallaba.
    #[test]
    fn write_container_never_leaves_a_partial_file_at_dest_when_a_source_is_missing() {
        let dir = temp_dir("no-partial-dest-on-failure");
        let src_ok = dir.join("a.txt");
        std::fs::write(&src_ok, b"contenido A").unwrap();
        let src_missing = dir.join("no-existe.txt");

        let dest = dir.join("respaldo.cclinbackup");
        let err = write_container(&dest, &[("a.txt", &src_ok), ("b.txt", &src_missing)]).unwrap_err();
        assert!(matches!(err, ArchiveError::Io(_)));
        assert!(!dest.exists(), "dest_path nunca debe aparecer cuando la construcción del contenedor falla");

        // Tampoco debe quedar un temporal huérfano con un nombre que pudiera confundirse con un
        // `.cclinbackup` válido: el único requisito exigido es que no aparezca en `dest`, pero se
        // verifica además que la limpieza best-effort del temporal efectivamente ocurrió en este
        // caso (fallo detectado de forma síncrona, sin crash de por medio).
        let leftover: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftover.is_empty(), "no debe quedar un temporal huérfano tras un fallo síncrono: {leftover:?}");
    }

    /// Tras una construcción exitosa, el temporal hermano usado internamente debe haber sido
    /// promovido a `dest` (mediante `rename`) y no debe quedar ningún archivo `.tmp-*` adicional
    /// en el directorio.
    #[test]
    fn write_container_leaves_no_stray_temporary_file_after_success() {
        let dir = temp_dir("no-stray-tmp-after-success");
        let src = dir.join("a.txt");
        std::fs::write(&src, b"contenido A").unwrap();

        let dest = dir.join("respaldo.cclinbackup");
        write_container(&dest, &[("a.txt", &src)]).unwrap();
        assert!(dest.exists());

        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(!entries.iter().any(|name| name.contains(".tmp-")), "no debe quedar ningún temporal tras una construcción exitosa: {entries:?}");
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
