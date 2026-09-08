import { z } from 'zod'

export const instrumentFormSchema = z.object({
  name: z.string().min(1, 'El nombre del instrumento es obligatorio.'),
  abbreviation: z.string().optional(),
  description: z.string().optional(),
  category: z.string().optional(),
})

export type InstrumentFormValues = z.infer<typeof instrumentFormSchema>

/**
 * `totalScore` viaja como texto en el formulario (permite dejarlo vacío) y
 * se convierte a número al enviar — sin restringir el signo: algunos
 * instrumentos usan puntajes estandarizados que pueden ser negativos (ver
 * `docs/db-schema.md`, "Algo que decidí NO agregar").
 * `subscaleScores` es JSON de texto libre, sin ninguna estructura impuesta
 * por este formulario — el backend valida únicamente que sea JSON
 * sintácticamente correcto, nunca su forma.
 */
export const administrationFormSchema = z.object({
  instrumentId: z.string().min(1, 'Selecciona un instrumento del catálogo.'),
  episodeId: z.string().optional(),
  administeredAt: z.string().min(1, 'La fecha de administración es obligatoria.'),
  context: z.string().optional(),
  totalScore: z.string().optional(),
  subscaleScores: z.string().optional(),
  interpretationText: z.string().optional(),
})

export type AdministrationFormValues = z.infer<typeof administrationFormSchema>
