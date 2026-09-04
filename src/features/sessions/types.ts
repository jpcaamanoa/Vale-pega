export type SessionStatus = 'programada' | 'realizada' | 'cancelada' | 'no_asistio'
export type SessionModality = 'presencial' | 'online' | 'telefonico'

export interface Session {
  id: string
  patientId: string
  appointmentId: string | null
  /** Opcional (Fase 9) — proceso terapéutico al que pertenece esta sesión. */
  episodeId: string | null
  sessionDate: string
  startTime: string | null
  durationMinutes: number | null
  modality: SessionModality | null
  status: SessionStatus
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/**
 * Fila de listado — deliberadamente sin contenido clínico (ver
 * `repositories::sessions::SessionListItem` en el backend). Solo lo
 * necesario para una lista cronológica y para saber si hay una nota
 * abierta, nunca su texto.
 */
export interface SessionListItem {
  id: string
  sessionDate: string
  startTime: string | null
  durationMinutes: number | null
  modality: SessionModality | null
  status: SessionStatus
  hasCurrentNote: boolean
  currentNoteIsLocked: boolean
}

export interface SessionInput {
  patientId: string
  appointmentId?: string | null
  /** Opcional (Fase 9) — vincular esta sesión a un proceso terapéutico. */
  episodeId?: string | null
  sessionDate: string
  startTime?: string | null
  durationMinutes?: number | null
  modality?: SessionModality | null
}

export interface SessionMetadataInput {
  sessionDate: string
  startTime?: string | null
  durationMinutes?: number | null
  modality?: SessionModality | null
  status: SessionStatus
}

export interface SessionNote {
  id: string
  sessionId: string
  content: string | null
  interventions: string | null
  homeworkTasks: string | null
  nextFocus: string | null
  version: number
  isLocked: boolean
  isCurrent: boolean
  closedAt: string | null
  supersededAt: string | null
  createdAt: string
  updatedAt: string
}

export const SESSION_STATUS_LABELS: Record<SessionStatus, string> = {
  programada: 'Programada',
  realizada: 'Realizada',
  cancelada: 'Cancelada',
  no_asistio: 'No asistió',
}

export const SESSION_MODALITY_LABELS: Record<SessionModality, string> = {
  presencial: 'Presencial',
  online: 'Online',
  telefonico: 'Telefónico',
}
