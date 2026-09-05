import { invoke } from '@tauri-apps/api/core'
import type { TherapyTask, TherapyTaskInput, TherapyTaskListItem, TherapyTaskReviewInput, TherapyTaskUpdateInput } from './types'

export const therapyTasksApi = {
  create: (input: TherapyTaskInput) => invoke<TherapyTask>('create_therapy_task', { input }),

  get: (id: string) => invoke<TherapyTask>('get_therapy_task', { id }),

  list: (patientId: string) => invoke<TherapyTaskListItem[]>('list_therapy_tasks', { patientId }),

  listArchived: (patientId: string) => invoke<TherapyTaskListItem[]>('list_archived_therapy_tasks', { patientId }),

  listPending: (patientId: string) => invoke<TherapyTaskListItem[]>('list_pending_therapy_tasks', { patientId }),

  /** `'pendiente'` + `'parcial'` — usada exclusivamente por la advertencia del flujo de cierre de un proceso (Fase 11). */
  listPendingOrPartial: (patientId: string) => invoke<TherapyTaskListItem[]>('list_pending_or_partial_therapy_tasks', { patientId }),

  update: (id: string, input: TherapyTaskUpdateInput) => invoke<TherapyTask>('update_therapy_task', { id, input }),

  review: (id: string, input: TherapyTaskReviewInput) => invoke<TherapyTask>('review_therapy_task', { id, input }),

  archive: (id: string) => invoke<void>('archive_therapy_task', { id }),

  restore: (id: string) => invoke<TherapyTask>('restore_therapy_task', { id }),

  pendingCount: () => invoke<number>('get_pending_therapy_task_count'),
}
