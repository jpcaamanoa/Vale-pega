import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { formatSessionDate } from '../sessions/datetime'
import { episodeClinicalProfileApi, treatmentEpisodesApi } from './api'
import { episodeClinicalProfileFormSchema, type EpisodeClinicalProfileFormValues } from './schema'
import { TREATMENT_EPISODE_STATUS_LABELS, type EpisodeClinicalProfile, type EpisodeClinicalProfileInput, type TreatmentEpisode } from './types'

function ProfileField({ label, value }: { label: string; value: string | null }) {
  return (
    <div>
      <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{label}</h4>
      <p className="whitespace-pre-wrap text-sm text-foreground">{value || '—'}</p>
    </div>
  )
}

function EpisodeClinicalProfileForm({
  episodeId,
  initial,
  mode,
  onSaved,
  onCancel,
}: {
  episodeId: string
  initial?: EpisodeClinicalProfile
  mode: 'create' | 'edit'
  onSaved: (profile: EpisodeClinicalProfile) => void
  onCancel: () => void
}) {
  const [error, setError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    formState: { isSubmitting },
  } = useForm<EpisodeClinicalProfileFormValues>({
    resolver: zodResolver(episodeClinicalProfileFormSchema),
    defaultValues: {
      presentingProblem: initial?.presentingProblem ?? '',
      primaryDiagnosisCode: initial?.primaryDiagnosisCode ?? '',
      diagnosisNotes: initial?.diagnosisNotes ?? '',
    },
  })

  const submit = async (values: EpisodeClinicalProfileFormValues) => {
    setError(null)
    try {
      const input: EpisodeClinicalProfileInput = {
        presentingProblem: values.presentingProblem || null,
        primaryDiagnosisCode: values.primaryDiagnosisCode || null,
        diagnosisNotes: values.diagnosisNotes || null,
      }
      const saved = mode === 'create' ? await episodeClinicalProfileApi.create(episodeId, input) : await episodeClinicalProfileApi.update(episodeId, input)
      onSaved(saved)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudieron guardar los antecedentes del proceso.')
    }
  }

  return (
    <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
      <TextField label="Motivo de consulta" {...register('presentingProblem')} />
      <TextField label="Código de diagnóstico principal" {...register('primaryDiagnosisCode')} />
      <Textarea label="Notas diagnósticas" {...register('diagnosisNotes')} />
      {error && <p className="text-sm text-danger">{error}</p>}
      <div className="flex justify-end gap-2 pt-2">
        <Button type="button" variant="secondary" onClick={onCancel} disabled={isSubmitting}>
          Cancelar
        </Button>
        <Button type="submit" disabled={isSubmitting}>
          {isSubmitting ? 'Guardando…' : 'Guardar'}
        </Button>
      </div>
    </form>
  )
}

/**
 * Detalle de un proceso terapéutico (Fase 9). Deliberadamente sin la
 * acción de "cerrar definitivamente" — el valor `'cerrado'` existe en el
 * backend para preparar el modelo, pero el cierre estructurado (motivo,
 * resumen, objetivos alcanzados) es Fase 10. Aquí solo se puede
 * pausar/reactivar y archivar/restaurar el registro administrativo.
 */
