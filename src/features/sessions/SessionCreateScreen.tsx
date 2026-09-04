import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { agendaApi } from '../agenda/api'
import { isoToLocalInput } from '../agenda/datetime'
import { treatmentEpisodesApi } from '../treatment-episodes/api'
import type { TreatmentEpisode } from '../treatment-episodes/types'
import { sessionsApi } from './api'
import { formatSessionDate } from './datetime'
import { sessionCreateFormSchema, type SessionCreateFormValues } from './schema'
import { SESSION_MODALITY_LABELS, type SessionInput } from './types'

/**
 * Flujo A (desde la ficha del paciente) y Flujo B (desde una cita de
 * Agenda, vía `?appointmentId=...`) comparten esta misma pantalla. En el
 * Flujo B, `appointmentId` viaja como parámetro de URL — nunca como
 * `location.state` — y solo identifica la cita; la fecha/hora se precarga
 * consultando la cita ya existente, nunca se transporta contenido clínico.
 */
export function SessionCreateScreen() {
  const { patientId } = useParams<{ patientId: string }>()
  const [searchParams] = useSearchParams()
  const appointmentId = searchParams.get('appointmentId') || undefined
  const navigate = useNavigate()
  const [error, setError] = useState<string | null>(null)
  const [episodes, setEpisodes] = useState<TreatmentEpisode[]>([])

  const {
    register,
    handleSubmit,
    reset,
    setValue,
    formState: { errors, isSubmitting },
  } = useForm<SessionCreateFormValues>({
    resolver: zodResolver(sessionCreateFormSchema),
    defaultValues: { sessionDate: '', startTime: '', durationMinutes: '50', modality: '', episodeId: '' },
  })

  useEffect(() => {
    if (!patientId) return
    // Solo procesos que pueden recibir sesiones nuevas — nunca 'cerrado'
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
        // Sin proceso disponible no es un error de la pantalla — el selector
        // simplemente queda vacío.
      })
  }, [patientId, setValue])

  useEffect(() => {
    if (!appointmentId) return
    agendaApi
      .get(appointmentId)
      .then((appointment) => {
        const local = isoToLocalInput(appointment.startsAt)
        const [datePart, timePart] = local.split('T')
        reset({ sessionDate: datePart, startTime: timePart, durationMinutes: '50', modality: appointment.modality ?? '' })
      })
      .catch(() => {
        // Si la cita no se pudo precargar, la usuaria igual puede completar la fecha a mano.
      })
  }, [appointmentId, reset])

  if (!patientId) return null

  const submit = async (values: SessionCreateFormValues) => {
    setError(null)
    try {
      const input: SessionInput = {
        patientId,
        appointmentId: appointmentId ?? null,
        episodeId: values.episodeId || null,
        sessionDate: values.sessionDate,
        startTime: values.startTime || null,
        durationMinutes: values.durationMinutes ? Number(values.durationMinutes) : null,
        modality: (values.modality || null) as SessionInput['modality'],
      }
      const created = await sessionsApi.create(input)
      navigate(`/patients/${patientId}/sessions/${created.id}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo crear la sesión.')
    }
  }

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="mb-6 text-xl font-semibold text-foreground">Nueva sesión</h1>
      {error && <p className="mb-4 text-sm text-danger">{error}</p>}
      <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-6">
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <TextField label="Fecha" type="date" {...register('sessionDate')} error={errors.sessionDate?.message} />
          <TextField label="Hora de inicio" type="time" {...register('startTime')} error={errors.startTime?.message} />
          <TextField
            label="Duración (minutos)"
            type="number"
            min={1}
            {...register('durationMinutes')}
            error={errors.durationMinutes?.message}
          />
          <Select label="Modalidad" {...register('modality')}>
            <option value="">— Sin especificar —</option>
            {Object.entries(SESSION_MODALITY_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </Select>
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
        </div>
        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" variant="secondary" onClick={() => navigate(`/patients/${patientId}`)}>
            Cancelar
          </Button>
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? 'Creando…' : 'Crear sesión'}
          </Button>
        </div>
      </form>
    </div>
  )
}
