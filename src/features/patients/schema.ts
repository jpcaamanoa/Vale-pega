import { z } from 'zod'
import { isValidChileanRut } from './rut'

// Los campos opcionales se dejan pasar tal cual (incluida una cadena vacía)
// — Rust (services::patients::none_if_blank) ya normaliza "" a ausente. No
// hace falta duplicar esa transformación acá, y evita el desajuste de tipos
// entre "input" y "output" de Zod al combinarlo con react-hook-form.
const optionalText = z.string().optional()

const dateField = z
  .string()
  .optional()
  .refine((v) => !v || /^\d{4}-\d{2}-\d{2}$/.test(v), 'Formato esperado: AAAA-MM-DD')

export const patientFormSchema = z.object({
  fullName: z.string().trim().min(1, 'El nombre completo es obligatorio'),
  preferredName: optionalText,
  rut: optionalText.refine((v) => !v || v.trim() === '' || isValidChileanRut(v), 'RUT inválido (verifica el dígito verificador)'),
  birthDate: dateField,
  phone: optionalText,
  email: optionalText.refine((v) => !v || v.trim() === '' || /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(v), 'Correo inválido'),
  address: optionalText,
  emergencyContactName: optionalText,
  emergencyContactPhone: optionalText,
  emergencyContactRelationship: optionalText,
  status: z.enum(['activo', 'inactivo', 'alta', 'archivado']),
  referredBy: optionalText,
  intakeDate: dateField,
})

export type PatientFormValues = z.infer<typeof patientFormSchema>
