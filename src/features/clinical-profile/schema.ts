import { z } from 'zod'

/**
 * `riskFlags` se valida únicamente como JSON sintácticamente válido — igual
 * criterio que el backend (`services::patient_clinical_profile::validate_risk_flags`):
 * ninguna forma específica (objeto, lista, etc.) es exigida ni interpretada.
 */
const riskFlagsField = z.string().optional().refine((v) => {
  if (!v || !v.trim()) return true
  try {
    JSON.parse(v)
    return true
  } catch {
    return false
  }
}, 'Debe ser JSON válido, por ejemplo: ["dato uno", "dato dos"]')

export const clinicalProfileFormSchema = z.object({
  presentingProblem: z.string().optional(),
  primaryDiagnosisCode: z.string().optional(),
  diagnosisNotes: z.string().optional(),
  riskFlags: riskFlagsField,
  relevantMedicalNotes: z.string().optional(),
})

export type ClinicalProfileFormValues = z.infer<typeof clinicalProfileFormSchema>
