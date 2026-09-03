//! Catálogo cerrado de regiones y comunas de Chile, más el valor reservado
//! "Extranjero" para pacientes residentes fuera del país. Fuente única de
//! verdad compartida con el frontend: este módulo solo lee
//! `src/data/chile-geo.json` (incluido en el binario en tiempo de
//! compilación con `include_str!`) — el mismo archivo que
//! `src/features/patients/geo.ts` importa directamente. No existe una
//! segunda copia de los 346 nombres de comuna en ningún lugar del código
//! Rust: si el archivo cambiara, ambos lados verían exactamente el mismo
//! contenido sin necesidad de sincronizarlos manualmente.
//!
//! `test_catalog_matches_the_expected_shape_of_chile` (abajo) es la
//! salvaguarda de que ese único archivo compartido sigue teniendo la forma
//! esperada (16 regiones, 346 comunas, sin duplicados) — no detecta una
//! divergencia entre dos copias (no existen dos copias), detecta corrupción
//! o un error de edición del archivo fuente.

use std::sync::OnceLock;

use serde::Deserialize;

const CATALOG_JSON: &str = include_str!("../../src/data/chile-geo.json");

/// Valor interno estable para pacientes residentes fuera de Chile. No es
/// una región del catálogo geográfico — se trata como un caso reservado
/// aparte, tanto en la validación de este módulo como en el frontend
/// (`EXTRANJERO` en `src/features/patients/geo.ts`, el mismo valor literal).
pub const EXTRANJERO: &str = "Extranjero";

#[derive(Debug, Deserialize)]
struct CatalogFile {
    regions: Vec<RegionEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RegionEntry {
    pub name: String,
    pub communes: Vec<String>,
}

fn catalog() -> &'static Vec<RegionEntry> {
    static CATALOG: OnceLock<Vec<RegionEntry>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let parsed: CatalogFile =
            serde_json::from_str(CATALOG_JSON).expect("src/data/chile-geo.json debe ser JSON válido — ver geo::tests");
        parsed.regions
    })
}

/// `true` si `name` es exactamente el nombre de una de las 16 regiones del
/// catálogo. `EXTRANJERO` no cuenta como región del catálogo — se valida
/// aparte en `services::patients`.
pub fn is_known_region(name: &str) -> bool {
    catalog().iter().any(|r| r.name == name)
}

/// `true` si `commune` pertenece exactamente a `region` según el catálogo.
pub fn commune_belongs_to_region(region: &str, commune: &str) -> bool {
    catalog().iter().find(|r| r.name == region).is_some_and(|r| r.communes.iter().any(|c| c == commune))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_matches_the_expected_shape_of_chile() {
        let regions = catalog();
        assert_eq!(regions.len(), 16, "Chile tiene 16 regiones");

        let total_communes: usize = regions.iter().map(|r| r.communes.len()).sum();
        assert_eq!(total_communes, 346, "Chile tiene 346 comunas");

        let mut all_communes: Vec<&str> = regions.iter().flat_map(|r| r.communes.iter().map(|c| c.as_str())).collect();
        all_communes.sort_unstable();
        let mut deduped = all_communes.clone();
        deduped.dedup();
        assert_eq!(all_communes.len(), deduped.len(), "ninguna comuna debería repetirse entre regiones");

        let mut region_names: Vec<&str> = regions.iter().map(|r| r.name.as_str()).collect();
        region_names.sort_unstable();
        let mut deduped_regions = region_names.clone();
        deduped_regions.dedup();
        assert_eq!(region_names.len(), deduped_regions.len(), "ningún nombre de región debería repetirse");

        // Ninguna región ni comuna debería tener espacios sobrantes ni
        // quedar vacía — señal de un problema de limpieza del archivo
        // fuente, no algo que deba corregirse en tiempo de ejecución.
        for r in regions {
            assert_eq!(r.name.trim(), r.name, "nombre de región con espacios sobrantes: {:?}", r.name);
            assert!(!r.name.is_empty());
            for c in &r.communes {
                assert_eq!(c.trim(), c, "nombre de comuna con espacios sobrantes: {:?}", c);
                assert!(!c.is_empty());
            }
        }
    }

    #[test]
    fn extranjero_is_not_a_catalog_region() {
        assert!(!is_known_region(EXTRANJERO));
    }

    #[test]
    fn recognizes_a_real_region_and_rejects_an_unknown_one() {
        assert!(is_known_region("Región Metropolitana de Santiago"));
        assert!(!is_known_region("Región Inventada"));
    }

    #[test]
    fn commune_membership_is_checked_against_its_own_region() {
        assert!(commune_belongs_to_region("Región Metropolitana de Santiago", "Ñuñoa"));
        // Quillota es una comuna real, pero de la Región de Valparaíso, no de la RM.
        assert!(!commune_belongs_to_region("Región Metropolitana de Santiago", "Quillota"));
        assert!(commune_belongs_to_region("Región de Valparaíso", "Quillota"));
    }

    #[test]
    fn nuble_was_correctly_split_from_biobio() {
        // Región de Ñuble (creada en 2018) y Región del Biobío deben ser
        // regiones separadas, cada una con sus propias comunas — no debe
        // quedar rastro del esquema pre-2018 donde Ñuble era una provincia
        // dentro de una única región del Biobío.
        assert!(commune_belongs_to_region("Región de Ñuble", "Chillán"));
        assert!(!commune_belongs_to_region("Región del Biobío", "Chillán"));
        assert!(commune_belongs_to_region("Región del Biobío", "Concepción"));
        assert!(!commune_belongs_to_region("Región de Ñuble", "Concepción"));
    }
}
