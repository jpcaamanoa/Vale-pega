/**
 * Fase 15. La formulación es una hipótesis clínica de trabajo escrita por
 * la profesional, nunca generada automáticamente. Ver `docs/formulation.md`.
 *
 * Siempre pertenece a un proceso terapéutico concreto (`episodeId`) — nunca
 * "suelta" del paciente. Un reingreso (proceso nuevo) nunca hereda
 * automáticamente la formulación de un proceso anterior.
 */
export interface CaseFormulation {
  id: string
  patientId: string
  episodeId: string | null
  title: string
  modelType: string | null
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/** Nunca hay estado de "borrador": cada versión ya es una versión confirmada. La actual es siempre la de `versionNumber` más alto. */
export interface FormulationVersion {
  id: string
  formulationId: string
  versionNumber: number
  summaryText: string | null
  createdAt: string
}

/** Fila de listado — minimización de IPC: nunca lleva `summaryText` completo. */
export interface FormulationSummary {
  id: string
  patientId: string
  episodeId: string | null
  title: string
  modelType: string | null
  currentVersion: number
  updatedAt: string
}

export interface FormulationInput {
  patientId: string
  episodeId: string
  title: string
  modelType?: string | null
  summaryText?: string | null
}

export interface NewVersionInput {
  summaryText?: string | null
}

/**
 * Secciones conceptuales sugeridas para estructurar el contenido de una
 * formulación (§13 del encargo de Fase 15). No son columnas SQL — el
 * frontend las serializa a un único `summaryText` con encabezados Markdown
 * simples (`## Sección`) y las vuelve a separar al abrir una versión. La
 * profesional puede dejar cualquier sección vacía; ninguna es obligatoria.
 */
export const FORMULATION_SECTIONS: { key: string; label: string }[] = [
  { key: 'sintesis', label: 'Síntesis del caso' },
  { key: 'predisponentes', label: 'Factores predisponentes' },
  { key: 'precipitantes', label: 'Factores precipitantes' },
  { key: 'perpetuantes', label: 'Factores perpetuantes / mantenedores' },
  { key: 'protectores', label: 'Factores protectores' },
  { key: 'patrones', label: 'Patrones cognitivos/emocionales/conductuales' },
  { key: 'hipotesis', label: 'Hipótesis de funcionamiento' },
  { key: 'focos', label: 'Focos terapéuticos' },
  { key: 'plan', label: 'Plan de intervención' },
  { key: 'observaciones', label: 'Observaciones' },
]

/** Sugerencias comunes para `modelType` — texto libre, nunca una lista cerrada (el `<select>` siempre admite "Otro" con texto propio). */
export const FORMULATION_MODEL_SUGGESTIONS = ['TCC', '5P (predisponentes/precipitantes/perpetuantes/protectores/presentación)', 'Transdiagnóstico']
