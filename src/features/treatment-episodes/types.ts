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