export function TreatmentEpisodeDetailScreen() {
  const { patientId, episodeId } = useParams<{ patientId: string; episodeId: string }>()
  const navigate = useNavigate()
  const [episode, setEpisode] = useState<TreatmentEpisode | null>(null)
  const [profile, setProfile] = useState<EpisodeClinicalProfile | null | undefined>(undefined)
  const [error, setError] = useState<string | null>(null)
  const [statusError, setStatusError] = useState<string | null>(null)
  const [editingProfile, setEditingProfile] = useState(false)
  const [confirmingArchive, setConfirmingArchive] = useState(false)
  const [confirmingRestore, setConfirmingRestore] = useState(false)

  const load = () => {
    if (!episodeId) return
    setError(null)
    treatmentEpisodesApi
      .get(episodeId)
      .then(setEpisode)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el proceso terapéutico.'))
    episodeClinicalProfileApi.get(episodeId).then(setProfile)
  }

  useEffect(load, [episodeId])

  const handleToggleStatus = async () => {
    if (!episode) return
    setStatusError(null)
    try {
      const next = episode.status === 'activo' ? 'pausado' : 'activo'
      const updated = await treatmentEpisodesApi.setStatus(episode.id, next)
      setEpisode(updated)
    } catch (err) {
      setStatusError(typeof err === 'string' ? err : 'No se pudo cambiar el estado del proceso.')
    }
  }

  const handleArchive = async () => {
    if (!episodeId) return
    try {
      await treatmentEpisodesApi.archive(episodeId)
      navigate(`/patients/${patientId}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo archivar el proceso.')
    }
  }

  const handleRestore = async () => {
    if (!episodeId) return
    try {
      const restored = await treatmentEpisodesApi.restore(episodeId)
      setEpisode(restored)
      setConfirmingRestore(false)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo restaurar el proceso.')
    }
  }

  if (error) return <p className="p-10 text-sm text-danger">{error}</p>
  if (!episode) return <p className="p-10 text-sm text-muted-foreground">Cargando…</p>

  const isArchived = episode.deletedAt !== null

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <Link to={`/patients/${patientId}`} className="mb-4 inline-block text-sm text-accent hover:underline">
        Volver a la ficha del paciente
      </Link>

      {isArchived && (
        <div className="mb-6 rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
          Este proceso está archivado. No aparece en el listado de procesos activos hasta que se restaure.
        </div>
      )}

      <div className="mb-6 flex items-start justify-between">
        <div>
          <h1 className="text-xl font-semibold text-foreground">Proceso terapéutico</h1>
          <p className="text-sm text-muted-foreground">Iniciado el {formatSessionDate(episode.startedAt)}</p>
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

      <div className="mb-6 flex items-center justify-between rounded-lg border border-border bg-surface p-6">
        <div>
          <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Estado</h3>
          <p className="text-sm text-foreground">{TREATMENT_EPISODE_STATUS_LABELS[episode.status]}</p>
        </div>
        {!isArchived && episode.status !== 'cerrado' && (
          <Button variant="secondary" onClick={handleToggleStatus}>
            {episode.status === 'activo' ? 'Pausar' : 'Reactivar'}
          </Button>
        )}
      </div>
      {statusError && <p className="mb-6 text-sm text-danger">{statusError}</p>}

      {editingProfile ? (
        <EpisodeClinicalProfileForm
          episodeId={episode.id}
          initial={profile ?? undefined}
          mode={profile ? 'edit' : 'create'}
          onSaved={(saved) => {
            setProfile(saved)
            setEditingProfile(false)
          }}
          onCancel={() => setEditingProfile(false)}
        />
      ) : profile === undefined ? (
        <p className="text-sm text-muted-foreground">Cargando antecedentes del proceso…</p>
      ) : profile === null ? (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">Este proceso todavía no tiene antecedentes específicos registrados.</p>
          <Button onClick={() => setEditingProfile(true)}>Agregar antecedentes del proceso</Button>
        </div>
      ) : (
        <div className="flex flex-col gap-5 rounded-lg border border-border bg-surface p-6">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Antecedentes del proceso</h3>
            <Button variant="secondary" onClick={() => setEditingProfile(true)}>
              Editar
            </Button>
          </div>
          <ProfileField label="Motivo de consulta" value={profile.presentingProblem} />
          <ProfileField label="Código de diagnóstico principal" value={profile.primaryDiagnosisCode} />
          <ProfileField label="Notas diagnósticas" value={profile.diagnosisNotes} />
        </div>
      )}

      {confirmingArchive && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
          <div className="w-full max-w-sm rounded-2xl bg-surface-elevated p-6 shadow-lg">
            <h2 className="mb-2 text-base font-semibold text-foreground">Archivar proceso</h2>
            <p className="mb-4 text-sm text-muted-foreground">
              El proceso se marcará como archivado. No se elimina ninguna información — puede recuperarse más adelante.
            </p>
            <div className="flex justify-end gap-2">
              <Button variant="secondary" onClick={() => setConfirmingArchive(false)}>
                Cancelar
              </Button>
              <Button onClick={handleArchive}>Archivar</Button>
            </div>
          </div>
        </div>
      )}

      {confirmingRestore && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
          <div className="w-full max-w-sm rounded-2xl bg-surface-elevated p-6 shadow-lg">
            <h2 className="mb-2 text-base font-semibold text-foreground">Restaurar proceso</h2>
            <p className="mb-4 text-sm text-muted-foreground">El proceso volverá a estar disponible, con todos sus datos intactos.</p>
            <div className="flex justify-end gap-2">
              <Button variant="secondary" onClick={() => setConfirmingRestore(false)}>
                Cancelar
              </Button>
              <Button onClick={handleRestore}>Restaurar</Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
