import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { formatSessionDate } from '../sessions/datetime'
import { SESSION_STATUS_LABELS, type SessionStatus } from '../sessions/types'
import { goalsApi } from './api'
import { goalIndicatorFormSchema, goalUpdateFormSchema, type GoalIndicatorFormValues, type GoalUpdateFormValues } from './schema'
import { GOAL_STATUS_LABELS, type Goal, type GoalIndicator, type GoalIndicatorInput, type GoalSessionRow, type GoalUpdateInput } from './types'

function ConfirmDialog({
  title,
  description,
  confirmLabel,
  onDismiss,
  onConfirm,
}: {
  title: string
  description: string
  confirmLabel: string
  onDismiss: () => void
  onConfirm: () => void
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
      <div className="w-full max-w-sm rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-2 text-base font-semibold text-foreground">{title}</h2>
        <p className="mb-4 text-sm text-muted-foreground">{description}</p>
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onDismiss}>
            Volver
          </Button>
          <Button onClick={onConfirm}>{confirmLabel}</Button>
        </div>
      </div>
    </div>
  )
}

function GoalMetadataForm({ goal, onSaved }: { goal: Goal; onSaved: (goal: Goal) => void }) {
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<GoalUpdateFormValues>({
    resolver: zodResolver(goalUpdateFormSchema),
    defaultValues: {
      title: goal.title,
      description: goal.description ?? '',
      status: goal.status,
      targetDate: goal.targetDate ?? '',
    },
  })

  const submit = async (values: GoalUpdateFormValues) => {
    setError(null)
    setSaved(false)
    try {
      const input: GoalUpdateInput = {
        title: values.title,
        description: values.description || null,
        status: values.status,
        targetDate: values.targetDate || null,
      }
      const updated = await goalsApi.update(goal.id, input)
      onSaved(updated)
      setSaved(true)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar el objetivo.')
    }
  }

  return (
    <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-4">
      <TextField label="Título" {...register('title')} error={errors.title?.message} />
      <Textarea label="Descripción" {...register('description')} error={errors.description?.message} />
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <Select label="Estado" {...register('status')} error={errors.status?.message}>
          {Object.entries(GOAL_STATUS_LABELS).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </Select>
        <TextField label="Fecha objetivo" type="date" {...register('targetDate')} error={errors.targetDate?.message} />
      </div>
      {error && <p className="text-sm text-danger">{error}</p>}
      <div className="flex items-center gap-3">
        <Button type="submit" variant="secondary" disabled={isSubmitting}>
          {isSubmitting ? 'Guardando…' : 'Guardar cambios'}
        </Button>
        {saved && !isSubmitting && <span className="text-sm text-success">Guardado.</span>}
      </div>
    </form>
  )
}

function IndicatorForm({
  initial,
  onCancel,
  onSubmit,
  submitLabel,
}: {
  initial?: GoalIndicator
  onCancel: () => void
  onSubmit: (input: GoalIndicatorInput) => Promise<void>
  submitLabel: string
}) {
  const [error, setError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<GoalIndicatorFormValues>({
    resolver: zodResolver(goalIndicatorFormSchema),
    defaultValues: {
      description: initial?.description ?? '',
      baselineValue: initial?.baselineValue ?? '',
      targetValue: initial?.targetValue ?? '',
    },
  })

  const submit = async (values: GoalIndicatorFormValues) => {
    setError(null)
    try {
      await onSubmit({ description: values.description, baselineValue: values.baselineValue || null, targetValue: values.targetValue || null })
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar el indicador.')
    }
  }

  return (
    <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-3 rounded-lg border border-border bg-surface p-4">
      <TextField label="Indicador" {...register('description')} error={errors.description?.message} />
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <TextField label="Valor de partida" {...register('baselineValue')} />
        <TextField label="Valor a alcanzar" {...register('targetValue')} />
      </div>
      {error && <p className="text-sm text-danger">{error}</p>}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="secondary" onClick={onCancel} disabled={isSubmitting}>
          Cancelar
        </Button>
        <Button type="submit" disabled={isSubmitting}>
          {isSubmitting ? 'Guardando…' : submitLabel}
        </Button>
      </div>
    </form>
  )
}

export function GoalDetailScreen() {
  const { patientId, goalId } = useParams<{ patientId: string; goalId: string }>()
  const navigate = useNavigate()
  const [goal, setGoal] = useState<Goal | null>(null)
  const [indicators, setIndicators] = useState<GoalIndicator[] | null>(null)
  const [relatedSessions, setRelatedSessions] = useState<GoalSessionRow[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [confirmingArchive, setConfirmingArchive] = useState(false)
  const [confirmingRestore, setConfirmingRestore] = useState(false)
  const [addingIndicator, setAddingIndicator] = useState(false)
  const [editingIndicatorId, setEditingIndicatorId] = useState<string | null>(null)
  const [deletingIndicator, setDeletingIndicator] = useState<GoalIndicator | null>(null)

  const load = () => {
    if (!goalId) return
    setError(null)
    Promise.all([goalsApi.get(goalId), goalsApi.listIndicators(goalId), goalsApi.listSessionsForGoal(goalId)])
      .then(([g, i, s]) => {
        setGoal(g)
        setIndicators(i)
        setRelatedSessions(s)
      })
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el objetivo.'))
  }

  useEffect(load, [goalId])

  const reloadIndicators = () => {
    if (!goalId) return
    goalsApi.listIndicators(goalId).then(setIndicators)
  }

  const handleArchive = async () => {
    if (!goalId) return
    try {
      await goalsApi.archive(goalId)
      navigate(`/patients/${patientId}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo archivar el objetivo.')
    }
  }

  const handleRestore = async () => {
    if (!goalId) return
    try {
      const restored = await goalsApi.restore(goalId)
      setGoal(restored)
      setConfirmingRestore(false)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo restaurar el objetivo.')
    }
  }

  const handleCreateIndicator = async (input: GoalIndicatorInput) => {
    if (!goalId) return
    await goalsApi.createIndicator(goalId, input)
    setAddingIndicator(false)
    reloadIndicators()
  }

  const handleUpdateIndicator = async (indicatorId: string, input: GoalIndicatorInput) => {
    await goalsApi.updateIndicator(indicatorId, input)
    setEditingIndicatorId(null)
    reloadIndicators()
  }

  const handleDeleteIndicator = async () => {
    if (!deletingIndicator) return
    try {
      await goalsApi.deleteIndicator(deletingIndicator.id)
      setDeletingIndicator(null)
      reloadIndicators()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo eliminar el indicador.')
    }
  }

  if (error) return <p className="p-10 text-sm text-danger">{error}</p>
  if (!goal || !indicators || !relatedSessions) return <p className="p-10 text-sm text-muted-foreground">Cargando…</p>

  const isArchived = goal.deletedAt !== null

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-8 px-6 py-10">
      {isArchived && (
        <div className="rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
          Este objetivo está archivado. No aparece en el listado activo hasta que se restaure. Sus indicadores y
          sesiones relacionadas siguen intactos.
        </div>
      )}

      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold text-foreground">{goal.title}</h1>
          <button onClick={() => navigate(`/patients/${patientId}`)} className="text-sm text-accent hover:underline">
            Volver a la ficha del paciente
          </button>
        </div>
        <div className="flex gap-2">
          {isArchived ? (
            <Button variant="secondary" onClick={() => setConfirmingRestore(true)}>
              Restaurar
            </Button>
          ) : (
            <Button variant="secondary" onClick={() => setConfirmingArchive(true)}>
              Archivar
            </Button>
          )}
        </div>
      </div>

      <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Información del objetivo</h3>
        <GoalMetadataForm goal={goal} onSaved={setGoal} />
      </section>

      <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Indicadores</h3>
          {!addingIndicator && (
            <Button variant="secondary" onClick={() => setAddingIndicator(true)}>
              Agregar indicador
            </Button>
          )}
        </div>
        <p className="text-xs text-muted-foreground">
          Un indicador describe qué se está midiendo, con un valor de partida opcional y el valor que se espera
          alcanzar — en texto libre, sin cálculos automáticos.
        </p>

        {indicators.length === 0 && !addingIndicator && (
          <p className="py-6 text-center text-sm text-muted-foreground">Este objetivo todavía no tiene indicadores.</p>
        )}

        <div className="flex flex-col gap-3">
          {indicators.map((indicator) =>
            editingIndicatorId === indicator.id ? (
              <IndicatorForm
                key={indicator.id}
                initial={indicator}
                submitLabel="Guardar"
                onCancel={() => setEditingIndicatorId(null)}
                onSubmit={(input) => handleUpdateIndicator(indicator.id, input)}
              />
            ) : (
              <div key={indicator.id} className="flex items-center justify-between gap-4 rounded-lg border border-border p-4">
                <div>
                  <p className="text-sm font-medium text-foreground">{indicator.description}</p>
                  {(indicator.baselineValue || indicator.targetValue) && (
                    <p className="mt-1 text-xs text-muted-foreground">
                      {indicator.baselineValue ? `Partida: ${indicator.baselineValue}` : ''}
                      {indicator.baselineValue && indicator.targetValue ? ' — ' : ''}
                      {indicator.targetValue ? `Meta: ${indicator.targetValue}` : ''}
                    </p>
                  )}
                </div>
                <div className="flex shrink-0 gap-3">
                  <button onClick={() => setEditingIndicatorId(indicator.id)} className="text-sm text-accent hover:underline">
                    Editar
                  </button>
                  <button onClick={() => setDeletingIndicator(indicator)} className="text-sm text-danger hover:underline">
                    Eliminar
                  </button>
                </div>
              </div>
            ),
          )}
        </div>

        {addingIndicator && <IndicatorForm submitLabel="Agregar" onCancel={() => setAddingIndicator(false)} onSubmit={handleCreateIndicator} />}
      </section>

      <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Sesiones relacionadas</h3>
        {relatedSessions.length === 0 ? (
          <p className="py-6 text-center text-sm text-muted-foreground">Este objetivo todavía no se ha trabajado en ninguna sesión.</p>
        ) : (
          <div className="flex flex-col gap-3">
            {relatedSessions.map((s) => (
              <Link
                key={s.sessionId}
                to={`/patients/${patientId}/sessions/${s.sessionId}`}
                className="flex items-center justify-between gap-4 rounded-lg border border-border p-4 hover:bg-accent-soft"
              >
                <div>
                  <p className="text-sm font-medium text-foreground">
                    {formatSessionDate(s.sessionDate)}
                    {s.startTime ? ` · ${s.startTime}` : ''}
                  </p>
                  {s.progressNote && <p className="mt-1 text-xs text-muted-foreground">{s.progressNote}</p>}
                </div>
                <span className="shrink-0 text-xs text-muted-foreground">{SESSION_STATUS_LABELS[s.sessionStatus as SessionStatus]}</span>
              </Link>
            ))}
          </div>
        )}
      </section>

      {confirmingArchive && (
        <ConfirmDialog
          title="Archivar objetivo"
          description="El objetivo se marcará como archivado y dejará de aparecer en el listado activo. No se elimina ninguna información — sus indicadores y sesiones relacionadas permanecen intactos y pueden recuperarse más adelante."
          confirmLabel="Archivar"
          onDismiss={() => setConfirmingArchive(false)}
          onConfirm={handleArchive}
        />
      )}

      {confirmingRestore && (
        <ConfirmDialog
          title="Restaurar objetivo"
          description="El objetivo volverá a aparecer en el listado activo, con sus indicadores y sesiones relacionadas intactos."
          confirmLabel="Restaurar"
          onDismiss={() => setConfirmingRestore(false)}
          onConfirm={handleRestore}
        />
      )}

      {deletingIndicator && (
        <ConfirmDialog
          title="Eliminar indicador"
          description={`Se eliminará el indicador "${deletingIndicator.description}". Esta acción no se puede deshacer.`}
          confirmLabel="Eliminar"
          onDismiss={() => setDeletingIndicator(null)}
          onConfirm={handleDeleteIndicator}
        />
      )}
    </div>
  )
}
