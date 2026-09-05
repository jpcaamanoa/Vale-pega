import { invoke } from '@tauri-apps/api/core'
import type { GoalListItem } from '../goals/types'
import type { SessionListItem } from '../sessions/types'
import type {
  CloseEpisodeInput,
  EpisodeClinicalProfile,
  EpisodeClinicalProfileInput,
  EpisodeClosure,
  RevertClosureInput,
  TreatmentEpisode,
  TreatmentEpisodeInput,
} from './types'

export const treatmentEpisodesApi = {
  create: (input: TreatmentEpisodeInput) => invoke<TreatmentEpisode>('create_treatment_episode', { input }),

  get: (id: string) => invoke<TreatmentEpisode>('get_treatment_episode', { id }),

  list: (patientId: string) => invoke<TreatmentEpisode[]>('list_treatment_episodes', { patientId }),

  listArchived: (patientId: string) => invoke<TreatmentEpisode[]>('list_archived_treatment_episodes', { patientId }),

  setStatus: (id: string, status: 'activo' | 'pausado') => invoke<TreatmentEpisode>('set_treatment_episode_status', { id, status }),

  archive: (id: string) => invoke<void>('archive_treatment_episode', { id }),

  restore: (id: string) => invoke<TreatmentEpisode>('restore_treatment_episode', { id }),

  /** Sesiones futuras agendadas del proceso, todavía sin resolver — para construir el formulario de resolución obligatoria del cierre. */
  listUpcomingSessions: (episodeId: string) => invoke<SessionListItem[]>('list_upcoming_episode_sessions', { episodeId }),

  /** Sesiones históricas del proceso — para la vista de un proceso cerrado. */
  listSessions: (episodeId: string) => invoke<SessionListItem[]>('list_episode_sessions', { episodeId }),

  /** Objetivos relacionados con el proceso, con su estado actual en vivo. */
  listGoals: (episodeId: string) => invoke<GoalListItem[]>('list_episode_goals', { episodeId }),
}

export const episodeClosuresApi = {
  close: (episodeId: string, input: CloseEpisodeInput) => invoke<[EpisodeClosure, TreatmentEpisode]>('close_treatment_episode', { episodeId, input }),

  revert: (closureId: string, input: RevertClosureInput) => invoke<[EpisodeClosure, TreatmentEpisode]>('revert_episode_closure', { closureId, input }),

  /** `null` si el proceso no tiene un cierre vigente. */
  getActive: (episodeId: string) => invoke<EpisodeClosure | null>('get_active_episode_closure', { episodeId }),

  listHistory: (episodeId: string) => invoke<EpisodeClosure[]>('list_episode_closure_history', { episodeId }),
}

export const episodeClinicalProfileApi = {
  /** `null` significa que el proceso todavía no tiene antecedentes específicos registrados. */
  get: (episodeId: string) => invoke<EpisodeClinicalProfile | null>('get_episode_clinical_profile', { episodeId }),

  create: (episodeId: string, input: EpisodeClinicalProfileInput) => invoke<EpisodeClinicalProfile>('create_episode_clinical_profile', { episodeId, input }),

  update: (episodeId: string, input: EpisodeClinicalProfileInput) => invoke<EpisodeClinicalProfile>('update_episode_clinical_profile', { episodeId, input }),
}
