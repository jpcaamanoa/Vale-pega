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
        <h1 className="text-xl font-semibold text-slate-900">Pacientes</h1>
        <Button onClick={() => navigate('/patients/new')}>Nuevo paciente</Button>
      </div>

      <div className="flex gap-1 border-b border-slate-200">
        <button
          onClick={() => setView('active')}
          className={`px-3 py-2 text-sm font-medium transition-colors ${
            view === 'active'
              ? 'border-b-2 border-slate-900 text-slate-900'
              : 'text-slate-500 hover:text-slate-800'
          }`}
        >
          Activos
        </button>
        <button
          onClick={() => setView('archived')}
          className={`px-3 py-2 text-sm font-medium transition-colors ${
            view === 'archived'
              ? 'border-b-2 border-slate-900 text-slate-900'
              : 'text-slate-500 hover:text-slate-800'
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
        className="w-full rounded-lg border border-slate-300 px-3 py-2.5 text-sm outline-none focus:border-slate-500 focus:ring-1 focus:ring-slate-500"
        autoFocus
      />

      {error && <p className="text-sm text-red-600">{error}</p>}
      {loading && <p className="text-sm text-slate-400">Cargando…</p>}

      <div className="overflow-hidden rounded-lg border border-slate-200">
        <table className="w-full text-left text-sm">
          <thead className="bg-slate-50 text-xs uppercase tracking-wide text-slate-500">
            <tr>
              <th className="px-4 py-2.5 font-medium">Nombre</th>
              <th className="px-4 py-2.5 font-medium">Estado</th>
              <th className="px-4 py-2.5 font-medium">Ingreso</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100">
            {patients.map((p) => (
              <tr
                key={p.id}
                onClick={() => navigate(`/patients/${p.id}`)}
                className="cursor-pointer hover:bg-slate-50"
              >
                <td className="px-4 py-3">
                  <div className="font-medium text-slate-900">{p.fullName}</div>
                  {p.preferredName && <div className="text-xs text-slate-500">{p.preferredName}</div>}
                </td>
                <td className="px-4 py-3 text-slate-600">{PATIENT_STATUS_LABELS[p.status]}</td>
                <td className="px-4 py-3 text-slate-600">{p.intakeDate ?? '—'}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {!loading && patients.length === 0 && (
          <p className="px-4 py-8 text-center text-sm text-slate-400">
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
