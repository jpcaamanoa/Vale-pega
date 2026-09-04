import { z } from 'zod'

/** Mismo criterio que `clinicalProfileFormSchema` (Fase 6) — campos libres, todos opcionales. */
export const episodeClinicalProfileFormSchema = z.object({
  presentingProblem: z.string().optional(),
  primaryDiagnosisCode: z.string().optional(),
  diagnosisNotes: z.string().optional(),
})

export type EpisodeClinicalProfileFormValues = z.infer<typeof episodeClinicalProfileFormSchema>
