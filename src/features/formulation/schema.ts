import { z } from 'zod'

export const formulationCreateSchema = z.object({
  title: z.string().min(1, 'El título de la formulación es obligatorio.'),
  modelType: z.string().optional(),
})

export type FormulationCreateValues = z.infer<typeof formulationCreateSchema>

/** Las secciones del contenido son texto libre, todas opcionales — se validan en `serializeSections`, no aquí. */
export const formulationSectionsSchema = z.record(z.string(), z.string())

export type FormulationSectionsValues = z.infer<typeof formulationSectionsSchema>
