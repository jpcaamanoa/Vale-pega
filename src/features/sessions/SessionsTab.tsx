import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { sessionsApi } from './api'
import { formatSessionDate } from './datetime'
import { SessionContinuityPanel } from './SessionContinuityPanel'
import { SESSION_MODALITY_LABELS, SESSION_STATUS_LABELS, type SessionListItem } from './types'

type ViewMode = 'active' | 'archived'

/**
 * Pestaña "Sesiones" de la ficha del paciente — reemplaza el placeholder
 * "Próximamente" de la Fase 1.5. Nunca crea sesiones nuevas para un
 * paciente archivado (`patientArchived`), pero sigue mostrando el
 * historial completo — archivar un paciente no oculta sus sesiones.
 */
export function SessionsTab({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const navigate = useNavigate()
  const [view, setView] = useState<ViewMode>('active')
  const [sessions, setSessions] = useState<SessionListItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    const request = view === 'active' ? sessionsApi.list(patientId) : sessionsApi.listArchived(patientId)
    request
      .then((results) => {
        if (!cancelled) {
          setSessions(results)
          setError(null)
        }
      })
      .catch((err) => {
        if (!cancelled) setError(typeof err === 'string' ? err : 'No se pudo cargar las sesiones.')
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
    <div className="flex flex-col gap-6">
      <SessionContinuityPanel patientId={patientId} patientArchived={patientArchived} />

      <div className="flex items-center justify-between">
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
              view === 'archived' ? 'border-b-2 border-accent text-accent' : 'text-muted-foreground hover:text-foreground'
            }`}
          >
            Archivadas
          </button>
        </div>
        {canCreate && <Button onClick={() => navigate(`/patients/${patientId}/sessions/new`)}>Nueva sesión</Button>}
      </div>

      {error && <p className="text-sm text-danger">{error}</p>}
      {loading && <p className="text-sm text-muted-foreground">Cargando…</p>}

      {!loading && sessions.length === 0 && (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">
            {view === 'archived' ? 'No hay sesiones archivadas.' : 'No hay sesiones registradas.'}
          </p>
          {view === 'active' && canCreate && <Button onClick={() => navigate(`/patients/${patientId}/sessions/new`)}>Nueva sesión</Button>}
        </div>
      )}

      {sessions.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-border">
          <table className="w-full text-left text-sm">
            <thead className="bg-surface text-xs uppercase tracking-wide text-muted-foreground">
              <tr>
                <th className="px-4 py-2.5 font-medium">Fecha</th>
                <th className="px-4 py-2.5 font-medium">Hora</th>
                <th className="px-4 py-2.5 font-medium">Duración</th>
                <th className="px-4 py-2.5 font-medium">Modalidad</th>
                <th className="px-4 py-2.5 font-medium">Estado</th>
                <th className="px-4 py-2.5 font-medium">Nota</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {sessions.map((s) => (
                <tr
                  key={s.id}
                  onClick={() => navigate(`/patients/${patientId}/sessions/${s.id}`)}
                  className="cursor-pointer hover:bg-accent-soft"
                >
                  <td className="px-4 py-3 font-medium text-foreground">{formatSessionDate(s.sessionDate)}</td>
                  <td className="px-4 py-3 text-muted-foreground">{s.startTime ?? '—'}</td>
                  <td className="px-4 py-3 text-muted-foreground">{s.durationMinutes ? `${s.durationMinutes} min` : '—'}</td>
                  <td className="px-4 py-3 text-muted-foreground">{s.modality ? SESSION_MODALITY_LABELS[s.modality] : '—'}</td>
                  <td className="px-4 py-3 text-muted-foreground">{SESSION_STATUS_LABELS[s.status]}</td>
                  <td className="px-4 py-3">
                    {s.hasCurrentNote ? (
                      s.currentNoteIsLocked ? (
                        <span className="rounded-full bg-success-soft px-2 py-0.5 text-xs font-medium text-success">Cerrada</span>
                      ) : (
                        <span className="rounded-full bg-warning-soft px-2 py-0.5 text-xs font-medium text-warning">Borrador</span>
                      )
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
