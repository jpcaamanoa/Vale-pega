import { invoke } from '@tauri-apps/api/core'
import type { GeographicStatistics } from './types'

export const statisticsApi = {
  /**
   * `suppressSmallCategories` siempre en `false` desde esta pantalla privada
   * (solo la psicóloga tratante la ve, sobre sus propios pacientes): agrupar
   * en "Otras" las categorías con pocos pacientes tiene sentido para un
   * informe/exportación compartible con terceros, no aquí — ver
   * `services::patients::geographic_statistics` en el backend.
   */
  geographic: (includeArchived: boolean) =>
    invoke<GeographicStatistics>('get_geographic_statistics', { includeArchived, suppressSmallCategories: false }),
}
