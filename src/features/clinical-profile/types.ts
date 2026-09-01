/**
 * Registro mutable simple — sin versionado, sin historial (ver
 * `docs/clinical-profile.md`). Un único registro por paciente, `patientId`
 * es la propia clave primaria de `patient_clinical_profile` en el backend.
 */
export interface ClinicalProfile {
  patientId: string
  presentingProblem: string | null
  primaryDiagnosisCode: string | null
  diagnosisNotes: string | null
  riskFlags: string | null
  relevantMedicalNotes: string | null
  updatedAt: string
}

export interface ClinicalProfileInput {
  presentingProblem?: string | null
  primaryDiagnosisCode?: string | null
  diagnosisNotes?: string | null
  riskFlags?: string | null
  relevantMedicalNotes?: string | null
}
