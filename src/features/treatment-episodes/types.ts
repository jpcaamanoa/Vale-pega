/**
 * Fase 9 — Procesos terapéuticos. Deliberadamente pequeño: solo
 * `startedAt`/`status`. Nada de motivo/resumen de cierre — eso pertenece a
 * la futura Fase 10 (Cierre/Alta). Ver `docs/treatment-episodes.md`.
 */
export type TreatmentEpisodeStatus = 'activo' | 'pausado' | 'cerrado'

export interface TreatmentEpisode {
  id: string
  patientId: string
  startedAt: string
  status: TreatmentEpisodeStatus
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

export interface TreatmentEpisodeInput {
  patientId: string
  /** Opcional — si no se envía, el backend usa la fecha de hoy. */
  startedAt?: string | null
}

/**
 * Antecedentes clínicos específicos de un proceso — motivo de consulta,
 * diagnóstico y notas diagnósticas de ESE proceso. `riskFlags`/
 * `relevantMedicalNotes` no existen aquí: permanecen longitudinales en
 * `patient_clinical_profile` (Fase 6).
 */
export interface EpisodeClinicalProfile {
  episodeId: string
  presentingProblem: string | null
  primaryDiagnosisCode: string | null
  diagnosisNotes: string | null
  updatedAt: string
}

export interface EpisodeClinicalProfileInput {
  presentingProblem?: string | null
  primaryDiagnosisCode?: string | null
  diagnosisNotes?: string | null
}

export const TREATMENT_EPISODE_STATUS_LABELS: Record<TreatmentEpisodeStatus, string> = {
  activo: 'Activo',
  pausado: 'Pausado',
  cerrado: 'Cerrado',
}

/**
 * Cierre estructurado de un proceso terapéutico (Fase 11). Ver
 * `docs/episode-closure.md`. Inmutable tras crearse — corregir un error de
 * fondo es anular (`revertedAt`/`revertedReason`) y crear un cierre nuevo,
 * nunca editar uno existente.
 */
export type ClosureReason = 'alta' | 'cierre_acordado' | 'interrupcion' | 'derivacion' | 'decision_profesional' | 'otro'
export type ClosureOutcome = 'objetivos_logrados' | 'parcialmente_logrados' | 'no_logrados' | 'no_evaluable'

export const CLOSURE_REASON_LABELS: Record<ClosureReason, string> = {
  alta: 'Alta terapéutica',
  cierre_acordado: 'Cierre acordado',
  interrupcion: 'Interrupción del proceso',
  derivacion: 'Derivación',
  decision_profesional: 'Decisión profesional',
  otro: 'Otro',
}

export const CLOSURE_OUTCOME_LABELS: Record<ClosureOutcome, string> = {
  objetivos_logrados: 'Objetivos logrados',
  parcialmente_logrados: 'Objetivos parcialmente logrados',
  no_logrados: 'Objetivos no logrados',
  no_evaluable: 'No evaluable',
}

export interface EpisodeClosure {
  id: string
  episodeId: string
  closedAt: string
  reason: ClosureReason
  reasonDetail: string | null
  outcome: ClosureOutcome
  summary: string | null
  recommendations: string | null
  revertedAt: string | null
  revertedReason: string | null
  createdAt: string
  updatedAt: string
}

export interface SessionResolutionInput {
  sessionId: string
  /** `true` = cancelar esta sesión futura como parte del cierre; `false` = mantenerla tal cual. */
  cancel: boolean
}

export interface CloseEpisodeInput {
  /** Opcional — si no se envía, el backend usa la fecha de hoy. */
  closedAt?: string | null
  reason: ClosureReason
  reasonDetail?: string | null
  outcome: ClosureOutcome
  summary?: string | null
  recommendations?: string | null
  /** Debe cubrir exactamente las sesiones futuras agendadas del proceso — resolución manual explícita, nunca implícita. */
  sessionResolutions: SessionResolutionInput[]
}

export interface RevertClosureInput {
  revertedReason: string
  /** A qué estado vuelve el proceso — siempre se pregunta explícitamente, nunca se asume. */
  reopenStatus: 'activo' | 'pausado'
}
