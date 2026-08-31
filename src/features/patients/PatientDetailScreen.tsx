import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { patientsApi } from './api'
import { PATIENT_STATUS_LABELS, type Patient } from './types'

type SectionId =
  | 'resumen'
  | 'antecedentes'
  | 'sesiones'
  | 'formulacion'
  | 'objetivos'
  | 'evaluaciones'
  | 'documentos'
  | 'pagos'
  | 'linea_temporal'

const SECTIONS: { id: SectionId; label: string }[] = [
  { id: 'resumen', label: 'Resumen' },
  { id: 'antecedentes', label: 'Antecedentes' },
  { id: 'sesiones', label: 'Sesiones' },
  { id: 'formulacion', label: 'Formulación' },
  { id: 'objetivos', label: 'Objetivos' },
  { id: 'evaluaciones', label: 'Evaluaciones' },
  { id: 'documentos', label: 'Documentos' },
  { id: 'pagos', label: 'Pagos' },
  { id: 'linea_temporal', label: 'Línea temporal' },
]

// Estas secciones se implementan en fases posteriores (formulación clínica,
// sesiones, evaluaciones, etc.) — la navegación ya está preparada para
// recibirlas sin rehacer la ficha del paciente.
const SECTIONS_WITH_REAL_CONTENT: SectionId[] = ['resumen']

function SummaryRow({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <div className="flex justify-between border-b border-slate-100 py-2 text-sm last:border-b-0">
      <span className="text-slate-500">{label}</span>
      <span className="text-slate-900">{value || '—'}</span>
    </div>
  )
}

function ResumenSection({ patient }: { patient: Patient }) {
  return (
    <div className="grid grid-cols-1 gap-8 sm:grid-cols-2">
      <div>
        <h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-slate-500">Datos personales</h3>
        <SummaryRow label="Nombre completo" value={patient.fullName} />
        <SummaryRow label="Nombre preferido" value={patient.preferredName} />
        <SummaryRow label="RUT" value={patient.rut} />
        <SummaryRow label="Fecha de nacimiento" value={patient.birthDate} />
      </div>
      <div>
        <h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-slate-500">Contacto</h3>
        <SummaryRow label="Teléfono" value={patient.phone} />
        <SummaryRow label="Correo" value={patient.email} />
        <SummaryRow label="Dirección" value={patient.address} />
      </div>
      <div>
        <h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-slate-500">Contacto de emergencia</h3>
        <SummaryRow label="Nombre" value={patient.emergencyContactName} />
        <SummaryRow label="Teléfono" value={patient.emergencyContactPhone} />
        <SummaryRow label="Relación" value={patient.emergencyContactRelationship} />
      </div>
      <div>
        <h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-slate-500">Administrativo</h3>
        <SummaryRow label="Estado" value={PATIENT_STATUS_LABELS[patient.status]} />
        <SummaryRow label="Derivado por" value={patient.referredBy} />
        <SummaryRow label="Fecha de ingreso" value={patient.intakeDate} />
      </div>
    </div>
  )
}

export function PatientDetailScreen() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const [patient, setPatient] = useState<Patient | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [section, setSection] = useState<SectionId>('resumen')
  const [confirmingArchive, setConfirmingArchive] = useState(false)
  const [confirmingRestore, setConfirmingRestore] = useState(false)

  const load = () => {
    if (!id) return
    patientsApi
      .get(id)
      .then(setPatient)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el paciente.'))
  }

  useEffect(load, [id])

  const handleArchive = async () => {
    if (!id) return
    try {
      await patientsApi.archive(id)
      navigate('/')
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo archivar al paciente.')
    }
  }

  const handleRestore = async () => {
    if (!id) return
    try {
      await patientsApi.restore(id)
      setConfirmingRestore(false)
      load()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo restaurar al paciente.')
    }
  }

  if (error) return <p className="p-10 text-sm text-red-600">{error}</p>
  if (!patient) return <p className="p-10 text-sm text-slate-400">Cargando…</p>

  const isArchived = patient.deletedAt !== null

  return (
    <div className="mx-auto max-w-4xl px-6 py-10">
      {isArchived && (
        <div className="mb-6 rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800">
          Este paciente está archivado. No aparece en el listado de pacientes activos ni puede
          editarse hasta que se restaure.
        </div>
      )}

      <div className="mb-6 flex items-start justify-between">
        <div>
          <h1 className="text-xl font-semibold text-slate-900">{patient.fullName}</h1>
          {patient.preferredName && <p className="text-sm text-slate-500">{patient.preferredName}</p>}
        </div>
        <div className="flex gap-2">
          {isArchived ? (
            <Button variant="secondary" onClick={() => setConfirmingRestore(true)}>
              Restaurar
            </Button>
          ) : (
            <>
              <Button variant="secondary" onClick={() => navigate(`/patients/${id}/edit`)}>
                Editar
              </Button>
              <Button variant="secondary" onClick={() => setConfirmingArchive(true)}>
                Archivar
              </Button>
            </>
          )}
        </div>
      </div>

      <nav className="mb-6 flex flex-wrap gap-1 border-b border-slate-200">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            onClick={() => setSection(s.id)}
            className={`px-3 py-2 text-sm font-medium transition-colors ${
              section === s.id
                ? 'border-b-2 border-slate-900 text-slate-900'
                : 'text-slate-500 hover:text-slate-800'
            }`}
          >
            {s.label}
          </button>
        ))}
      </nav>

      {SECTIONS_WITH_REAL_CONTENT.includes(section) ? (
        section === 'resumen' && <ResumenSection patient={patient} />
      ) : (
        <p className="py-16 text-center text-sm text-slate-400">Próximamente.</p>
      )}

      {confirmingArchive && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 px-4">
          <div className="w-full max-w-sm rounded-2xl bg-white p-6 shadow-lg">
            <h2 className="mb-2 text-base font-semibold text-slate-900">Archivar paciente</h2>
            <p className="mb-4 text-sm text-slate-600">
              El paciente se marcará como archivado y dejará de aparecer en el listado. No se elimina ninguna
              información — puede recuperarse más adelante.
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
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 px-4">
          <div className="w-full max-w-sm rounded-2xl bg-white p-6 shadow-lg">
            <h2 className="mb-2 text-base font-semibold text-slate-900">Restaurar paciente</h2>
            <p className="mb-4 text-sm text-slate-600">
              El paciente volverá a aparecer en el listado de pacientes activos, con todos sus
              datos intactos.
            </p>
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
