import { useEffect, useState } from 'react'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { Textarea } from '../../components/ui/Textarea'
import { TextField } from '../../components/ui/TextField'
import { goalsApi } from '../goals/api'
import type { GoalListItem } from '../goals/types'
import { therapyTasksApi } from './api'
import { THERAPY_TASK_STATUS_LABELS, type TherapyTaskListItem, type TherapyTaskStatus } from './types'

const RESOLVABLE_STATUSES: TherapyTaskStatus[] = ['parcial', 'realizada', 'no_realizada', 'descartada']

function isOverdue(reviewDueAt: string | null): boolean {
  if (!reviewDueAt) return false
  return reviewDueAt < new Date().toISOString().slice(0, 10)
}

/**
 * Tareas terapéuticas entre sesiones. Se usa tanto dentro de una sesión
 * concreta (con `sessionId`, que queda como `assignedInSessionId` de lo que
 * se cree aquí, y como `reviewedInSessionId` cuando se resuelve una tarea
 * desde esta pantalla) como en la ficha del paciente fuera de cualquier
 * sesión (`sessionId` ausente — resolver una tarea ahí no exige haber
 * pasado por ninguna sesión, por ejemplo para descartarla).
 */
export function TherapyTasksSection({ patientId, sessionId, patientArchived }: { patientId: string; sessionId?: string; patientArchived: boolean }) {
  const [pending, setPending] = useState<TherapyTaskListItem[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const [goals, setGoals] = useState<GoalListItem[] | null>(null)
  const [descriptionDraft, setDescriptionDraft] = useState('')
  const [goalDraft, setGoalDraft] = useState('')
  const [reviewDueDraft, setReviewDueDraft] = useState('')
  const [resolvingId, setResolvingId] = useState<string | null>(null)
  const [resolveStatus, setResolveStatus] = useState<TherapyTaskStatus>('realizada')
  const [showHistory, setShowHistory] = useState(false)
  const [history, setHistory] = useState<TherapyTaskListItem[] | null>(null)

  const load = () => {
    therapyTasksApi
      .listPending(patientId)
      .then(setPending)
      .catch(() => setError('No se pudieron cargar las tareas pendientes.'))
  }

  useEffect(load, [patientId])

  const openAdd = () => {
    setAdding(true)
    setDescriptionDraft('')
    setGoalDraft('')
    setReviewDueDraft('')
    if (!goals) goalsApi.list(patientId).then(setGoals)
  }

  const handleCreate = async () => {
    if (!descriptionDraft.trim()) return
    try {
      await therapyTasksApi.create({
        patientId,
        description: descriptionDraft,
        assignedInSessionId: sessionId ?? null,
        goalId: goalDraft || null,
        reviewDueAt: reviewDueDraft || null,
      })
      setAdding(false)
      load()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar la tarea.')
    }
  }

  const handleResolve = async (id: string) => {
    try {
      await therapyTasksApi.review(id, { status: resolveStatus, reviewedInSessionId: sessionId ?? null })
      setResolvingId(null)
      load()
      if (showHistory) loadHistory()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo actualizar la tarea.')
    }
  }

  const loadHistory = () => {
    therapyTasksApi.list(patientId).then(setHistory)
  }

  const toggleHistory = () => {
    if (showHistory) {
      setShowHistory(false)
      return
    }
    setShowHistory(true)
    loadHistory()
  }

  const reopen = async (id: string) => {
    try {
      await therapyTasksApi.review(id, { status: 'pendiente', reviewedInSessionId: null })
      load()
      loadHistory()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo reabrir la tarea.')
    }
  }

  return (
    <section className="flex flex-col gap-3 rounded-lg border border-border bg-surface p-6">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Tareas entre sesiones</h3>
        {!adding && !patientArchived && (
          <Button variant="secondary" onClick={openAdd}>
            Agregar tarea
          </Button>
        )}
      </div>

      {error && <p className="text-sm text-danger">{error}</p>}

      {adding && (
        <div className="flex flex-col gap-3 rounded-lg border border-border p-4">
          <Textarea label="Descripción de la tarea" value={descriptionDraft} onChange={(e) => setDescriptionDraft(e.target.value)} />
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <Select label="Vincular a objetivo (opcional)" value={goalDraft} onChange={(e) => setGoalDraft(e.target.value)}>
              <option value="">— Sin vincular —</option>
              {goals?.map((g) => (
                <option key={g.id} value={g.id}>
                  {g.title}
                </option>
              ))}
            </Select>
            <TextField label="Revisar antes de (opcional)" type="date" value={reviewDueDraft} onChange={(e) => setReviewDueDraft(e.target.value)} />
          </div>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={() => setAdding(false)}>
              Cancelar
            </Button>
            <Button type="button" onClick={handleCreate} disabled={!descriptionDraft.trim()}>
              Guardar
            </Button>
          </div>
        </div>
      )}

      {pending === null && <p className="text-sm text-muted-foreground">Cargando…</p>}
      {pending !== null && pending.length === 0 && !adding && <p className="text-sm text-muted-foreground">Sin tareas pendientes.</p>}

      {pending !== null && pending.length > 0 && (
        <ul className="flex flex-col gap-2">
          {pending.map((task) => (
            <li key={task.id} className="rounded-lg border border-border p-3">
              <p className="text-sm text-foreground">{task.description}</p>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                {task.goalTitle && <span className="rounded-full bg-accent-soft px-2 py-0.5 text-accent">{task.goalTitle}</span>}
                {task.reviewDueAt && (
                  <span className={isOverdue(task.reviewDueAt) ? 'text-danger' : ''}>
                    Revisar antes de {task.reviewDueAt.split('-').reverse().join('-')}
                  </span>
                )}
              </div>

              {resolvingId === task.id ? (
                <div className="mt-2 flex flex-wrap items-end gap-2">
                  <Select label="Resultado" value={resolveStatus} onChange={(e) => setResolveStatus(e.target.value as TherapyTaskStatus)}>
                    {RESOLVABLE_STATUSES.map((s) => (
                      <option key={s} value={s}>
                        {THERAPY_TASK_STATUS_LABELS[s]}
                      </option>
                    ))}
                  </Select>
                  <Button type="button" variant="secondary" onClick={() => setResolvingId(null)}>
                    Cancelar
                  </Button>
                  <Button type="button" onClick={() => handleResolve(task.id)}>
                    Guardar
                  </Button>
                </div>
              ) : (
                <button
                  onClick={() => {
                    setResolvingId(task.id)
                    setResolveStatus('realizada')
                  }}
                  className="mt-2 text-xs text-accent hover:underline"
                >
                  Marcar revisión
                </button>
              )}
            </li>
          ))}
        </ul>
      )}

      <button onClick={toggleHistory} className="self-start text-xs text-accent hover:underline">
        {showHistory ? 'Ocultar historial' : 'Ver historial'}
      </button>

      {showHistory && (
        <div className="flex flex-col gap-2 border-t border-border pt-3">
          {history === null && <p className="text-sm text-muted-foreground">Cargando…</p>}
          {history !== null && history.length === 0 && <p className="text-sm text-muted-foreground">Sin tareas registradas.</p>}
          {history?.map((task) => (
            <div key={task.id} className="rounded-lg border border-border p-3">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <p className="text-sm text-foreground">{task.description}</p>
                  {task.goalTitle && <span className="mt-1 inline-block rounded-full bg-accent-soft px-2 py-0.5 text-xs text-accent">{task.goalTitle}</span>}
                </div>
                <span
                  className={`shrink-0 rounded-full px-2 py-0.5 text-xs font-medium ${
                    task.status === 'pendiente'
                      ? 'bg-warning-soft text-warning'
                      : task.status === 'realizada'
                        ? 'bg-success-soft text-success'
                        : task.status === 'parcial'
                          ? 'bg-accent-soft text-accent'
                          : 'bg-disabled text-disabled-foreground'
                  }`}
                >
                  {THERAPY_TASK_STATUS_LABELS[task.status]}
                </span>
              </div>
              {task.status !== 'pendiente' && (
                <button onClick={() => reopen(task.id)} className="mt-2 text-xs text-accent hover:underline">
                  ↩ Volver a pendiente
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </section>
  )
}
