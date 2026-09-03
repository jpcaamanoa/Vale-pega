import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { ClinicalProfileTab } from '../clinical-profile/ClinicalProfileTab'
import { GoalsTab } from '../goals/GoalsTab'
import { PaymentsTab } from '../payments/PaymentsTab'
import { SessionsTab } from '../sessions/SessionsTab'
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
// evaluaciones, etc.) — la navegación ya está preparada para recibirlas sin
// rehacer la ficha del paciente. "Sesiones" es real desde la Fase 4;
// "Objetivos" es real desde la Fase 5; "Antecedentes" es real desde la Fase 6;
// "Pagos" es real desde la Fase 7.
const SECTIONS_WITH_REAL_CONTENT: SectionId[] = ['resumen', 'antecedentes', 'sesiones', 'objetivos', 'pagos']

function SummaryRow({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <div className="flex justify-between border-b border-border py-2 text-sm last:border-b-0">
      <span className="text-muted-foreground">{label}</span>
      <span className="text-foreground">{value || '—'}</span>
    </div>
  )
}

function ResumenSection({ patient }: { patient: Patient }) {
  return (
    <div className="grid grid-cols-1 gap-8 sm:grid-cols-2">
      <div>
        <h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-muted-foreground">Datos personales</h3>
        <SummaryRow label="Nombre completo" value={patient.fullName} />
        <SummaryRow label="Nombre preferido" value={patient.preferredName} />
        <SummaryRow label="RUT" value={patient.rut} />
        <SummaryRow label="Fecha de nacimiento" value={patient.birthDate} />
      </div>
      <div>
        <h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-muted-foreground">Contacto</h3>
        <SummaryRow label="Teléfono" value={patient.phone} />
        <SummaryRow label="Correo" value={patient.email} />
        <SummaryRow label="Dirección" value={patient.address} />
        <SummaryRow label="Región" value={patient.region} />
        <SummaryRow label="Comuna" value={patient.commune} />
      </div>
      <div>
        <h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-muted-foreground">Contacto de emergencia</h3>
        <SummaryRow label="Nombre" value={patient.emergencyContactName} />
        <SummaryRow label="Teléfono" value={patient.emergencyContactPhone} />
        <SummaryRow label="Relación" value={patient.emergencyContactRelationship} />
      </div>
      <div>
        <h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-muted-foreground">Administrativo</h3>
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
      navigate('/patients')
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

  if (error) return <p className="p-10 text-sm text-danger">{error}</p>
  if (!patient) return <p className="p-10 text-sm text-muted-foreground">Cargando…</p>

  const isArchived = patient.deletedAt !== null

  return (
    <div className="mx-auto max-w-4xl px-6 py-10">
      {isArchived && (
        <div className="mb-6 rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
          Este paciente está archivado. No aparece en el listado de pacientes activos ni puede
          editarse hasta que se restaure.
        </div>
      )}

      <div className="mb-6 flex items-start justify-between">
        <div>
          <h1 className="text-xl font-semibold text-foreground">{patient.fullName}</h1>
          {patient.preferredName && <p className="text-sm text-muted-foreground">{patient.preferredName}</p>}
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

      <nav className="mb-6 flex flex-wrap gap-1 border-b border-border">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            onClick={() => setSection(s.id)}
            className={`px-3 py-2 text-sm font-medium transition-colors ${
              section === s.id
                ? 'border-b-2 border-accent text-accent'
                : 'text-muted-foreground hover:text-foreground'
            }`}
          >
            {s.label}
          </button>
        ))}
      </nav>

      {SECTIONS_WITH_REAL_CONTENT.includes(section) ? (
        <>
          {section === 'resumen' && <ResumenSection patient={patient} />}
          {section === 'antecedentes' && id && <ClinicalProfileTab patientId={id} patientArchived={isArchived} />}
          {section === 'sesiones' && id && <SessionsTab patientId={id} patientArchived={isArchived} />}
          {section === 'objetivos' && id && <GoalsTab patientId={id} patientArchived={isArchived} />}
          {section === 'pagos' && id && <PaymentsTab patientId={id} patientArchived={isArchived} />}
        </>
      ) : (
        <p className="py-16 text-center text-sm text-muted-foreground">Próximamente.</p>
      )}

      {confirmingArchive && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
          <div className="w-full max-w-sm rounded-2xl bg-surface-elevated p-6 shadow-lg">
            <h2 className="mb-2 text-base font-semibold text-foreground">Archivar paciente</h2>
            <p className="mb-4 text-sm text-muted-foreground">
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
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
          <div className="w-full max-w-sm rounded-2xl bg-surface-elevated p-6 shadow-lg">
            <h2 className="mb-2 text-base font-semibold text-foreground">Restaurar paciente</h2>
            <p className="mb-4 text-sm text-muted-foreground">
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
