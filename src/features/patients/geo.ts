// Catálogo cerrado de regiones y comunas de Chile — misma fuente única que
// lee el backend (`src-tauri/src/geo.rs`, vía `include_str!`). Este archivo
// no mantiene una segunda copia de los nombres: importa directamente
// `src/data/chile-geo.json`, así que ambos lados siempre ven exactamente el
// mismo contenido.
import chileGeoData from '../../data/chile-geo.json'

export interface RegionEntry {
  name: string
  communes: string[]
}

interface ChileGeoCatalog {
  regions: RegionEntry[]
}

const catalog = chileGeoData as ChileGeoCatalog

/** Valor reservado para pacientes residentes fuera de Chile — mismo valor
 * literal que `geo::EXTRANJERO` en el backend. No es una región del
 * catálogo: no tiene comunas asociadas. */
export const EXTRANJERO = 'Extranjero'

/** Opciones para el `<Select>` de región: las 16 regiones de Chile más
 * "Extranjero" al final. */
export const REGION_OPTIONS: string[] = [...catalog.regions.map((r) => r.name), EXTRANJERO]

/** Comunas de una región, o lista vacía si la región no está informada o es
 * "Extranjero" (que nunca tiene comuna). */
export function communesForRegion(region: string | null | undefined): string[] {
  if (!region || region === EXTRANJERO) return []
  return catalog.regions.find((r) => r.name === region)?.communes ?? []
}
