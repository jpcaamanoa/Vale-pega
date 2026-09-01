import { invoke } from '@tauri-apps/api/core'
import type { ClinicalProfile, ClinicalProfileInput } from './types'

export const clinicalProfileApi = {
  /** `null` significa que el paciente todavía no tiene antecedentes registrados. */
  get: (patientId: string) => invoke<ClinicalProfile | null>('get_clinical_profile', { patientId }),

  create: (patientId: string, input: ClinicalProfileInput) => invoke<ClinicalProfile>('create_clinical_profile', { patientId, input }),

  update: (patientId: string, input: ClinicalProfileInput) => invoke<ClinicalProfile>('update_clinical_profile', { patientId, input }),
}
