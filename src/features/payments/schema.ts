import { z } from 'zod'

// La autoritativa vive en Rust (services::payments::validate) — este
// archivo es solo para feedback inmediato en el formulario.

const amountField = z
  .string()
  .trim()
  .min(1, 'El monto es obligatorio')
  .refine((v) => Number.isFinite(Number(v)), 'El monto debe ser un número')
  .refine((v) => Number(v) >= 0, 'El monto no puede ser negativo')
  .refine((v) => Number.isInteger(Number(v)), 'El monto en CLP debe ser un número entero, sin decimales')

const dateField = z
  .string()
  .optional()
  .refine((v) => !v || /^\d{4}-\d{2}-\d{2}$/.test(v), 'Formato esperado: AAAA-MM-DD')

const PAYMENT_STATUSES = ['pendiente', 'pagado', 'atrasado', 'condonado'] as const

function crossFieldChecks(data: { amount: string; method?: string; status: (typeof PAYMENT_STATUSES)[number]; paidAt?: string }, ctx: z.RefinementCtx) {
  if (Number(data.amount) === 0 && data.status !== 'condonado') {
    ctx.addIssue({ code: 'custom', message: "Un monto de 0 solo es válido si el estado es 'Condonado'", path: ['amount'] })
  }
  if (data.status === 'pagado' && !data.method) {
    ctx.addIssue({ code: 'custom', message: "El método es obligatorio cuando el estado es 'Pagado'", path: ['method'] })
  }
  if (data.status === 'pagado' && !data.paidAt) {
    ctx.addIssue({ code: 'custom', message: "La fecha de pago es obligatoria cuando el estado es 'Pagado'", path: ['paidAt'] })
  }
  if ((data.status === 'pendiente' || data.status === 'atrasado') && data.paidAt) {
    ctx.addIssue({ code: 'custom', message: "La fecha de pago solo aplica a 'Pagado' o 'Condonado'", path: ['paidAt'] })
  }
}

export const paymentFormSchema = z
  .object({
    amount: amountField,
    method: z.string().optional(),
    status: z.enum(PAYMENT_STATUSES),
    dueDate: dateField,
    paidAt: dateField,
    notes: z.string().optional(),
  })
  .superRefine(crossFieldChecks)

export type PaymentFormValues = z.infer<typeof paymentFormSchema>
