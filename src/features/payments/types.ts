export type PaymentStatus = 'pendiente' | 'pagado' | 'atrasado' | 'condonado'
export type PaymentMethod = 'efectivo' | 'transferencia' | 'tarjeta' | 'otro'

/** Ficha completa de un pago. */
export interface Payment {
  id: string
  patientId: string
  sessionId: string | null
  amount: number
  currency: string
  method: PaymentMethod | null
  status: PaymentStatus
  dueDate: string | null
  paidAt: string | null
  notes: string | null
  /**
   * Calculado en el backend en el momento de leer (nunca persistido): un
   * pago sigue guardado como `pendiente` hasta que alguien lo marca
   * `pagado`/`condonado` — `isOverdue` es solo una señal de presentación
   * para mostrar "Atrasado" cuando corresponde, sin que reeditar y guardar
   * el pago vuelva a escribir "pendiente" por encima. Ver
   * `effectivePaymentStatusLabel`.
   */
  isOverdue: boolean
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/**
 * Fila de listado — deliberadamente sin `notes` (detalle administrativo,
 * no necesario para una lista) ni `patientId` (el listado ya está scoped a
 * un paciente) — mismo criterio de minimización que `GoalListItem`.
 */
export interface PaymentListItem {
  id: string
  sessionId: string | null
  amount: number
  currency: string
  method: PaymentMethod | null
  status: PaymentStatus
  dueDate: string | null
  paidAt: string | null
  isOverdue: boolean
  createdAt: string
}

export interface PaymentInput {
  patientId: string
  sessionId?: string | null
  amount: number
  currency?: string | null
  method?: PaymentMethod | null
  status?: PaymentStatus | null
  dueDate?: string | null
  paidAt?: string | null
  notes?: string | null
}

/** Deliberadamente sin `patientId` — reasignar un pago a otro paciente no
 * es una operación de este MVP. */
export interface PaymentUpdateInput {
  sessionId?: string | null
  amount: number
  currency?: string | null
  method?: PaymentMethod | null
  status: PaymentStatus
  dueDate?: string | null
  paidAt?: string | null
  notes?: string | null
}

/** Agregados administrativos para el Dashboard — nunca pagos individuales. */
export interface PaymentDashboardSummary {
  paidThisMonthTotal: number
  pendingCount: number
  pendingTotal: number
}

export const PAYMENT_STATUS_LABELS: Record<PaymentStatus, string> = {
  pendiente: 'Pendiente',
  pagado: 'Pagado',
  atrasado: 'Atrasado',
  condonado: 'Condonado',
}

export const PAYMENT_METHOD_LABELS: Record<PaymentMethod, string> = {
  efectivo: 'Efectivo',
  transferencia: 'Transferencia',
  tarjeta: 'Tarjeta',
  otro: 'Otro',
}

/**
 * Etiqueta que ve la usuaria: "Atrasado" para un pago que sigue guardado
 * como `pendiente` pero ya pasó su `dueDate` — sin que eso implique que el
 * campo `status` real cambió. Usar siempre esta función para mostrar el
 * estado (listados, ficha), nunca `PAYMENT_STATUS_LABELS[status]` directo
 * salvo dentro del propio formulario de edición, que opera sobre el
 * `status` crudo a propósito.
 */
export function effectivePaymentStatusLabel(payment: { status: PaymentStatus; isOverdue: boolean }): string {
  if (payment.status === 'pendiente' && payment.isOverdue) return PAYMENT_STATUS_LABELS.atrasado
  return PAYMENT_STATUS_LABELS[payment.status]
}
