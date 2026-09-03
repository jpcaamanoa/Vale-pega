import { zodResolver } from '@hookform/resolvers/zod'
import { useState } from 'react'
import { useForm } from 'react-hook-form'
import { useLocation, useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { Textarea } from '../../components/ui/Textarea'
import { TextField } from '../../components/ui/TextField'
import { paymentsApi } from './api'
import { paymentFormSchema, type PaymentFormValues } from './schema'
import { PAYMENT_METHOD_LABELS, PAYMENT_STATUS_LABELS, type PaymentInput, type PaymentMethod, type PaymentStatus } from './types'

/**
 * Se usa tanto desde la ficha del paciente ("Nueva entrada de pago") como
 * desde el botón "Registrar pago" de `SessionDetailScreen` — es la misma
 * pantalla, el mismo formulario y las mismas reglas de negocio en ambos
 * casos. Cuando llega desde una sesión, `location.state.sessionId` viene
 * precargado y se muestra como referencia fija, no como un selector — no
 * existe en esta fase una forma de elegir la sesión desde este formulario,
 * el backend igual valida `session.patient_id == payment.patient_id`.
 */
export function PaymentCreateScreen() {
  const { patientId } = useParams<{ patientId: string }>()
  const navigate = useNavigate()
  const location = useLocation()
  const sessionId = (location.state as { sessionId?: string } | null)?.sessionId ?? null
  const [error, setError] = useState<string | null>(null)

  const {
    register,
    handleSubmit,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<PaymentFormValues>({
    resolver: zodResolver(paymentFormSchema),
    defaultValues: { amount: '', method: '', status: 'pendiente', dueDate: '', paidAt: '', notes: '' },
  })

  const status = watch('status')

  if (!patientId) return null

  const submit = async (values: PaymentFormValues) => {
    setError(null)
    try {
      const input: PaymentInput = {
        patientId,
        sessionId,
        amount: Number(values.amount),
        method: (values.method || null) as PaymentMethod | null,
        status: values.status as PaymentStatus,
        dueDate: values.dueDate || null,
        paidAt: values.paidAt || null,
        notes: values.notes || null,
      }
      const created = await paymentsApi.create(input)
      navigate(`/patients/${patientId}/payments/${created.id}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo registrar el pago.')
    }
  }

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="mb-6 text-xl font-semibold text-foreground">Nueva entrada de pago</h1>
      {sessionId && (
        <p className="mb-4 rounded-lg border border-border bg-accent-soft px-3 py-2 text-xs text-accent">
          Se vinculará a la sesión desde la que se abrió este formulario.
        </p>
      )}
      {error && <p className="mb-4 text-sm text-danger">{error}</p>}
      <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-6">
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

        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" variant="secondary" onClick={() => navigate(`/patients/${patientId}`)}>
            Cancelar
          </Button>
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? 'Guardando…' : 'Registrar pago'}
          </Button>
        </div>
      </form>
    </div>
  )
}
