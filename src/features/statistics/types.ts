/** Una categoría (nombre de región o de comuna) ya agrupada — nunca
 * contiene ningún dato identificable de un paciente individual. Las
 * categorías con menos de 3 pacientes llegan agrupadas en "Otras" (ver
 * `services::patients::group_small_categories` en el backend). */
export interface GeoDistributionItem {
  label: string
  count: number
}

export interface GeographicStatistics {
  withLocation: number
  withoutLocation: number
  byRegion: GeoDistributionItem[]
  byCommune: GeoDistributionItem[]
}
