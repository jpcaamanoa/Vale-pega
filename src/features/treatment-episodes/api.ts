import { invoke } from '@tauri-apps/api/core'
import type { EpisodeClinicalProfile, EpisodeClinicalProfileInput, TreatmentEpisode, TreatmentEpisodeInput } from './types'

export const treatmentEpisodesApi = {
  create: (input: TreatmentEpisodeInput) => invoke<TreatmentEpisode>('create_treatment_episode', { input }),

  get: (id: string) => invoke<TreatmentEpisode>('get_treatment_episode', { id }),

  list: (patientId: string) => invoke<TreatmentEpisode[]>('list_treatment_episodes', { patientId }),

  listArchived: (patientId: string) => invoke<TreatmentEpisode[]>('list_archived_treatment_episodes', { patientId }),

  setStatus: (id: string, status: 'activo' | 'pausado') => invoke<TreatmentEpisode>('set_treatment_episode_status', { id, status }),

  archive: (id: string) => invoke<void>('archive_treatment_episode', { id }),

  restore: (id: string) => invoke<TreatmentEpisode>('restore_treatment_episode', { id }),
}

export const episodeClinicalProfileApi = {
  /** `null` significa que el proceso todavía no tiene antecedentes específicos registrados. */
  get: (episodeId: string) => invoke<EpisodeClinicalProfile | null>('get_episode_clinical_profile', { episodeId }),

  create: (episodeId: string, input: EpisodeClinicalProfileInput) => invoke<EpisodeClinicalProfile>('create_episode_clinical_profile', { episodeId, input }),

  update: (episodeId: string, input: EpisodeClinicalProfileInput) => invoke<EpisodeClinicalProfile>('update_episode_clinical_profile', { episodeId, input }),
}
