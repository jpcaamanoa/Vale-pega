import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { useGlobalShortcut } from '../../shared/useGlobalShortcut'
import { agendaApi } from './api'
import { formatLocalDate, formatLocalTime, startOfDayIsoDaysFromNow, startOfTodayIso } from './datetime'
import { APPOINTMENT_MODALITY_LABELS, type Appointment } from './types'

type ViewMode = 'active' | 'archived'
type RangePreset = 'today' | 'week' | 'all'

const RANGE_LABELS: Record<RangePreset, string> = {
  today: 'Hoy',
  week: 'Próximos 7 días',
  all: 'Todas',
}

function rangeFor(preset: RangePreset): { from?: string; to?: string } {
  switch (preset) {
    case 'today':
      return { from: startOfTodayIso(), to: startOfDayIsoDaysFromNow(1) }
    case 'week':
      return { from: startOfTodayIso(), to: startOfDayIsoDaysFromNow(7) }
    case 'all':
      return {}
  }
}

export function AgendaScreen() {
  const navigate = useNavigate()
  const [view, setView] = useState<ViewMode>('active')
  const [range, setRange] = useState<RangePreset>('today')
  const [appointments, setAppointments] = useState<Appointment[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    const { from, to } = rangeFor(range)
    const request = view === 'active' ? agendaApi.list(from, to) : agendaApi.listArchived(from, to)
    request
      .then((results) => {
        if (!cancelled) {
          setAppointments(results)
          setError(null)
        }
      })
      .catch((err) => {
        if (!cancelled) setError(typeof err === 'string' ? err : 'No se pudo cargar la agenda.')
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [view, range])

  useGlobalShortcut('n', () => navigate('/agenda/new'))

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-10">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-foreground">Agenda</h1>
        <Button onClick={() => navigate('/agenda/new')}>Nueva cita</Button>
      </div>

      <div className="flex gap-1 border-b border-border">
        <button
          onClick={() => setView('active')}
          className={`px-3 py-2 text-sm font-medium transition-colors ${
            view === 'active' ? 'border-b-2 border-accent text-accent' : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          Activas
        </button>
        <button
          onClick={() => setView('archived')}
          className={`px-3 py-2 text-sm font-medium transition-colors ${
            view === 'archived'
              ? 'border-b-2 border-accent text-accent'
              : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          Archivadas
        </button>
      </div>

      <div className="flex gap-2">
        {(Object.entries(RANGE_LABELS) as [RangePreset, string][]).map(([preset, label]) => (
          <button
            key={preset}
            onClick={() => setRange(preset)}
            className={`rounded-full px-3 py-1.5 text-xs font-medium transition-colors ${
              range === preset
                ? 'bg-accent text-accent-foreground'
                : 'border border-border text-muted-foreground hover:text-foreground'
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      {error && <p className="text-sm text-danger">{error}</p>}
      {loading && <p className="text-sm text-muted-foreground">Cargando…</p>}

      <div className="overflow-hidden rounded-lg border border-border">
        <table className="w-full text-left text-sm">
          <thead className="bg-surface text-xs uppercase tracking-wide text-muted-foreground">
            <tr>
              <th className="px-4 py-2.5 font-medium">Fecha</th>
              <th className="px-4 py-2.5 font-medium">Hora</th>
              <th className="px-4 py-2.5 font-medium">Paciente</th>
              <th className="px-4 py-2.5 font-medium">Modalidad</th>
              <th className="px-4 py-2.5 font-medium">Estado</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {appointments.map((a) => (
              <tr key={a.id} onClick={() => navigate(`/agenda/${a.id}`)} className="cursor-pointer hover:bg-accent-soft">
                <td className="px-4 py-3 text-muted-foreground">{formatLocalDate(a.startsAt)}</td>
                <td className="px-4 py-3 text-muted-foreground">
                  {formatLocalTime(a.startsAt)}–{formatLocalTime(a.endsAt)}
                </td>
                <td className="px-4 py-3">
                  <span className="font-medium text-foreground">
                    {a.patientId ? a.patientName : 'Bloqueo personal'}
                  </span>
                </td>
                <td className="px-4 py-3 text-muted-foreground">
                  {a.modality ? APPOINTMENT_MODALITY_LABELS[a.modality] : '—'}
                </td>
                <td className="px-4 py-3">
                  {a.status === 'cancelada' ? (
                    <span className="rounded-full bg-disabled px-2 py-0.5 text-xs font-medium text-disabled-foreground">
                      Cancelada
                    </span>
                  ) : (
                    <span className="text-muted-foreground">Programada</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!loading && appointments.length === 0 && (
          <p className="px-4 py-8 text-center text-sm text-muted-foreground">
            {view === 'archived' ? 'No hay citas archivadas en este rango.' : 'No hay citas en este rango.'}
          </p>
        )}
      </div>
    </div>
  )
}
