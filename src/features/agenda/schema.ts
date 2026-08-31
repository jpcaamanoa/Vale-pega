import { z } from 'zod'

export const appointmentFormSchema = z
  .object({
    patientId: z.string().optional(),
    startsAt: z.string().min(1, 'La fecha y hora de inicio son obligatorias'),
    endsAt: z.string().min(1, 'La fecha y hora de término son obligatorias'),
    modality: z.string().optional(),
  })
  .refine((v) => !v.startsAt || !v.endsAt || v.endsAt > v.startsAt, {
    message: 'La hora de término debe ser posterior a la hora de inicio',
    path: ['endsAt'],
  })

export type AppointmentFormValues = z.infer<typeof appointmentFormSchema>
