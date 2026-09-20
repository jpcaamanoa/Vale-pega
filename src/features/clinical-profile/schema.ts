import { z } from 'zod'

/**
 * `riskFlags` ("Factores de riesgo") ya no se edita como texto en este
 * formulario — la UI lo maneja como una lista de tags (`TagListField`,
 * serializada con `serializeRiskFlags`), que siempre produce JSON válido
 * por construcción. Por eso no necesita su propia validación aquí; el
 * backend (`services::patient_clinical_profile::validate_risk_flags`) sigue
 * validando de todas formas, como último resguardo.
 */
export const clinicalProfileFormSchema = z.object({
  presentingProblem: z.string().optional(),
  primaryDiagnosisCode: z.string().optional(),
  diagnosisNotes: z.string().optional(),
  relevantMedicalNotes: z.string().optional(),
})

export type ClinicalProfileFormValues = z.infer<typeof clinicalProfileFormSchema>
