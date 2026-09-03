import { invoke } from '@tauri-apps/api/core'
import type { GeographicStatistics } from './types'

export const statisticsApi = {
  geographic: (includeArchived: boolean) =>
    invoke<GeographicStatistics>('get_geographic_statistics', { includeArchived }),
}
