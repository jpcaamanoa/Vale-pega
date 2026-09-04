import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { formatSessionDate } from '../sessions/datetime'
import { treatmentEpisodesApi } from './api'
import { TREATMENT_EPISODE_STATUS_LABELS, type TreatmentEpisode } from './types'

/**
 * Pestaña "Procesos" de la ficha del paciente (Fase 9) — resuelve el
 * problema estructural "paciente ≠ proceso" identificado en la auditoría
 * post Fase 8. Deliberadamente pequeña: muestra el proceso activo (si hay
 * uno), permite iniciar uno nuevo cuando no lo hay, y lista los procesos
 * anteriores. Ningún gráfico, ninguna estadística — eso queda para fases
 * futuras. Ver `docs/treatment-episodes.md`.
 */
export function ProcessesTab({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const navigate = useNavigate()
  const [episodes, setEpisodes] = useState<TreatmentEpisode[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const [startedAt, setStartedAt] = useState('')
  const [createError, setCreateError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const load = () => {
    treatmentEpisodesApi
      .list(patientId)
      .then((results) => {
        setEpisodes(results)
        setError(null)
      })
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudieron cargar los procesos terapéuticos.'))
  }

  useEffect(load, [patientId])

  const startProcess = async () => {
    setSubmitting(true)
    setCreateError(null)
    try {
      await treatmentEpisodesApi.create({ patientId, startedAt: startedAt || null })
      setCreating(false)
      setStartedAt('')
      load()
    } catch (err) {
      setCreateError(typeof err === 'string' ? err : 'No se pudo iniciar el proceso.')
    } finally {
      setSubmitting(false)
    }
  }

  if (error) return <p className="text-sm text-danger">{error}</p>
  if (episodes === null) return <p className="text-sm text-muted-foreground">Cargando…</p>

  const active = episodes.find((e) => e.status === 'activo')
  const previous = episodes.filter((e) => e.id !== active?.id)
  const canStart = !patientArchived && !active

  return (
    <div className="flex flex-col gap-6">
      <section className="rounded-lg border border-border bg-surface p-6">
        <h3 className="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">Proceso actual</h3>
        {active ? (
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm text-foreground">Iniciado el {formatSessionDate(active.startedAt)}</p>
              <p className="text-sm text-muted-foreground">{TREATMENT_EPISODE_STATUS_LABELS[active.status]}</p>
            </div>
            <Button variant="secondary" onClick={() => navigate(`/patients/${patientId}/episodes/${active.id}`)}>
              Ver proceso
            </Button>
          </div>
        ) : (
          <div className="flex flex-col items-center gap-3 py-8 text-center">
            <p className="text-sm text-muted-foreground">Sin proceso activo.</p>
            {canStart && !creating && <Button onClick={() => setCreating(true)}>Iniciar proceso</Button>}
          </div>
        )}

        {creating && (
          <div className="mt-4 flex flex-col gap-3 rounded-lg border border-border p-4">
            <label className="flex flex-col gap-1 text-sm">
              <span className="text-foreground">Fecha de inicio (opcional — hoy si se deja vacío)</span>
              <input
                type="date"
                value={startedAt}
                onChange={(e) => setStartedAt(e.target.value)}
                className="rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground"
              />
            </label>
            {createError && <p className="text-sm text-danger">{createError}</p>}
            <div className="flex justify-end gap-2">
              <Button type="button" variant="secondary" onClick={() => setCreating(false)} disabled={submitting}>
                Cancelar
              </Button>
              <Button type="button" onClick={startProcess} disabled={submitting}>
                {submitting ? 'Iniciando…' : 'Iniciar proceso'}
              </Button>
            </div>
          </div>
        )}
      </section>

      {previous.length > 0 && (
        <section className="rounded-lg border border-border bg-surface p-6">
          <h3 className="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">Procesos anteriores</h3>
          <ul className="flex flex-col divide-y divide-border">
            {previous.map((ep) => (
              <li key={ep.id}>
                <button
                  onClick={() => navigate(`/patients/${patientId}/episodes/${ep.id}`)}
                  className="flex w-full items-center justify-between py-2.5 text-left hover:text-accent"
                >
                  <span className="text-sm text-foreground">Iniciado el {formatSessionDate(ep.startedAt)}</span>
                  <span className="text-xs text-muted-foreground">{TREATMENT_EPISODE_STATUS_LABELS[ep.status]}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  )
}
