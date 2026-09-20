/**
 * Fase 12 — Plan de Seguridad Clínico Versionado. Herramienta de
 * documentación clínica pura: nunca infiere riesgo, nunca puntúa, nunca
 * notifica ni contacta a nadie automáticamente. Ver `docs/safety-plan.md`.
 */
export type SafetyPlanStatus = 'borrador' | 'vigente' | 'reemplazado'

export const SAFETY_PLAN_STATUS_LABELS: Record<SafetyPlanStatus, string> = {
  borrador: 'Borrador',
  vigente: 'Vigente',
  reemplazado: 'Reemplazado',
}

/** Contenido completo — solo se pide cuando la usuaria abre explícitamente un plan. */
export interface SafetyPlan {
  id: string
  patientId: string
  version: number
  status: SafetyPlanStatus
  warningSigns: string | null
  internalStrategies: string | null
  socialSupportStrategies: string | null
  meansSafety: string | null
  crisisSteps: string | null
  notes: string | null
  reviewedAt: string | null
  confirmedAt: string | null
  supersededAt: string | null
  createdAt: string
  updatedAt: string
}

/** Lo que devuelve el historial — deliberadamente sin contenido narrativo. */
export interface SafetyPlanSummary {
  id: string
  patientId: string
  version: number
  status: SafetyPlanStatus
  reviewedAt: string | null
  confirmedAt: string | null
  supersededAt: string | null
  createdAt: string
  updatedAt: string
}

export interface SafetyPlanInput {
  warningSigns?: string | null
  internalStrategies?: string | null
  socialSupportStrategies?: string | null
  meansSafety?: string | null
  crisisSteps?: string | null
  notes?: string | null
  /** Fecha (AAAA-MM-DD) de la última revisión del plan con la persona. */
  reviewedAt?: string | null
}

/**
 * `support_person` es el tipo original de Fase 12 — ya no se ofrece para
 * crear un contacto nuevo (rechazado por el backend con
 * `InvalidContactType`), pero un contacto que ya lo tenía sigue siendo
 * legible y editable: nunca se reclasifica automáticamente en uno de los
 * tipos nuevos del rediseño de seis pasos.
 */
export type SafetyPlanContactType = 'support_person' | 'professional' | 'service' | 'distraction_person' | 'help_contact'

export const SAFETY_PLAN_CONTACT_TYPE_LABELS: Record<SafetyPlanContactType, string> = {
  support_person: 'Persona de apoyo (registrado antes del rediseño)',
  professional: 'Profesional',
  service: 'Servicio',
  distraction_person: 'Persona de distracción',
  help_contact: 'Persona a quien pedir ayuda',
}

/** Tipos que la interfaz nueva sigue ofreciendo para crear un contacto — mismo criterio que `CREATABLE_CONTACT_TYPES` del backend. */
export const CREATABLE_SAFETY_PLAN_CONTACT_TYPES: SafetyPlanContactType[] = ['professional', 'service', 'distraction_person', 'help_contact']

export interface SafetyPlanContact {
  id: string
  safetyPlanId: string
  contactType: SafetyPlanContactType
  name: string
  relationshipOrRole: string | null
  phone: string | null
  notes: string | null
  /** Paso 5 — solo tiene sentido para `contactType` 'professional'/'service'. */
  address: string | null
  servicePhone: string | null
  isEmergencyContact: boolean
  isCrisisService: boolean
  sortOrder: number
}

export interface SafetyPlanContactInput {
  contactType: SafetyPlanContactType
  name: string
  relationshipOrRole?: string | null
  phone?: string | null
  notes?: string | null
  address?: string | null
  servicePhone?: string | null
  isEmergencyContact?: boolean
  isCrisisService?: boolean
}

/** Taxonomía cerrada de `safety_plan_list_items.item_type` — ver `VALID_ITEM_TYPES` del backend. */
export type SafetyPlanItemType = 'warning_sign' | 'strategy' | 'distraction_place'

export interface SafetyPlanListItem {
  id: string
  safetyPlanId: string
  itemType: SafetyPlanItemType
  content: string
  sortOrder: number
}

export interface SafetyPlanListItemInput {
  itemType: SafetyPlanItemType
  content: string
}

/**
 * Advertencia sobria y no alarmista (§25 de la aprobación de Fase 12) — se
 * muestra siempre que se abre la pestaña, exista o no un plan.
 */
export const SAFETY_PLAN_DISCLAIMER =
  'Este plan es un recurso de apoyo elaborado en el contexto de la atención clínica. Cuaderno Clínico no es un servicio de emergencia ni sustituye la atención de urgencia.'
