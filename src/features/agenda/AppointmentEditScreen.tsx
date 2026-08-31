import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { agendaApi } from './api'
import { AppointmentForm } from './AppointmentForm'
import type { Appointment, AppointmentInput } from './types'

export function AppointmentEditScreen() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const [appointment, setAppointment] = useState<Appointment | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!id) return
    agendaApi
      .get(id)
      .then(setAppointment)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar la cita.'))
  }, [id])

  const handleSubmit = async (input: AppointmentInput) => {
    if (!id) return
    setError(null)
    try {
      const updated = await agendaApi.update(id, input)
      navigate(`/agenda/${id}`, { state: { syncOutcome: updated.syncOutcome } })
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo actualizar la cita.')
    }
  }

  if (error) return <p className="p-10 text-sm text-danger">{error}</p>
  if (!appointment) return <p className="p-10 text-sm text-muted-foreground">Cargando…</p>

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="mb-6 text-xl font-semibold text-foreground">Editar cita</h1>
      <AppointmentForm
        appointment={appointment}
        onSubmit={handleSubmit}
        onCancel={() => navigate(`/agenda/${id}`)}
        submitLabel="Guardar cambios"
      />
    </div>
  )
}
