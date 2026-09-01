import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { formatSessionDate } from '../sessions/datetime'
import { goalsApi } from './api'
import { GOAL_STATUS_LABELS, type GoalListItem } from './types'

type ViewMode = 'active' | 'archived'

/**
 * Pestaña "Objetivos" de la ficha del paciente — reemplaza el placeholder
 * "Próximamente" de la Fase 1.5. Nunca crea objetivos nuevos para un
 * paciente archivado (`patientArchived`), pero sigue mostrando el
 * historial completo — archivar un paciente no oculta sus objetivos.
 * Mismo patrón que `SessionsTab` (Fase 4).
 */
export function GoalsTab({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const navigate = useNavigate()
  const [view, setView] = useState<ViewMode>('active')
  const [goals, setGoals] = useState<GoalListItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    const request = view === 'active' ? goalsApi.list(patientId) : goalsApi.listArchived(patientId)
    request
      .then((results) => {
        if (!cancelled) {
          setGoals(results)
          setError(null)
        }
      })
      .catch((err) => {
        if (!cancelled) setError(typeof err === 'string' ? err : 'No se pudo cargar los objetivos.')
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
        {canCreate && <Button onClick={() => navigate(`/patients/${patientId}/goals/new`)}>Nuevo objetivo</Button>}
      </div>

      {error && <p className="text-sm text-danger">{error}</p>}
      {loading && <p className="text-sm text-muted-foreground">Cargando…</p>}

      {!loading && goals.length === 0 && (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">
            {view === 'archived' ? 'No hay objetivos archivados.' : 'No hay objetivos registrados todavía.'}
          </p>
          {view === 'active' && canCreate && <Button onClick={() => navigate(`/patients/${patientId}/goals/new`)}>Nuevo objetivo</Button>}
        </div>
      )}

      {goals.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-border">
          <table className="w-full text-left text-sm">
            <thead className="bg-surface text-xs uppercase tracking-wide text-muted-foreground">
              <tr>
                <th className="px-4 py-2.5 font-medium">Título</th>
                <th className="px-4 py-2.5 font-medium">Estado</th>
                <th className="px-4 py-2.5 font-medium">Fecha objetivo</th>
                <th className="px-4 py-2.5 font-medium">Indicadores</th>
                <th className="px-4 py-2.5 font-medium">Sesiones</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {goals.map((g) => (
                <tr
                  key={g.id}
                  onClick={() => navigate(`/patients/${patientId}/goals/${g.id}`)}
                  className="cursor-pointer hover:bg-accent-soft"
                >
                  <td className="px-4 py-3 font-medium text-foreground">{g.title}</td>
                  <td className="px-4 py-3 text-muted-foreground">{GOAL_STATUS_LABELS[g.status]}</td>
                  <td className="px-4 py-3 text-muted-foreground">{g.targetDate ? formatSessionDate(g.targetDate) : '—'}</td>
                  <td className="px-4 py-3 text-muted-foreground">{g.indicatorCount}</td>
                  <td className="px-4 py-3 text-muted-foreground">{g.sessionCount}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
