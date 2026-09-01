import { invoke } from '@tauri-apps/api/core'
import type {
  Goal,
  GoalIndicator,
  GoalIndicatorInput,
  GoalInput,
  GoalListItem,
  GoalSessionRow,
  GoalUpdateInput,
  SessionGoalLinkInput,
  SessionGoalRow,
} from './types'

export const goalsApi = {
  create: (input: GoalInput) => invoke<Goal>('create_goal', { input }),

  get: (id: string) => invoke<Goal>('get_goal', { id }),

  list: (patientId: string) => invoke<GoalListItem[]>('list_goals', { patientId }),

  listArchived: (patientId: string) => invoke<GoalListItem[]>('list_archived_goals', { patientId }),

  update: (id: string, input: GoalUpdateInput) => invoke<Goal>('update_goal', { id, input }),

  archive: (id: string) => invoke<void>('archive_goal', { id }),

  restore: (id: string) => invoke<Goal>('restore_goal', { id }),

  listIndicators: (goalId: string) => invoke<GoalIndicator[]>('list_goal_indicators', { goalId }),

  createIndicator: (goalId: string, input: GoalIndicatorInput) => invoke<GoalIndicator>('create_goal_indicator', { goalId, input }),

  updateIndicator: (id: string, input: GoalIndicatorInput) => invoke<GoalIndicator>('update_goal_indicator', { id, input }),

  deleteIndicator: (id: string) => invoke<void>('delete_goal_indicator', { id }),

  linkSessionGoal: (input: SessionGoalLinkInput) => invoke<void>('link_session_goal', { input }),

  unlinkSessionGoal: (sessionId: string, goalId: string) => invoke<void>('unlink_session_goal', { sessionId, goalId }),

  updateSessionGoalProgress: (sessionId: string, goalId: string, progressNote: string | null) =>
    invoke<void>('update_session_goal_progress', { sessionId, goalId, progressNote }),

  listGoalsForSession: (sessionId: string) => invoke<SessionGoalRow[]>('list_goals_for_session', { sessionId }),

  listSessionsForGoal: (goalId: string) => invoke<GoalSessionRow[]>('list_sessions_for_goal', { goalId }),

  listAvailableGoalsForSession: (sessionId: string) => invoke<GoalListItem[]>('list_available_goals_for_session', { sessionId }),
}
