export type GoalStatus = 'activo' | 'logrado' | 'pausado' | 'descartado'

export interface Goal {
  id: string
  patientId: string
  formulationId: string | null
  /** Opcional (Fase 9) — proceso terapéutico al que pertenece este objetivo. */
  episodeId: string | null
  title: string
  description: string | null
  status: GoalStatus
  targetDate: string | null
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/**
 * Fila de listado — deliberadamente sin `description` (ver
 * `repositories::goals::GoalListItem` en el backend). Solo lo necesario
 * para una lista y para saber cuántos indicadores/sesiones tiene el
 * objetivo, nunca el contenido clínico completo.
 */
export interface GoalListItem {
  id: string
  title: string
  status: GoalStatus
  targetDate: string | null
  indicatorCount: number
  sessionCount: number
}

export interface GoalInput {
  patientId: string
  /** Opcional (Fase 9) — vincular este objetivo a un proceso terapéutico. */
  episodeId?: string | null
  title: string
  description?: string | null
  targetDate?: string | null
}

export interface GoalUpdateInput {
  title: string
  description?: string | null
  status: GoalStatus
  targetDate?: string | null
}

export interface GoalIndicator {
  id: string
  goalId: string
  description: string
  baselineValue: string | null
  targetValue: string | null
}

export interface GoalIndicatorInput {
  description: string
  baselineValue?: string | null
  targetValue?: string | null
}

export interface SessionGoalLinkInput {
  sessionId: string
  goalId: string
  progressNote?: string | null
}

/** Objetivo trabajado en una sesión, visto desde la sesión. */
export interface SessionGoalRow {
  goalId: string
  goalTitle: string
  goalStatus: GoalStatus
  progressNote: string | null
}

/** Sesión donde se trabajó un objetivo, vista desde el objetivo. */
export interface GoalSessionRow {
  sessionId: string
  sessionDate: string
  startTime: string | null
  sessionStatus: string
  progressNote: string | null
}

export const GOAL_STATUS_LABELS: Record<GoalStatus, string> = {
  activo: 'Activo',
  logrado: 'Logrado',
  pausado: 'Pausado',
  descartado: 'Descartado',
}
