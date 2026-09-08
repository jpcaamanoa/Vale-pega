import { invoke } from '@tauri-apps/api/core'
import type {
  AdministrationInput,
  AdministrationUpdateInput,
  AssessmentAdministration,
  AssessmentAdministrationSummary,
  AssessmentInstrument,
  InstrumentInput,
} from './types'

export const assessmentsApi = {
  createInstrument: (input: InstrumentInput) => invoke<AssessmentInstrument>('create_assessment_instrument', { input }),

  getInstrument: (id: string) => invoke<AssessmentInstrument>('get_assessment_instrument', { id }),

  /** El catálogo completo — nunca filtrado por paciente, es compartido entre todos. */
  listInstruments: () => invoke<AssessmentInstrument[]>('list_assessment_instruments'),

  updateInstrument: (id: string, input: InstrumentInput) => invoke<AssessmentInstrument>('update_assessment_instrument', { id, input }),

  createAdministration: (input: AdministrationInput) => invoke<AssessmentAdministration>('create_assessment_administration', { input }),

  /** Contenido completo de una administración — solo se pide al abrirla explícitamente. */
  getAdministration: (id: string) => invoke<AssessmentAdministration>('get_assessment_administration', { id }),

  listAdministrations: (patientId: string) => invoke<AssessmentAdministrationSummary[]>('list_assessment_administrations', { patientId }),

  listArchivedAdministrations: (patientId: string) => invoke<AssessmentAdministrationSummary[]>('list_archived_assessment_administrations', { patientId }),

  /** Evolución longitudinal de un mismo instrumento — nunca mezcla instrumentos distintos. */
  listAdministrationsForInstrument: (patientId: string, instrumentId: string) =>
    invoke<AssessmentAdministrationSummary[]>('list_assessment_administrations_for_instrument', { patientId, instrumentId }),

  updateAdministration: (id: string, input: AdministrationUpdateInput) => invoke<AssessmentAdministration>('update_assessment_administration', { id, input }),

  archiveAdministration: (id: string) => invoke<void>('archive_assessment_administration', { id }),

  restoreAdministration: (id: string) => invoke<AssessmentAdministration>('restore_assessment_administration', { id }),
}
