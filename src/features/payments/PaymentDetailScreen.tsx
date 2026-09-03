import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { Textarea } from '../../components/ui/Textarea'
import { TextField } from '../../components/ui/TextField'
import { paymentsApi } from './api'
import { formatClp } from './formatCurrency'
import { paymentFormSchema, type PaymentFormValues } from './schema'
import {
  effectivePaymentStatusLabel,
  PAYMENT_METHOD_LABELS,
  PAYMENT_STATUS_LABELS,
  type Payment,
  type PaymentMethod,
  type PaymentStatus,
  type PaymentUpdateInput,
} from './types'

function ConfirmDialog({
  title,
  description,
  confirmLabel,
  onDismiss,
  onConfirm,
}: {
  title: string
  description: string
  confirmLabel: string
  onDismiss: () => void
  onConfirm: () => void
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
      <div className="w-full max-w-sm rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-2 text-base font-semibold text-foreground">{title}</h2>
        <p className="mb-4 text-sm text-muted-foreground">{description}</p>
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onDismiss}>
            Volver
          </Button>
          <Button onClick={onConfirm}>{confirmLabel}</Button>
        </div>
      </div>
    </div>
  )
}

function PaymentMetadataForm({ payment, onSaved }: { payment: Payment; onSaved: (payment: Payment) => void }) {
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const {
    register,
    handleSubmit,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<PaymentFormValues>({
    resolver: zodResolver(paymentFormSchema),
    defaultValues: {
      amount: String(payment.amount),
      method: payment.method ?? '',
      status: payment.status,
      dueDate: payment.dueDate ?? '',
      paidAt: payment.paidAt ?? '',
      notes: payment.notes ?? '',
    },
  })

  const status = watch('status')

  const submit = async (values: PaymentFormValues) => {
    setError(null)
    setSaved(false)
    try {
      const input: PaymentUpdateInput = {
        sessionId: payment.sessionId,
        amount: Number(values.amount),
        method: (values.method || null) as PaymentMethod | null,
        status: values.status as PaymentStatus,
        dueDate: values.dueDate || null,
        paidAt: values.paidAt || null,
        notes: values.notes || null,
      }
      const updated = await paymentsApi.update(payment.id, input)
      onSaved(updated)
      setSaved(true)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar el pago.')
    }
  }

  return (
    <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-4">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <TextField label="Monto (CLP)" type="number" step="1" min="0" {...register('amount')} error={errors.amount?.message} />
        <Select label="Estado" {...register('status')} error={errors.status?.message}>
          {Object.entries(PAYMENT_STATUS_LABELS).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </Select>
      </div>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <Select label="Método de pago" {...register('method')} error={errors.method?.message}>
          <option value="">No informado</option>
          {Object.entries(PAYMENT_METHOD_LABELS).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </Select>
        <TextField label="Fecha de vencimiento" type="date" {...register('dueDate')} error={errors.dueDate?.message} />
      </div>
      {(status === 'pagado' || status === 'condonado') && (
        <TextField label="Fecha de pago" type="date" {...register('paidAt')} error={errors.paidAt?.message} />
      )}
      <Textarea label="Notas administrativas" {...register('notes')} error={errors.notes?.message} />
      {error && <p className="text-sm text-danger">{error}</p>}
      <div className="flex items-center gap-3">
        <Button type="submit" variant="secondary" disabled={isSubmitting}>
          {isSubmitting ? 'Guardando…' : 'Guardar cambios'}
        </Button>
        {saved && !isSubmitting && <span className="text-sm text-success">Guardado.</span>}
      </div>
    </form>
  )
}

export function PaymentDetailScreen() {
  const { patientId, paymentId } = useParams<{ patientId: string; paymentId: string }>()
  const navigate = useNavigate()
  const [payment, setPayment] = useState<Payment | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [confirmingArchive, setConfirmingArchive] = useState(false)
  const [confirmingRestore, setConfirmingRestore] = useState(false)

  const load = () => {
    if (!paymentId) return
    paymentsApi
      .get(paymentId)
      .then(setPayment)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el pago.'))
  }

  useEffect(load, [paymentId])

  const handleArchive = async () => {
    if (!paymentId) return
    try {
      await paymentsApi.archive(paymentId)
      navigate(`/patients/${patientId}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo archivar el pago.')
    }
  }

  const handleRestore = async () => {
    if (!paymentId) return
    try {
      const restored = await paymentsApi.restore(paymentId)
      setPayment(restored)
      setConfirmingRestore(false)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo restaurar el pago.')
    }
  }

  if (error) return <p className="p-10 text-sm text-danger">{error}</p>
  if (!payment) return <p className="p-10 text-sm text-muted-foreground">Cargando…</p>

  const isArchived = payment.deletedAt !== null

  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-8 px-6 py-10">
      {isArchived && (
        <div className="rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
          Este pago está archivado. No aparece en el listado activo hasta que se restaure. Sus datos siguen intactos y
          pueden corregirse.
        </div>
      )}

      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold text-foreground">{formatClp(payment.amount)}</h1>
          <p className="text-sm text-muted-foreground">{effectivePaymentStatusLabel(payment)}</p>
          <button onClick={() => navigate(`/patients/${patientId}`)} className="text-sm text-accent hover:underline">
            Volver a la ficha del paciente
          </button>
        </div>
        <div className="flex gap-2">
          {isArchived ? (
            <Button variant="secondary" onClick={() => setConfirmingRestore(true)}>
              Restaurar
            </Button>
          ) : (
            <Button variant="secondary" onClick={() => setConfirmingArchive(true)}>
              Archivar
            </Button>
          )}
        </div>
      </div>

      {payment.sessionId && (
        <Link
          to={`/patients/${patientId}/sessions/${payment.sessionId}`}
          className="text-sm text-accent hover:underline"
        >
          Ver sesión vinculada
        </Link>
      )}

      <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Información del pago</h3>
        <PaymentMetadataForm payment={payment} onSaved={setPayment} />
      </section>

      {confirmingArchive && (
        <ConfirmDialog
          title="Archivar pago"
          description="El pago se marcará como archivado y dejará de aparecer en el listado activo. No se elimina ninguna información — puede recuperarse más adelante."
          confirmLabel="Archivar"
          onDismiss={() => setConfirmingArchive(false)}
          onConfirm={handleArchive}
        />
      )}

      {confirmingRestore && (
        <ConfirmDialog
          title="Restaurar pago"
          description="El pago volverá a aparecer en el listado activo, con todos sus datos intactos."
          confirmLabel="Restaurar"
          onDismiss={() => setConfirmingRestore(false)}
          onConfirm={handleRestore}
        />
      )}
    </div>
  )
}
