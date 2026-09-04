import { invoke } from '@tauri-apps/api/core'
import type { PatientPrepNote, PrepNoteInput, PrepNoteStatus } from './types'

export const prepNotesApi = {
  create: (input: PrepNoteInput) => invoke<PatientPrepNote>('create_prep_note', { input }),

  get: (id: string) => invoke<PatientPrepNote>('get_prep_note', { id }),

  list: (patientId: string) => invoke<PatientPrepNote[]>('list_prep_notes', { patientId }),

  listPending: (patientId: string) => invoke<PatientPrepNote[]>('list_pending_prep_notes', { patientId }),

  update: (id: string, content: string) => invoke<PatientPrepNote>('update_prep_note', { id, content }),

  setStatus: (id: string, status: PrepNoteStatus) => invoke<PatientPrepNote>('set_prep_note_status', { id, status }),
}
