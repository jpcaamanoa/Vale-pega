import { invoke } from '@tauri-apps/api/core'
import type { Session, SessionInput, SessionListItem, SessionMetadataInput, SessionNote } from './types'

export const sessionsApi = {
  create: (input: SessionInput) => invoke<Session>('create_session', { input }),

  get: (id: string) => invoke<Session>('get_session', { id }),

  getForAppointment: (appointmentId: string) => invoke<Session | null>('get_session_for_appointment', { appointmentId }),

  list: (patientId: string) => invoke<SessionListItem[]>('list_sessions', { patientId }),

  listArchived: (patientId: string) => invoke<SessionListItem[]>('list_archived_sessions', { patientId }),

  updateMetadata: (id: string, input: SessionMetadataInput) => invoke<Session>('update_session_metadata', { id, input }),

  archive: (id: string) => invoke<void>('archive_session', { id }),

  restore: (id: string) => invoke<Session>('restore_session', { id }),

  getCurrentNote: (sessionId: string) => invoke<SessionNote>('get_current_note', { sessionId }),

  listNoteHistory: (sessionId: string) => invoke<SessionNote[]>('list_note_history', { sessionId }),

  autosaveDraft: (
    sessionId: string,
    fields: { content?: string | null; interventions?: string | null; homeworkTasks?: string | null; nextFocus?: string | null },
  ) =>
    invoke<void>('autosave_note_draft', {
      sessionId,
      content: fields.content ?? null,
      interventions: fields.interventions ?? null,
      homeworkTasks: fields.homeworkTasks ?? null,
      nextFocus: fields.nextFocus ?? null,
    }),

  closeCurrentNote: (sessionId: string) => invoke<SessionNote>('close_current_note', { sessionId }),

  createNewNoteVersion: (sessionId: string) => invoke<SessionNote>('create_new_note_version', { sessionId }),

  /** Conteo global — para el bloque "Resumen" del Dashboard (Fase 8). */
  thisMonthCount: () => invoke<number>('get_sessions_this_month_count'),
}
