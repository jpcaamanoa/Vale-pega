import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { useGlobalShortcut } from '../../shared/useGlobalShortcut'
import { patientsApi } from './api'
import { PATIENT_STATUS_LABELS, type PatientListItem } from './types'

const SEARCH_DEBOUNCE_MS = 250

export function PatientsListScreen() {
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const [patients, setPatients] = useState<PatientListItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    const timeout = setTimeout(() => {
      patientsApi
        .list(query)
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
  }, [query])

  useGlobalShortcut('n', () => navigate('/patients/new'))

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-10">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-slate-900">Pacientes</h1>
        <Button onClick={() => navigate('/patients/new')}>Nuevo paciente</Button>
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
            {query ? 'No se encontraron pacientes.' : 'Todavía no has creado ningún paciente.'}
          </p>
        )}
      </div>
    </div>
  )
}
