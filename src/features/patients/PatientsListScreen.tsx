import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { useGlobalShortcut } from '../../shared/useGlobalShortcut'
import { patientsApi } from './api'
import { PATIENT_STATUS_LABELS, type PatientListItem } from './types'

const SEARCH_DEBOUNCE_MS = 250

type ViewMode = 'active' | 'archived'

export function PatientsListScreen() {
  const navigate = useNavigate()
  const [view, setView] = useState<ViewMode>('active')
  const [query, setQuery] = useState('')
  const [patients, setPatients] = useState<PatientListItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    const timeout = setTimeout(() => {
      const request = view === 'active' ? patientsApi.list(query) : patientsApi.listArchived(query)
      request
        .then((results) => {
          if (!cancelled) {
            setPatients(results)
            setError(null)
          }
        })
        .catch((err) => {
          if (!cancelled) setError(typeof err === 'string' ? err : 'No se pudo cargar el listado.')
        })
        .finally(() => {
          if (!cancelled) setLoading(false)
        })
    }, SEARCH_DEBOUNCE_MS)
    return () => {
      cancelled = true
      clearTimeout(timeout)
    }
  }, [query, view])

  useGlobalShortcut('n', () => navigate('/patients/new'))

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-10">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-foreground">Pacientes</h1>
        <Button onClick={() => navigate('/patients/new')}>Nuevo paciente</Button>
      </div>

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
            view === 'archived'
              ? 'border-b-2 border-accent text-accent'
              : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          Archivados
        </button>
      </div>

      <input
        type="search"
        placeholder="Buscar por nombre…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        className="w-full rounded-lg border border-border bg-surface px-3 py-2.5 text-sm text-foreground outline-none focus:border-accent focus:ring-1 focus:ring-accent"
        autoFocus
      />

      {error && <p className="text-sm text-danger">{error}</p>}
      {loading && <p className="text-sm text-muted-foreground">Cargando…</p>}

      <div className="overflow-hidden rounded-lg border border-border">
        <table className="w-full text-left text-sm">
          <thead className="bg-surface text-xs uppercase tracking-wide text-muted-foreground">
            <tr>
              <th className="px-4 py-2.5 font-medium">Nombre</th>
              <th className="px-4 py-2.5 font-medium">Estado</th>
              <th className="px-4 py-2.5 font-medium">Ingreso</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {patients.map((p) => (
              <tr
                key={p.id}
                onClick={() => navigate(`/patients/${p.id}`)}
                className="cursor-pointer hover:bg-accent-soft"
              >
                <td className="px-4 py-3">
                  <div className="font-medium text-foreground">{p.fullName}</div>
                  {p.preferredName && <div className="text-xs text-muted-foreground">{p.preferredName}</div>}
                </td>
                <td className="px-4 py-3 text-muted-foreground">{PATIENT_STATUS_LABELS[p.status]}</td>
                <td className="px-4 py-3 text-muted-foreground">{p.intakeDate ?? '—'}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {!loading && patients.length === 0 && (
          <p className="px-4 py-8 text-center text-sm text-muted-foreground">
            {view === 'archived'
              ? query
                ? 'No se encontraron pacientes archivados.'
                : 'No hay pacientes archivados.'
              : query
                ? 'No se encontraron pacientes.'
                : 'Todavía no has creado ningún paciente.'}
          </p>
        )}
      </div>
    </div>
  )
}
