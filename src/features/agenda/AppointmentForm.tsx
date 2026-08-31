import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { patientsApi } from '../patients/api'
import type { PatientListItem } from '../patients/types'
import { agendaApi } from './api'
import { formatLocalDateTime, isoToLocalInput, localInputToIso } from './datetime'
import { appointmentFormSchema, type AppointmentFormValues } from './schema'
import { APPOINTMENT_MODALITY_LABELS, type Appointment, type AppointmentInput, type OverlapWarning } from './types'

const OVERLAP_DEBOUNCE_MS = 400

function appointmentToFormValues(appointment?: Appointment): Partial<AppointmentFormValues> {
  if (!appointment) return {}
  return {
    patientId: appointment.patientId ?? undefined,
    startsAt: isoToLocalInput(appointment.startsAt),
    endsAt: isoToLocalInput(appointment.endsAt),
    modality: appointment.modality ?? undefined,
  }
}

export function AppointmentForm({
  appointment,
  onSubmit,
  onCancel,
  submitLabel,
}: {
  appointment?: Appointment
  onSubmit: (input: AppointmentInput) => Promise<void>
  onCancel: () => void
  submitLabel: string
}) {
  const {
    register,
    handleSubmit,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<AppointmentFormValues>({
    resolver: zodResolver(appointmentFormSchema),
    defaultValues: appointmentToFormValues(appointment),
  })

  const [patients, setPatients] = useState<PatientListItem[]>([])
  const [overlaps, setOverlaps] = useState<OverlapWarning[]>([])

  useEffect(() => {
    patientsApi
      .list()
      .then(setPatients)
      .catch(() => setPatients([]))
  }, [])

  const startsAt = watch('startsAt')
  const endsAt = watch('endsAt')

  useEffect(() => {
    if (!startsAt || !endsAt || endsAt <= startsAt) {
      setOverlaps([])
      return
    }
    let cancelled = false
    const timeout = window.setTimeout(() => {
      agendaApi
        .checkOverlap(localInputToIso(startsAt), localInputToIso(endsAt), appointment?.id)
        .then((results) => {
          if (!cancelled) setOverlaps(results)
        })
        .catch(() => {
          if (!cancelled) setOverlaps([])
        })
    }, OVERLAP_DEBOUNCE_MS)
    return () => {
      cancelled = true
      window.clearTimeout(timeout)
    }
  }, [startsAt, endsAt, appointment?.id])

  const submit = async (values: AppointmentFormValues) => {
    await onSubmit({
      patientId: values.patientId || null,
      startsAt: localInputToIso(values.startsAt),
      endsAt: localInputToIso(values.endsAt),
      modality: (values.modality || null) as AppointmentInput['modality'],
    })
  }

  return (
    <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-6">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <Select label="Paciente" {...register('patientId')}>
          <option value="">— Sin paciente (bloqueo personal) —</option>
          {patients.map((p) => (
            <option key={p.id} value={p.id}>
              {p.fullName}
            </option>
          ))}
        </Select>
        <Select label="Modalidad" {...register('modality')}>
          <option value="">— Sin especificar —</option>
          {Object.entries(APPOINTMENT_MODALITY_LABELS).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </Select>
        <TextField label="Inicio" type="datetime-local" {...register('startsAt')} error={errors.startsAt?.message} />
        <TextField label="Término" type="datetime-local" {...register('endsAt')} error={errors.endsAt?.message} />
      </div>

      {overlaps.length > 0 && (
        <div className="rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
          <p className="mb-1 font-medium">
            Este horario se solapa con {overlaps.length === 1 ? 'otra cita' : `${overlaps.length} otras citas`}:
          </p>
          <ul className="list-disc pl-5">
            {overlaps.map((o, i) => (
              <li key={i}>
                {formatLocalDateTime(o.startsAt)} – {formatLocalDateTime(o.endsAt)}{' '}
                {o.hasPatient ? '(con paciente)' : '(bloqueo personal)'}
              </li>
            ))}
          </ul>
          <p className="mt-1 text-xs">Puedes guardar igual — esto es solo una advertencia.</p>
        </div>
      )}

      <div className="flex justify-end gap-2 pt-2">
        <Button type="button" variant="secondary" onClick={onCancel}>
          Cancelar
        </Button>
        <Button type="submit" disabled={isSubmitting}>
          {isSubmitting ? 'Guardando…' : submitLabel}
        </Button>
      </div>
    </form>
  )
}
