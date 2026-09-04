/**
 * Cinco estados: los cuatro pedidos explícitamente (`pendiente`/`parcial`/
 * `realizada`/`no_realizada`) más `descartada` — cubre una tarea que deja
 * de ser relevante *antes* de llegar a revisarse en ninguna sesión, un caso
 * distinto de `no_realizada` (que sí implica una revisión con resultado
 * negativo). Ver `docs/session-continuity.md`.
 */
export type TherapyTaskStatus = 'pendiente' | 'parcial' | 'realizada' | 'no_realizada' | 'descartada'

/**
 * Distinto de `session_notes.homeworkTasks`: es un registro operativo con
 * ciclo de vida propio, independiente de cualquier nota clínica concreta.
 */
export interface TherapyTask {
  id: string
  patientId: string
  assignedInSessionId: string | null
  goalId: string | null
  description: string
  status: TherapyTaskStatus
  assignedAt: string
  reviewDueAt: string | null
  reviewedInSessionId: string | null
  reviewedAt: string | null
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/** Fila de listado — incluye el título del objetivo vinculado (si hay uno), calculado en el backend. */
export interface TherapyTaskListItem {
  id: string
  assignedInSessionId: string | null
  goalId: string | null
  goalTitle: string | null
  description: string
  status: TherapyTaskStatus
  assignedAt: string
  reviewDueAt: string | null
  reviewedInSessionId: string | null
  reviewedAt: string | null
}

export interface TherapyTaskInput {
  patientId: string
  description: string
  assignedInSessionId?: string | null
  goalId?: string | null
  reviewDueAt?: string | null
}

/** Deliberadamente sin `patientId` ni `status` — reasignar una tarea a otro paciente no es una operación de este MVP; el estado cambia vía `review`. */
export interface TherapyTaskUpdateInput {
  description: string
  goalId?: string | null
  reviewDueAt?: string | null
}

export interface TherapyTaskReviewInput {
  status: TherapyTaskStatus
  reviewedInSessionId?: string | null
}

export const THERAPY_TASK_STATUS_LABELS: Record<TherapyTaskStatus, string> = {
  pendiente: 'Pendiente',
  parcial: 'Parcial',
  realizada: 'Realizada',
  no_realizada: 'No realizada',
  descartada: 'Descartada',
}
