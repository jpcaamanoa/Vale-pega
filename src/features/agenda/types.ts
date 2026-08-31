export type AppointmentStatus = 'programada' | 'cancelada'
export type AppointmentModality = 'presencial' | 'online' | 'telefonico'

/**
 * Estado devuelto por el backend después de cada mutación de una cita: qué
 * pasó al intentar reflejar el cambio en Google Calendar. Nunca implica que
 * la mutación local haya fallado — esa ya se guardó antes de intentar
 * sincronizar (ver `calendar::sync` en el backend).
 */
export type SyncOutcome =
  | { kind: 'not_connected' }
  | { kind: 'skipped' }
  | { kind: 'synced' }
  | { kind: 'disconnected' }
  | { kind: 'failed'; message: string }

/** Ficha completa de una cita. */
export interface Appointment {
  id: string
  patientId: string | null
  /** Vía LEFT JOIN — siempre al día, nunca queda desactualizado. */
  patientName: string | null
  startsAt: string
  endsAt: string
  status: AppointmentStatus
  modality: AppointmentModality | null
  googleEventId: string | null
  googleCalendarId: string | null
  lastSyncedAt: string | null
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/** Lo que devuelve cada comando mutador: la cita ya reconciliada con Google. */
export interface AppointmentWithSync extends Appointment {
  syncOutcome: SyncOutcome
}

export interface AppointmentInput {
  patientId?: string | null
  startsAt: string
  endsAt: string
  modality?: AppointmentModality | null
}

/**
 * Advertencia de solapamiento — deliberadamente sin el nombre del paciente
 * de la otra cita (ver `services::appointments::OverlapWarning` en el
 * backend). Nunca bloquea el guardado.
 */
export interface OverlapWarning {
  startsAt: string
  endsAt: string
  hasPatient: boolean
}

export const APPOINTMENT_MODALITY_LABELS: Record<AppointmentModality, string> = {
  presencial: 'Presencial',
  online: 'Online',
  telefonico: 'Telefónico',
}
