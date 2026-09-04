import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { formatSessionDate } from '../sessions/datetime'
import { treatmentEpisodesApi } from '../treatment-episodes/api'
import type { TreatmentEpisode } from '../treatment-episodes/types'
import { goalsApi } from './api'
import { goalCreateFormSchema, type GoalCreateFormValues } from './schema'
import type { GoalInput } from './types'

/**
 * Un objetivo puede guardarse sin indicadores — se agregan después desde
 * `GoalDetailScreen`. No hay autoguardado aquí (a diferencia de las notas
 * de sesión): los objetivos se guardan mediante una acción explícita.
 */
export function GoalCreateScreen() {
  const { patientId } = useParams<{ patientId: string }>()
  const navigate = useNavigate()
  const [error, setError] = useState<string | null>(null)
  const [episodes, setEpisodes] = useState<TreatmentEpisode[]>([])

  const {
    register,
    handleSubmit,
    setValue,
    formState: { errors, isSubmitting },
  } = useForm<GoalCreateFormValues>({
    resolver: zodResolver(goalCreateFormSchema),
    defaultValues: { title: '', description: '', targetDate: '', episodeId: '' },
  })

  useEffect(() => {
    if (!patientId) return
    // Solo procesos que pueden recibir objetivos nuevos — nunca 'cerrado'
    // (ver services::treatment_episodes::check_episode_assignable).
    treatmentEpisodesApi
      .list(patientId)
      .then((results) => {
        const assignable = results.filter((e) => e.status !== 'cerrado')
        setEpisodes(assignable)
        const active = assignable.find((e) => e.status === 'activo')
        if (active) setValue('episodeId', active.id)
      })
      .catch(() => {
        // Sin proceso disponible no es un error de la pantalla.
      })
  }, [patientId, setValue])

  if (!patientId) return null

  const submit = async (values: GoalCreateFormValues) => {
    setError(null)
    try {
      const input: GoalInput = {
        patientId,
        episodeId: values.episodeId || null,
        title: values.title,
        description: values.description || null,
        targetDate: values.targetDate || null,
      }
      const created = await goalsApi.create(input)
      navigate(`/patients/${patientId}/goals/${created.id}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo crear el objetivo.')
    }
  }

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="mb-6 text-xl font-semibold text-foreground">Nuevo objetivo</h1>
      {error && <p className="mb-4 text-sm text-danger">{error}</p>}
      <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-6">
        <TextField label="Título" {...register('title')} error={errors.title?.message} />
        <Textarea label="Descripción" {...register('description')} error={errors.description?.message} />
        <TextField
          label="Fecha objetivo"
          type="date"
          {...register('targetDate')}
          error={errors.targetDate?.message}
        />
        {episodes.length > 0 && (
          <Select label="Proceso terapéutico (opcional)" {...register('episodeId')}>
            <option value="">— Sin proceso —</option>
            {episodes.map((ep) => (
              <option key={ep.id} value={ep.id}>
                Iniciado el {formatSessionDate(ep.startedAt)} {ep.status === 'pausado' ? '(pausado)' : ''}
              </option>
            ))}
          </Select>
        )}
        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" variant="secondary" onClick={() => navigate(`/patients/${patientId}`)}>
            Cancelar
          </Button>
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? 'Creando…' : 'Crear objetivo'}
          </Button>
        </div>
      </form>
    </div>
  )
}
