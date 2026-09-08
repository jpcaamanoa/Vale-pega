import { invoke } from '@tauri-apps/api/core'
import type { SafetyPlan, SafetyPlanContact, SafetyPlanContactInput, SafetyPlanInput, SafetyPlanSummary } from './types'

export const safetyPlanApi = {
  /** `null` si el paciente todavía no tiene un plan de seguridad vigente. */
  getCurrent: (patientId: string) => invoke<SafetyPlan | null>('get_current_safety_plan', { patientId }),

  /** `null` si el paciente no tiene un borrador sin confirmar en este momento. */
  getDraft: (patientId: string) => invoke<SafetyPlan | null>('get_safety_plan_draft', { patientId }),

  /** Historial minimizado — sin contenido narrativo, ver `get(planId)` para abrir una versión. */
  listHistory: (patientId: string) => invoke<SafetyPlanSummary[]>('list_safety_plan_history', { patientId }),

  /** Contenido completo de una versión concreta del historial. */
  get: (planId: string) => invoke<SafetyPlan>('get_safety_plan_by_id', { planId }),

  createDraft: (patientId: string, input: SafetyPlanInput) => invoke<SafetyPlan>('create_safety_plan_draft', { patientId, input }),

  /**
   * "Actualizar plan": crea un borrador nuevo con una copia completa e
   * independiente del vigente actual (texto, fecha de revisión y
   * contactos, estos últimos con IDs nuevos) — hecho atómicamente en el
   * backend para que la red de contactos nunca quede vacía en silencio.
   */
  createDraftFromCurrent: (patientId: string) => invoke<SafetyPlan>('create_safety_plan_draft_from_current', { patientId }),

  updateDraft: (planId: string, input: SafetyPlanInput) => invoke<SafetyPlan>('update_safety_plan_draft', { planId, input }),

  discardDraft: (planId: string) => invoke<void>('discard_safety_plan_draft', { planId }),

  confirmDraft: (planId: string) => invoke<SafetyPlan>('confirm_safety_plan_draft', { planId }),

  listContacts: (planId: string) => invoke<SafetyPlanContact[]>('list_safety_plan_contacts', { planId }),

  addContact: (planId: string, input: SafetyPlanContactInput) => invoke<SafetyPlanContact>('add_safety_plan_contact', { planId, input }),

  updateContact: (contactId: string, input: SafetyPlanContactInput) => invoke<SafetyPlanContact>('update_safety_plan_contact', { contactId, input }),

  deleteContact: (contactId: string) => invoke<void>('delete_safety_plan_contact', { contactId }),
}
