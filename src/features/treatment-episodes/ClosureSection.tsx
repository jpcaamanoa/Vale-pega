import { useEffect, useState } from 'react'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import type { GoalListItem } from '../goals/types'
import { prepNotesApi } from '../prep-notes/api'
import type { PatientPrepNote } from '../prep-notes/types'
import { formatSessionDate } from '../sessions/datetime'
import type { SessionListItem } from '../sessions/types'
import { therapyTasksApi } from '../therapy-tasks/api'
import type { TherapyTaskListItem } from '../therapy-tasks/types'
import { episodeClosuresApi, treatmentEpisodesApi } from './api'
import {
  CLOSURE_OUTCOME_LABELS,
  CLOSURE_REASON_LABELS,
  type ClosureOutcome,
  type ClosureReason,
  type EpisodeClosure,
  type SessionResolutionInput,
  type TreatmentEpisode,
} from './types'

/**
 * Cierre estructurado de un proceso terapéutico (Fase 11). Ver
 * `docs/episode-closure.md`. Deliberadamente sin lenguaje técnico ("cierre"
 * es siempre del "proceso", nunca "episode"/"treatment_episode").
 */
export function ClosureSection({
  patientId,
  episode,
  isArchived,
  onEpisodeUpdated,
}: {
  patientId: string
  episode: TreatmentEpisode
  isArchived: boolean
  onEpisodeUpdated: (episode: TreatmentEpisode) => void
}) {
  const [closure, setClosure] = useState<EpisodeClosure | null | undefined>(undefined)
  const [sessions, setSessions] = useState<SessionListItem[] | null>(null)
  const [goals, setGoals] = useState<GoalListItem[] | null>(null)
  const [showCloseModal, setShowCloseModal] = useState(false)
  const [showReopenModal, setShowReopenModal] = useState(false)

  const load = () => {
    if (episode.status !== 'cerrado') {
      setClosure(undefined)
      return
    }
    episodeClosuresApi.getActive(episode.id).then(setClosure)
    treatmentEpisodesApi.listGoals(episode.id).then(setGoals)
    treatmentEpisodesApi.listSessions(episode.id).then(setSessions)
  }

  useEffect(load, [episode.id, episode.status])

  if (episode.status !== 'cerrado') {
    if (isArchived) return null
    return (
      <>
        <div className="mb-6 flex justify-end">
          <Button variant="secondary" onClick={() => setShowCloseModal(true)}>
            Cerrar proceso
          </Button>
        </div>
        {showCloseModal && (
          <CloseEpisodeModal
            episodeId={episode.id}
            patientId={patientId}
            onClosed={(_closure, updatedEpisode) => {
              setShowCloseModal(false)
              onEpisodeUpdated(updatedEpisode)
            }}
            onCancel={() => setShowCloseModal(false)}
          />
        )}
      </>
    )
  }

  return (
    <div className="mb-6 flex flex-col gap-5 rounded-lg border border-border bg-surface p-6">
      <div className="flex items-start justify-between">
        <div>
          <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Cierre del proceso</h3>
          {closure && <p className="mt-1 text-sm text-muted-foreground">Cerrado el {formatSessionDate(closure.closedAt)}</p>}
        </div>
        {!isArchived && (
          <Button variant="secondary" onClick={() => setShowReopenModal(true)}>
            Reabrir proceso
          </Button>
        )}
      </div>

      {closure === undefined && <p className="text-sm text-muted-foreground">Cargando…</p>}
      {closure === null && <p className="text-sm text-muted-foreground">Este proceso está cerrado, pero no se encontró el registro del cierre.</p>}
      {closure && (
        <>
          <ClosureField label="Motivo" value={CLOSURE_REASON_LABELS[closure.reason]} />
          {closure.reason === 'otro' && closure.reasonDetail && <ClosureField label="Detalle" value={closure.reasonDetail} />}
          <ClosureField label="Resultado" value={CLOSURE_OUTCOME_LABELS[closure.outcome]} />
          {closure.summary && <ClosureField label="Resumen del proceso" value={closure.summary} multiline />}
          {closure.recommendations && <ClosureField label="Recomendaciones" value={closure.recommendations} multiline />}
        </>
      )}

      {goals && goals.length > 0 && (
        <div>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Objetivos relacionados</h4>
          <ul className="flex flex-col divide-y divide-border">
            {goals.map((g) => (
              <li key={g.id} className="flex items-center justify-between py-2 text-sm">
                <span className="text-foreground">{g.title}</span>
                <span className="text-xs text-muted-foreground">{g.status}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {sessions && sessions.length > 0 && (
        <div>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Sesiones del proceso</h4>
          <ul className="flex flex-col divide-y divide-border">
            {sessions.map((s) => (
              <li key={s.id} className="flex items-center justify-between py-2 text-sm">
                <span className="text-foreground">{formatSessionDate(s.sessionDate)}</span>
                <span className="text-xs text-muted-foreground">{s.status}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {showReopenModal && closure && (
        <ReopenClosureModal
          closure={closure}
          onReopened={(updatedEpisode) => {
            setShowReopenModal(false)
            onEpisodeUpdated(updatedEpisode)
          }}
          onCancel={() => setShowReopenModal(false)}
        />
      )}
    </div>
  )
}

function ClosureField({ label, value, multiline }: { label: string; value: string; multiline?: boolean }) {
  return (
    <div>
      <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{label}</h4>
      <p className={`text-sm text-foreground ${multiline ? 'whitespace-pre-wrap' : ''}`}>{value}</p>
    </div>
  )
}

function todayDateInputValue(): string {
  const now = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`
}

function CloseEpisodeModal({
  episodeId,
  patientId,
  onClosed,
  onCancel,
}: {
  episodeId: string
  patientId: string
  onClosed: (closure: EpisodeClosure, episode: TreatmentEpisode) => void
  onCancel: () => void
}) {
  const [upcomingSessions, setUpcomingSessions] = useState<SessionListItem[] | null>(null)
  const [pendingTasks, setPendingTasks] = useState<TherapyTaskListItem[] | null>(null)
  const [pendingPrepNotes, setPendingPrepNotes] = useState<PatientPrepNote[] | null>(null)

  const [closedAt, setClosedAt] = useState(todayDateInputValue())
  const [reason, setReason] = useState<ClosureReason | ''>('')
  const [reasonDetail, setReasonDetail] = useState('')
  const [outcome, setOutcome] = useState<ClosureOutcome | ''>('')
  const [summary, setSummary] = useState('')
  const [recommendations, setRecommendations] = useState('')
  const [resolutions, setResolutions] = useState<Record<string, boolean | undefined>>({})

  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    treatmentEpisodesApi.listUpcomingSessions(episodeId).then(setUpcomingSessions)
    therapyTasksApi.listPendingOrPartial(patientId).then(setPendingTasks)
    prepNotesApi.listPending(patientId).then(setPendingPrepNotes)
  }, [episodeId, patientId])

  const allSessionsResolved = upcomingSessions !== null && upcomingSessions.every((s) => resolutions[s.id] !== undefined)
  const reasonDetailOk = reason !== 'otro' || reasonDetail.trim().length > 0
  const canSubmit = reason !== '' && outcome !== '' && reasonDetailOk && allSessionsResolved && !submitting

  const submit = async () => {
    if (reason === '' || outcome === '' || upcomingSessions === null || !allSessionsResolved || !reasonDetailOk || submitting) return
    setSubmitting(true)
    setError(null)
    try {
      const sessionResolutions: SessionResolutionInput[] = upcomingSessions.map((s) => ({ sessionId: s.id, cancel: resolutions[s.id] === true }))
      const [closure, episode] = await episodeClosuresApi.close(episodeId, {
        closedAt,
        reason,
        reasonDetail: reason === 'otro' ? reasonDetail : null,
        outcome,
        summary: summary || null,
        recommendations: recommendations || null,
        sessionResolutions,
      })
      onClosed(closure, episode)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo cerrar el proceso.')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="w-full max-w-lg rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-2 text-base font-semibold text-foreground">Cerrar proceso</h2>
        <p className="mb-4 text-sm text-muted-foreground">
          El proceso pasará a histórico. No podrán crearse sesiones ni objetivos nuevos asociados a él, salvo un reingreso mediante un proceso nuevo.
        </p>

        <div className="flex flex-col gap-4">
          <TextField label="Fecha de término" type="date" value={closedAt} onChange={(e) => setClosedAt(e.target.value)} />

          <Select label="Motivo" value={reason} onChange={(e) => setReason(e.target.value as ClosureReason)}>
            <option value="" disabled>
              Selecciona un motivo…
            </option>
            {Object.entries(CLOSURE_REASON_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </Select>

          {reason === 'otro' && <TextField label="Detalle del motivo" value={reasonDetail} onChange={(e) => setReasonDetail(e.target.value)} />}

          <Select label="Resultado" value={outcome} onChange={(e) => setOutcome(e.target.value as ClosureOutcome)}>
            <option value="" disabled>
              Selecciona un resultado…
            </option>
            {Object.entries(CLOSURE_OUTCOME_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </Select>

          <Textarea label="Resumen del proceso (opcional)" value={summary} onChange={(e) => setSummary(e.target.value)} />
          <Textarea label="Recomendaciones (opcional)" value={recommendations} onChange={(e) => setRecommendations(e.target.value)} />

          {pendingTasks !== null && pendingTasks.length > 0 && (
            <div className="rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
              Este paciente tiene {pendingTasks.length} tarea(s) pendiente(s) o parcial(es). No se modificarán ni se perderán al cerrar el proceso.
            </div>
          )}
          {pendingPrepNotes !== null && pendingPrepNotes.length > 0 && (
            <div className="rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
              Este paciente tiene {pendingPrepNotes.length} preparación(es) para próxima sesión pendiente(s). No se modificarán ni se perderán al cerrar el proceso.
            </div>
          )}

          {upcomingSessions === null && <p className="text-sm text-muted-foreground">Revisando sesiones futuras del proceso…</p>}
          {upcomingSessions !== null && upcomingSessions.length > 0 && (
            <div className="rounded-lg border border-border p-4">
              <h4 className="mb-2 text-sm font-medium text-foreground">Sesiones futuras agendadas de este proceso</h4>
              <p className="mb-3 text-xs text-muted-foreground">Debes decidir qué hacer con cada una antes de poder cerrar el proceso.</p>
              <div className="flex flex-col gap-3">
                {upcomingSessions.map((s) => (
                  <div key={s.id} className="flex items-center justify-between gap-3 rounded-md border border-border p-3">
                    <span className="text-sm text-foreground">{formatSessionDate(s.sessionDate)}</span>
                    <div className="flex gap-3 text-sm">
                      <label className="flex items-center gap-1.5">
                        <input type="radio" name={`resolution-${s.id}`} checked={resolutions[s.id] === false} onChange={() => setResolutions((r) => ({ ...r, [s.id]: false }))} />
                        Mantener
                      </label>
                      <label className="flex items-center gap-1.5">
                        <input type="radio" name={`resolution-${s.id}`} checked={resolutions[s.id] === true} onChange={() => setResolutions((r) => ({ ...r, [s.id]: true }))} />
                        Cancelar
                      </label>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {error && <p className="text-sm text-danger">{error}</p>}

          <div className="mt-2 flex justify-end gap-2">
            <Button variant="secondary" onClick={onCancel} disabled={submitting}>
              Cancelar
            </Button>
            <Button onClick={submit} disabled={!canSubmit}>
              {submitting ? 'Cerrando…' : 'Cerrar proceso'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

function ReopenClosureModal({ closure, onReopened, onCancel }: { closure: EpisodeClosure; onReopened: (episode: TreatmentEpisode) => void; onCancel: () => void }) {
  const [revertedReason, setRevertedReason] = useState('')
  const [reopenStatus, setReopenStatus] = useState<'activo' | 'pausado'>('activo')
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async () => {
    if (!revertedReason.trim()) return
    setSubmitting(true)
    setError(null)
    try {
      const [, episode] = await episodeClosuresApi.revert(closure.id, { revertedReason, reopenStatus })
      onReopened(episode)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo reabrir el proceso.')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
      <div className="w-full max-w-md rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-2 text-base font-semibold text-foreground">Reabrir proceso</h2>
        <p className="mb-4 text-sm text-muted-foreground">
          El cierre actual quedará registrado como anulado, conservando su motivo y resumen originales — nunca se borra. El proceso volverá a estar activo o pausado, según elijas.
        </p>

        <div className="flex flex-col gap-4">
          <TextField label="Motivo de la reapertura" value={revertedReason} onChange={(e) => setRevertedReason(e.target.value)} />

          <div className="flex gap-4 text-sm">
            <label className="flex items-center gap-1.5">
              <input type="radio" checked={reopenStatus === 'activo'} onChange={() => setReopenStatus('activo')} />
              Volver a activo
            </label>
            <label className="flex items-center gap-1.5">
              <input type="radio" checked={reopenStatus === 'pausado'} onChange={() => setReopenStatus('pausado')} />
              Volver a pausado
            </label>
          </div>

          {error && <p className="text-sm text-danger">{error}</p>}

          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={onCancel} disabled={submitting}>
              Cancelar
            </Button>
            <Button onClick={submit} disabled={submitting || !revertedReason.trim()}>
              {submitting ? 'Reabriendo…' : 'Reabrir proceso'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
