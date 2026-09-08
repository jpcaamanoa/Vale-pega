/**
 * Fase 13. Regla de copyright, no negociable: esta aplicación nunca
 * almacena ni muestra el contenido real de un instrumento (ítems,
 * preguntas, opciones, tablas de normas propietarias) — únicamente
 * metadatos del catálogo y resultados de una administración concreta. Ver
 * `docs/assessments.md`.
 */

export type AssessmentContext = 'ingreso' | 'seguimiento' | 'alta'

export const ASSESSMENT_CONTEXT_LABELS: Record<AssessmentContext, string> = {
  ingreso: 'Ingreso',
  seguimiento: 'Seguimiento',
  alta: 'Alta',
}

/** Instrumento del catálogo que la propia usuaria mantiene — nunca su contenido protegido, solo estos metadatos. */
export interface AssessmentInstrument {
  id: string
  name: string
  abbreviation: string | null
  description: string | null
  category: string | null
  isCustom: boolean
}

export interface InstrumentInput {
  name: string
  abbreviation?: string | null
  description?: string | null
  category?: string | null
}

/** Contenido completo de una administración — solo se pide al abrirla explícitamente. */
export interface AssessmentAdministration {
  id: string
  patientId: string
  instrumentId: string
  episodeId: string | null
  administeredAt: string
  context: AssessmentContext | null
  totalScore: number | null
  subscaleScores: string | null
  interpretationText: string | null
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/** Fila de listado — minimización de IPC: sin subescalas ni interpretación, ver `docs/assessments.md`. */
export interface AssessmentAdministrationSummary {
  id: string
  instrumentId: string
  instrumentName: string
  instrumentAbbreviation: string | null
  episodeId: string | null
  administeredAt: string
  context: AssessmentContext | null
  totalScore: number | null
}

export interface AdministrationInput {
  patientId: string
  instrumentId: string
  episodeId?: string | null
  administeredAt: string
  context?: AssessmentContext | null
  totalScore?: number | null
  subscaleScores?: string | null
  interpretationText?: string | null
}

/** Igual que `AdministrationInput` salvo `patientId`/`instrumentId`, que nunca se reasignan una vez creada. */
export interface AdministrationUpdateInput {
  episodeId?: string | null
  administeredAt: string
  context?: AssessmentContext | null
  totalScore?: number | null
  subscaleScores?: string | null
  interpretationText?: string | null
}
