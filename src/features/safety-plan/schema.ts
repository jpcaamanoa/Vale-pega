import { z } from 'zod'

/** Mismo criterio que `clinicalProfileFormSchema` (Fase 6) — campos libres, todos opcionales. */
export const safetyPlanFormSchema = z.object({
  warningSigns: z.string().optional(),
  internalStrategies: z.string().optional(),
  socialSupportStrategies: z.string().optional(),
  meansSafety: z.string().optional(),
  crisisSteps: z.string().optional(),
  notes: z.string().optional(),
  reviewedAt: z.string().optional(),
})

export type SafetyPlanFormValues = z.infer<typeof safetyPlanFormSchema>

export const safetyPlanContactFormSchema = z.object({
  contactType: z.enum(['support_person', 'professional', 'service', 'distraction_person', 'help_contact']),
  name: z.string().min(1, 'El contacto necesita un nombre.'),
  relationshipOrRole: z.string().optional(),
  phone: z.string().optional(),
  notes: z.string().optional(),
  address: z.string().optional(),
  servicePhone: z.string().optional(),
  isEmergencyContact: z.boolean().optional(),
  isCrisisService: z.boolean().optional(),
})

export type SafetyPlanContactFormValues = z.infer<typeof safetyPlanContactFormSchema>
