import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { patientsApi } from './api'
import { PatientForm } from './PatientForm'
import type { PatientFormValues } from './schema'

export function PatientCreateScreen() {
  const navigate = useNavigate()
  const [error, setError] = useState<string | null>(null)

  const handleSubmit = async (values: PatientFormValues) => {
    setError(null)
    try {
      const patient = await patientsApi.create(values)
      navigate(`/patients/${patient.id}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo crear el paciente.')
    }
  }

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="mb-6 text-xl font-semibold text-foreground">Nuevo paciente</h1>
      {error && <p className="mb-4 text-sm text-danger">{error}</p>}
      <PatientForm onSubmit={handleSubmit} onCancel={() => navigate('/')} submitLabel="Crear paciente" />
    </div>
  )
}
