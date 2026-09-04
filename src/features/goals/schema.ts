import { z } from 'zod'

const titleField = z.string().trim().min(1, 'El título es obligatorio')

const targetDateField = z
  .string()
  .optional()
  .refine((v) => !v || /^\d{4}-\d{2}-\d{2}$/.test(v), 'Formato esperado: AAAA-MM-DD')

export const goalCreateFormSchema = z.object({
  title: titleField,
  description: z.string().optional(),
  targetDate: targetDateField,
  episodeId: z.string().optional(),
})

export type GoalCreateFormValues = z.infer<typeof goalCreateFormSchema>

export const goalUpdateFormSchema = z.object({
  title: titleField,
  description: z.string().optional(),
  status: z.enum(['activo', 'logrado', 'pausado', 'descartado']),
  targetDate: targetDateField,
})

export type GoalUpdateFormValues = z.infer<typeof goalUpdateFormSchema>

export const goalIndicatorFormSchema = z.object({
  description: z.string().trim().min(1, 'La descripción es obligatoria'),
  baselineValue: z.string().optional(),
  targetValue: z.string().optional(),
})

export type GoalIndicatorFormValues = z.infer<typeof goalIndicatorFormSchema>
