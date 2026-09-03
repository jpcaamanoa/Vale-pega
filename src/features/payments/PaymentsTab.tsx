import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { formatLocalDate } from '../agenda/datetime'
import { formatSessionDate } from '../sessions/datetime'
import { paymentsApi } from './api'
import { formatClp } from './formatCurrency'
import { effectivePaymentStatusLabel, PAYMENT_METHOD_LABELS, type PaymentListItem, type PaymentStatus } from './types'

type ViewMode = 'active' | 'archived'

function statusBadgeClass(status: PaymentStatus, isOverdue: boolean): string {
  if (status === 'pagado') return 'bg-success-soft text-success'
  if (status === 'condonado') return 'bg-accent-soft text-accent'
  if (status === 'atrasado' || (status === 'pendiente' && isOverdue)) return 'bg-danger-soft text-danger'
  return 'bg-disabled text-disabled-foreground'
}

/**
 * Pestaña "Pagos" de la ficha del paciente — reemplaza el placeholder
 * "Próximamente" de la Fase 1.5. Nunca crea pagos nuevos para un paciente
 * archivado (`patientArchived`), pero sigue mostrando el historial
 * completo — archivar un paciente no oculta sus pagos. Mismo patrón que
 * `GoalsTab`/`SessionsTab`.
 */
export function PaymentsTab({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const navigate = useNavigate()
  const [view, setView] = useState<ViewMode>('active')
  const [payments, setPayments] = useState<PaymentListItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    const request = view === 'active' ? paymentsApi.list(patientId) : paymentsApi.listArchived(patientId)
    request
      .then((results) => {
        if (!cancelled) {
          setPayments(results)
          setError(null)
        }
      })
      .catch((err) => {
        if (!cancelled) setError(typeof err === 'string' ? err : 'No se pudieron cargar los pagos.')
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [patientId, view])

  const canCreate = !patientArchived

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <div className="flex gap-1 border-b border-border">
          <button
            onClick={() => setView('active')}
            className={`px-3 py-2 text-sm font-medium transition-colors ${
              view === 'active' ? 'border-b-2 border-accent text-accent' : 'text-muted-foreground hover:text-foreground'
            }`}
          >
            Activos
          </button>
          <button
            onClick={() => setView('archived')}
            className={`px-3 py-2 text-sm font-medium transition-colors ${
              view === 'archived' ? 'border-b-2 border-accent text-accent' : 'text-muted-foreground hover:text-foreground'
            }`}
          >
            Archivados
          </button>
        </div>
        {canCreate && <Button onClick={() => navigate(`/patients/${patientId}/payments/new`)}>Nueva entrada de pago</Button>}
      </div>

      {error && <p className="text-sm text-danger">{error}</p>}
      {loading && <p className="text-sm text-muted-foreground">Cargando…</p>}

      {!loading && payments.length === 0 && (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">
            {view === 'archived' ? 'No hay pagos archivados.' : 'Sin pagos registrados todavía.'}
          </p>
          {view === 'active' && canCreate && (
            <Button onClick={() => navigate(`/patients/${patientId}/payments/new`)}>Nueva entrada de pago</Button>
          )}
        </div>
      )}

      {payments.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-border">
          <table className="w-full text-left text-sm">
            <thead className="bg-surface text-xs uppercase tracking-wide text-muted-foreground">
              <tr>
                <th className="px-4 py-2.5 font-medium">Registrado</th>
                <th className="px-4 py-2.5 font-medium">Monto</th>
                <th className="px-4 py-2.5 font-medium">Estado</th>
                <th className="px-4 py-2.5 font-medium">Método</th>
                <th className="px-4 py-2.5 font-medium">Vencimiento</th>
                <th className="px-4 py-2.5 font-medium">Sesión</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {payments.map((p) => (
                <tr
                  key={p.id}
                  onClick={() => navigate(`/patients/${patientId}/payments/${p.id}`)}
                  className="cursor-pointer hover:bg-accent-soft"
                >
                  <td className="px-4 py-3 text-muted-foreground">{formatLocalDate(p.createdAt)}</td>
                  <td className="px-4 py-3 font-medium text-foreground">{formatClp(p.amount)}</td>
                  <td className="px-4 py-3">
                    <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${statusBadgeClass(p.status, p.isOverdue)}`}>
                      {effectivePaymentStatusLabel(p)}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-muted-foreground">{p.method ? PAYMENT_METHOD_LABELS[p.method] : '—'}</td>
                  <td className="px-4 py-3 text-muted-foreground">{p.dueDate ? formatSessionDate(p.dueDate) : '—'}</td>
                  <td className="px-4 py-3 text-muted-foreground">{p.sessionId ? 'Vinculado a una sesión' : '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
