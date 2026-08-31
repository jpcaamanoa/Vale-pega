import { invoke } from '@tauri-apps/api/core'
import type { Appointment, AppointmentInput, AppointmentWithSync, OverlapWarning } from './types'

export const agendaApi = {
  create: (input: AppointmentInput) => invoke<AppointmentWithSync>('create_appointment', { input }),

  get: (id: string) => invoke<Appointment>('get_appointment', { id }),

  list: (from?: string, to?: string) => invoke<Appointment[]>('list_appointments', { from: from ?? null, to: to ?? null }),

  listArchived: (from?: string, to?: string) =>
    invoke<Appointment[]>('list_archived_appointments', { from: from ?? null, to: to ?? null }),

  checkOverlap: (startsAt: string, endsAt: string, excludeId?: string) =>
    invoke<OverlapWarning[]>('check_overlap', { startsAt, endsAt, excludeId: excludeId ?? null }),

  update: (id: string, input: AppointmentInput) => invoke<AppointmentWithSync>('update_appointment', { id, input }),

  cancel: (id: string) => invoke<AppointmentWithSync>('cancel_appointment', { id }),

  archive: (id: string) => invoke<AppointmentWithSync>('archive_appointment', { id }),

  restore: (id: string) => invoke<AppointmentWithSync>('restore_appointment', { id }),

  /** Reintento manual — mismo comando que dispara la sincronización automática tras cada mutación. */
  retrySync: (id: string) => invoke<AppointmentWithSync>('retry_appointment_sync', { id }),
}
