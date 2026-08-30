import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { patientsApi } from './api'
import { PatientForm } from './PatientForm'
import type { PatientFormValues } from './schema'
import type { Patient } from './types'

export function PatientEditScreen() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const [patient, setPatient] = useState<Patient | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!id) return
    patientsApi
      .get(id)
      .then(setPatient)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el paciente.'))
  }, [id])

  const handleSubmit = async (values: PatientFormValues) => {
    if (!id) return
    setError(null)
    try {
      await patientsApi.update(id, values)
      navigate(`/patients/${id}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo actualizar el paciente.')
    }
  }

  if (error) return <p className="p-10 text-sm text-red-600">{error}</p>
  if (!patient) return <p className="p-10 text-sm text-slate-400">Cargando…</p>

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="mb-6 text-xl font-semibold text-slate-900">Editar paciente</h1>
      <PatientForm
        patient={patient}
        onSubmit={handleSubmit}
        onCancel={() => navigate(`/patients/${id}`)}
        submitLabel="Guardar cambios"
      />
    </div>
  )
}
