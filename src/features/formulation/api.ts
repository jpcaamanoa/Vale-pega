import { invoke } from '@tauri-apps/api/core'
import type { CaseFormulation, FormulationInput, FormulationSummary, FormulationVersion, NewVersionInput } from './types'

export const formulationsApi = {
  create: (input: FormulationInput) => invoke<[CaseFormulation, FormulationVersion]>('create_formulation', { input }),

  get: (id: string) => invoke<CaseFormulation>('get_formulation', { id }),

  /** `null` si el proceso todavía no tiene una formulación principal. */
  getByEpisode: (episodeId: string) => invoke<CaseFormulation | null>('get_formulation_by_episode', { episodeId }),

  /** Todas las formulaciones del paciente, de cualquiera de sus procesos — minimizado por IPC, nunca lleva el contenido completo de una versión. */
  list: (patientId: string) => invoke<FormulationSummary[]>('list_formulations', { patientId }),

  getCurrentVersion: (formulationId: string) => invoke<FormulationVersion>('get_current_formulation_version', { formulationId }),

  /** Contenido completo de una versión concreta — solo se pide al abrirla explícitamente desde el historial. */
  getVersion: (versionId: string) => invoke<FormulationVersion>('get_formulation_version', { versionId }),

  listVersions: (formulationId: string) => invoke<FormulationVersion[]>('list_formulation_versions', { formulationId }),

  /** "Actualizar formulación": crea una versión nueva, nunca sobrescribe la anterior. */
  createVersion: (formulationId: string, input: NewVersionInput) => invoke<[CaseFormulation, FormulationVersion]>('create_formulation_version', { formulationId, input }),
}
