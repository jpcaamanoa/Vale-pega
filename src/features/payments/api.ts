import { invoke } from '@tauri-apps/api/core'
import type { Payment, PaymentDashboardSummary, PaymentInput, PaymentListItem, PaymentUpdateInput } from './types'

export const paymentsApi = {
  create: (input: PaymentInput) => invoke<Payment>('create_payment', { input }),

  get: (id: string) => invoke<Payment>('get_payment', { id }),

  list: (patientId: string) => invoke<PaymentListItem[]>('list_payments', { patientId }),

  listArchived: (patientId: string) => invoke<PaymentListItem[]>('list_archived_payments', { patientId }),

  update: (id: string, input: PaymentUpdateInput) => invoke<Payment>('update_payment', { id, input }),

  archive: (id: string) => invoke<void>('archive_payment', { id }),

  restore: (id: string) => invoke<Payment>('restore_payment', { id }),

  dashboardSummary: () => invoke<PaymentDashboardSummary>('get_payment_dashboard_summary'),
}
