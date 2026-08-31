import { invoke } from '@tauri-apps/api/core'
import type { Patient, PatientInput, PatientListItem } from './types'

export const patientsApi = {
  create: (input: PatientInput) => invoke<Patient>('create_patient', { input }),

  get: (id: string) => invoke<Patient>('get_patient', { id }),

  list: (search?: string) => invoke<PatientListItem[]>('list_patients', { search: search || null }),

  listArchived: (search?: string) =>
    invoke<PatientListItem[]>('list_archived_patients', { search: search || null }),

  update: (id: string, input: PatientInput) => invoke<Patient>('update_patient', { id, input }),

  archive: (id: string) => invoke<void>('archive_patient', { id }),

  restore: (id: string) => invoke<void>('restore_patient', { id }),
}
