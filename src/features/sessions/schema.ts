import { z } from 'zod'

const dateField = z
  .string()
  .min(1, 'La fecha es obligatoria')
  .refine((v) => /^\d{4}-\d{2}-\d{2}$/.test(v), 'Formato esperado: AAAA-MM-DD')

const timeField = z
  .string()
  .optional()
  .refine((v) => !v || /^\d{2}:\d{2}$/.test(v), 'Formato esperado: HH:MM')

const durationField = z
  .string()
  .optional()
  .refine((v) => !v || (Number(v) > 0 && Number.isFinite(Number(v))), 'La duración debe ser un número mayor que cero')

export const sessionCreateFormSchema = z.object({
  sessionDate: dateField,
  startTime: timeField,
  durationMinutes: durationField,
  modality: z.string().optional(),
  episodeId: z.string().optional(),
})

export type SessionCreateFormValues = z.infer<typeof sessionCreateFormSchema>

export const sessionMetadataFormSchema = z.object({
  sessionDate: dateField,
  startTime: timeField,
  durationMinutes: durationField,
  modality: z.string().optional(),
  status: z.enum(['programada', 'realizada', 'cancelada', 'no_asistio']),
})

export type SessionMetadataFormValues = z.infer<typeof sessionMetadataFormSchema>
