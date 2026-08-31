import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { agendaApi } from './api'
import { AppointmentForm } from './AppointmentForm'
import type { AppointmentInput } from './types'

export function AppointmentCreateScreen() {
  const navigate = useNavigate()
  const [error, setError] = useState<string | null>(null)

  const handleSubmit = async (input: AppointmentInput) => {
    setError(null)
    try {
      const created = await agendaApi.create(input)
      // El resultado de la sincronización con Google viaja en el estado de
      // navegación para que la ficha de la cita lo muestre una sola vez —
      // esta pantalla se desmonta de inmediato, así que mostrarlo aquí no
      // llegaría a verse.
      navigate(`/agenda/${created.id}`, { state: { syncOutcome: created.syncOutcome } })
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo crear la cita.')
    }
  }

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="mb-6 text-xl font-semibold text-foreground">Nueva cita</h1>
      {error && <p className="mb-4 text-sm text-danger">{error}</p>}
      <AppointmentForm onSubmit={handleSubmit} onCancel={() => navigate('/agenda')} submitLabel="Crear cita" />
    </div>
  )
}
