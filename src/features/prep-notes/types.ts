export type PrepNoteStatus = 'pendiente' | 'abordado' | 'descartado'

/**
 * "Quiero acordarme de esto la próxima vez que vea a este paciente" —
 * distinto de `session_notes.nextFocus`: es un registro operativo con su
 * propio ciclo de vida, no un campo dentro de una nota clínica versionada.
 * Ver `docs/session-continuity.md`.
 */
export interface PatientPrepNote {
  id: string
  patientId: string
  originSessionId: string | null
  content: string
  status: PrepNoteStatus
  createdAt: string
  updatedAt: string
}

export interface PrepNoteInput {
  patientId: string
  originSessionId?: string | null
  content: string
}

export const PREP_NOTE_STATUS_LABELS: Record<PrepNoteStatus, string> = {
  pendiente: 'Pendiente',
  abordado: 'Abordado',
  descartado: 'Descartado',
}
