import { invoke } from '@tauri-apps/api/core'
import type { Patient, PatientHardDeleteScope, PatientInput, PatientListItem } from './types'

export const patientsApi = {
  create: (input: PatientInput) => invoke<Patient>('create_patient', { input }),

  get: (id: string) => invoke<Patient>('get_patient', { id }),

  list: (search?: string) => invoke<PatientListItem[]>('list_patients', { search: search || null }),

  listArchived: (search?: string) =>
    invoke<PatientListItem[]>('list_archived_patients', { search: search || null }),

  update: (id: string, input: PatientInput) => invoke<Patient>('update_patient', { id, input }),

  archive: (id: string) => invoke<void>('archive_patient', { id }),

  restore: (id: string) => invoke<void>('restore_patient', { id }),

  /** Resumen de solo lectura para el modal de confirmación de "Eliminar permanentemente" — nunca
   * borra nada. */
  getHardDeleteScope: (id: string) => invoke<PatientHardDeleteScope>('get_patient_hard_delete_scope', { id }),

  /** Borrado físico e irreversible — solo alcanzable desde un paciente ya archivado, tras
   * confirmar escribiendo "ELIMINAR". */
  hardDelete: (id: string) => invoke<void>('hard_delete_patient', { id }),
}
