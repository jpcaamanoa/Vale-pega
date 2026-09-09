//! Registro de temporales descifrados de Documentos (Fase 16, Bloque 21 de la aprobación).
//!
//! Un temporal plaintext solo existe cuando la Opción A de visualización lo exige (abrir un PDF
//! u otro formato con la aplicación por defecto del sistema, ver `commands::documents::
//! open_document_externally`) — nunca para imágenes, que se muestran en memoria sin tocar disco.
//! Vive siempre en `std::env::temp_dir()` (nunca dentro de `vault/files`), con un nombre opaco
//! (UUID nuevo, nunca el nombre real del documento) y un prefijo reconocible
//! (`TEMP_FILE_PREFIX`) que permite un barrido de arranque si un crash impidió limpiarlo.
//!
//! Este módulo vive fuera de `security/*` — no necesita ningún secreto del vault, solo rastrea
//! rutas de archivos ya descifrados por `services::documents::get_document_content`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const TEMP_FILE_PREFIX: &str = "cuaderno-clinico-doc-";

/// Estado compartido (pensado para vivir como `tauri::State`, igual que `VaultSession`):
/// qué temporales de documentos existen ahora mismo, para poder limpiarlos todos de una vez al
/// bloquear el vault o al cerrar la aplicación (Bloque 21 de la aprobación).
pub struct DocumentTempRegistry {
    paths: Mutex<HashSet<PathBuf>>,
}

impl Default for DocumentTempRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentTempRegistry {
    pub fn new() -> Self {
        Self { paths: Mutex::new(HashSet::new()) }
    }

    /// Genera una ruta de temporal nueva con nombre opaco, la registra, y la devuelve — el
    /// llamador es quien realmente escribe el contenido descifrado ahí.
    pub fn allocate(&self, extension_hint: Option<&str>) -> PathBuf {
        let filename = match extension_hint.filter(|e| !e.is_empty() && e.len() <= 10 && e.chars().all(|c| c.is_ascii_alphanumeric())) {
            Some(ext) => format!("{TEMP_FILE_PREFIX}{}.{ext}", uuid::Uuid::new_v4()),
            None => format!("{TEMP_FILE_PREFIX}{}", uuid::Uuid::new_v4()),
        };
        let path = std::env::temp_dir().join(filename);
        self.paths.lock().unwrap().insert(path.clone());
        path
    }

    /// Borra y desregistra un temporal puntual — pensado para cuando la propia aplicación puede
    /// detectar que ya no lo necesita (poco frecuente: normalmente se limpia recién al bloquear/
    /// cerrar, ver `cleanup_all`, porque no siempre es detectable cuándo la aplicación externa
    /// terminó de usarlo). Ningún comando de esta fase lo llama todavía — queda disponible para
    /// una futura mejora de UX (p. ej. detectar el cierre del visor externo en plataformas que lo
    /// permitan) sin tener que rediseñar el registro.
    #[allow(dead_code)]
    pub fn remove(&self, path: &Path) {
        let _ = std::fs::remove_file(path);
        self.paths.lock().unwrap().remove(path);
    }

    /// Limpia todos los temporales registrados ahora mismo — llamado al bloquear el vault
    /// (manual o automático) y, cuando es posible, al cerrar la aplicación (Bloque 21 de la
    /// aprobación). Best-effort: un archivo que ya no existe, o que el SO todavía tiene abierto
    /// en la aplicación externa, no hace fallar la limpieza de los demás.
    pub fn cleanup_all(&self) {
        let mut paths = self.paths.lock().unwrap();
        for path in paths.drain() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Barrido de arranque (Bloque 21: "limpiarse en el siguiente arranque si un crash impidió
/// hacerlo") — busca, dentro del directorio temporal del sistema, cualquier archivo con el
/// prefijo reconocible de esta aplicación y lo borra. No depende del registro en memoria (que se
/// pierde entre reinicios) — solo del nombre del archivo. Nunca toca ningún archivo ajeno: el
/// prefijo es lo bastante específico para no colisionar con nada más.
pub fn sweep_stale_temp_files() {
    let dir = std::env::temp_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if name.starts_with(TEMP_FILE_PREFIX) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_produces_a_path_inside_the_system_temp_dir_with_the_expected_prefix() {
        let registry = DocumentTempRegistry::new();
        let path = registry.allocate(Some("pdf"));
        assert!(path.starts_with(std::env::temp_dir()));
        assert!(path.file_name().unwrap().to_str().unwrap().starts_with(TEMP_FILE_PREFIX));
        assert!(path.to_str().unwrap().ends_with(".pdf"));
    }

    #[test]
    fn allocate_without_extension_hint_still_produces_a_valid_path() {
        let registry = DocumentTempRegistry::new();
        let path = registry.allocate(None);
        assert!(path.file_name().unwrap().to_str().unwrap().starts_with(TEMP_FILE_PREFIX));
    }

    #[test]
    fn rejects_a_malicious_extension_hint_and_falls_back_to_no_extension() {
        let registry = DocumentTempRegistry::new();
        let path = registry.allocate(Some("pdf/../../etc"));
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(!name.contains('/'), "un hint de extensión con separadores nunca debe alcanzar el nombre de archivo");
    }

    #[test]
    fn cleanup_all_removes_every_registered_file() {
        let registry = DocumentTempRegistry::new();
        let a = registry.allocate(Some("pdf"));
        let b = registry.allocate(Some("png"));
        std::fs::write(&a, b"contenido a").unwrap();
        std::fs::write(&b, b"contenido b").unwrap();

        registry.cleanup_all();

        assert!(!a.exists());
        assert!(!b.exists());
    }

    #[test]
    fn cleanup_all_does_not_panic_if_a_file_was_already_removed_externally() {
        let registry = DocumentTempRegistry::new();
        let a = registry.allocate(Some("pdf"));
        std::fs::write(&a, b"contenido").unwrap();
        std::fs::remove_file(&a).unwrap();

        registry.cleanup_all();
    }

    #[test]
    fn remove_deletes_and_deregisters_a_single_file() {
        let registry = DocumentTempRegistry::new();
        let a = registry.allocate(Some("pdf"));
        let b = registry.allocate(Some("png"));
        std::fs::write(&a, b"contenido a").unwrap();
        std::fs::write(&b, b"contenido b").unwrap();

        registry.remove(&a);
        assert!(!a.exists());
        assert!(b.exists());

        registry.cleanup_all();
        assert!(!b.exists());
    }

    #[test]
    fn sweep_stale_temp_files_removes_files_with_the_recognizable_prefix_and_leaves_others_untouched() {
        let marker = std::env::temp_dir().join(format!("{TEMP_FILE_PREFIX}sweep-test-{}", std::process::id()));
        std::fs::write(&marker, b"restos de un crash anterior").unwrap();
        let unrelated = std::env::temp_dir().join(format!("cc-unrelated-file-{}.txt", std::process::id()));
        std::fs::write(&unrelated, b"otro archivo cualquiera, no debe tocarse").unwrap();

        sweep_stale_temp_files();

        assert!(!marker.exists(), "un temporal huérfano de un crash anterior debe limpiarse al arrancar");
        assert!(unrelated.exists(), "un archivo ajeno sin el prefijo reconocible nunca debe tocarse");
        let _ = std::fs::remove_file(&unrelated);
    }
}
