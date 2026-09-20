import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { TextField } from '../../components/ui/TextField'
import { AssessmentsTab } from '../assessments/AssessmentsTab'
import { ClinicalProfileTab } from '../clinical-profile/ClinicalProfileTab'
import { DocumentsTab } from '../documents/DocumentsTab'
import { FormulationTab } from '../formulation/FormulationTab'
import { GoalsTab } from '../goals/GoalsTab'
import { PatientLibrarySection } from '../library/PatientLibrarySection'
import { PaymentsTab } from '../payments/PaymentsTab'
import { SafetyPlanTab } from '../safety-plan/SafetyPlanTab'
import { SessionsTab } from '../sessions/SessionsTab'
import { ProcessesTab } from '../treatment-episodes/ProcessesTab'
import { patientsApi } from './api'
import { PATIENT_STATUS_LABELS, type Patient, type PatientHardDeleteScope } from './types'

type SectionId =
  | 'resumen'
  | 'procesos'
  | 'antecedentes'
  | 'sesiones'
  | 'formulacion'
  | 'objetivos'
  | 'evaluaciones'
  | 'documentos'
  | 'biblioteca'
  | 'pagos'
  | 'plan_seguridad'
  | 'linea_temporal'

// "Línea temporal" (FASE 4B, decisión de producto explícita): oculta de la navegación hasta
// tener una implementación real — nunca un placeholder "Próximamente" visible en una RC. `'linea_temporal'`
// se conserva en `SectionId` (tipo) y en el resto de la estructura de la pantalla porque no hay
// ningún motivo para borrar código/estructuras reutilizables, solo para dejar de mostrar la
// pestaña — ver `docs/ARCHITECTURE.md` sobre esta decisión.
const SECTIONS: { id: SectionId; label: string }[] = [
  { id: 'resumen', label: 'Resumen' },
  { id: 'procesos', label: 'Procesos' },
  { id: 'antecedentes', label: 'Antecedentes' },
  { id: 'sesiones', label: 'Sesiones' },
  { id: 'formulacion', label: 'Formulación' },
  { id: 'objetivos', label: 'Objetivos' },
  { id: 'evaluaciones', label: 'Evaluaciones' },
  { id: 'documentos', label: 'Documentos' },
  { id: 'biblioteca', label: 'Biblioteca' },
  { id: 'pagos', label: 'Pagos' },
  { id: 'plan_seguridad', label: 'Plan de seguridad' },
]

// Estas secciones ya están en `SECTIONS` y tienen contenido real. "Sesiones" es real desde la
// Fase 4; "Objetivos" es real desde la Fase 5; "Antecedentes" es real desde la Fase 6; "Pagos" es
// real desde la Fase 7; "Procesos" es real desde la Fase 9; "Plan de seguridad" es real desde la
// Fase 12; "Evaluaciones" es real desde la Fase 13; "Formulación" es real desde la Fase 15;
// "Documentos" es real desde la Fase 16; "Biblioteca" es real desde la fase de continuación
// post-Fase 19.
const SECTIONS_WITH_REAL_CONTENT: SectionId[] = [
  'resumen',
  'procesos',
  'antecedentes',
  'sesiones',
  'formulacion',
  'objetivos',
  'evaluaciones',
  'documentos',
  'biblioteca',
  'pagos',
  'plan_seguridad',
]

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

const HARD_DELETE_CONFIRM_WORD = 'ELIMINAR'

const HARD_DELETE_SCOPE_LABELS: { key: Exclude<keyof PatientHardDeleteScope, 'hasClinicalProfile'>; label: string }[] = [
  { key: 'sessions', label: 'Sesiones' },
  { key: 'caseFormulations', label: 'Formulaciones' },
  { key: 'therapeuticGoals', label: 'Objetivos terapéuticos' },
  { key: 'assessmentAdministrations', label: 'Evaluaciones' },
  { key: 'payments', label: 'Pagos' },
  { key: 'treatmentEpisodes', label: 'Procesos' },
  { key: 'safetyPlans', label: 'Versiones del plan de seguridad' },
  { key: 'documents', label: 'Documentos propios' },
  { key: 'appointments', label: 'Citas de agenda' },
  { key: 'reminders', label: 'Recordatorios' },
  { key: 'libraryAssociations', label: 'Asociaciones con recursos de Biblioteca' },
]

/** Modal de "Eliminar permanentemente" (FASE 2B) — solo alcanzable desde un paciente ya
 * archivado. Muestra el resumen real del alcance (`patientsApi.getHardDeleteScope`) antes de
 * exigir escribir "ELIMINAR". Los recursos globales de Biblioteca nunca se mencionan como algo
 * que se borra — solo la asociación con este paciente, ver `services::patients::hard_delete_patient`. */
function HardDeletePatientModal({ patientId, onDeleted, onCancel }: { patientId: string; onDeleted: () => void; onCancel: () => void }) {
  const [scope, setScope] = useState<PatientHardDeleteScope | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [confirmText, setConfirmText] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const loadScope = () => {
    setError(null)
    patientsApi
      .getHardDeleteScope(patientId)
      .then(setScope)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el resumen del paciente. Intenta nuevamente.'))
  }

  useEffect(loadScope, [patientId])

  const submit = async () => {
    setError(null)
    setSubmitting(true)
    try {
      await patientsApi.hardDelete(patientId)
      onDeleted()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo eliminar el paciente permanentemente.')
      setSubmitting(false)
    }
  }

  const visibleScopeRows = scope ? HARD_DELETE_SCOPE_LABELS.filter(({ key }) => scope[key] > 0) : []

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="my-auto max-h-[85vh] w-full max-w-lg overflow-y-auto rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-3 text-base font-semibold text-foreground">Eliminar permanentemente esta ficha</h2>
        <p className="mb-4 rounded-lg border border-danger bg-danger-soft px-3 py-2 text-sm text-danger">
          Esta acción eliminará permanentemente la ficha del paciente y sus datos asociados. No se puede deshacer.
        </p>
        <p className="mb-4 rounded-lg border border-warning/40 bg-warning-soft px-3 py-2 text-sm text-warning">
          Los registros clínicos pueden estar sujetos a obligaciones de conservación. Verifica los requisitos
          aplicables antes de eliminarlos permanentemente.
        </p>

        {scope === null && !error && <p className="mb-4 text-sm text-muted-foreground">Cargando resumen…</p>}
        {scope === null && error && (
          <div className="mb-4 flex flex-col gap-2">
            <p className="text-sm text-danger">{error}</p>
            <div>
              <Button type="button" variant="secondary" onClick={loadScope}>
                Reintentar
              </Button>
            </div>
          </div>
        )}
        {scope && (
          <div className="mb-4">
            <p className="mb-2 text-sm font-medium text-foreground">Se eliminará también:</p>
            {visibleScopeRows.length === 0 ? (
              <p className="text-sm text-muted-foreground">Este paciente no tiene datos clínicos registrados todavía.</p>
            ) : (
              <ul className="flex flex-col gap-1 rounded-lg border border-border px-3 py-2 text-sm text-muted-foreground">
                {visibleScopeRows.map(({ key, label }) => (
                  <li key={key} className="flex justify-between">
                    <span>{label}</span>
                    <span className="text-foreground">{scope[key]}</span>
                  </li>
                ))}
                {scope.hasClinicalProfile && (
                  <li className="flex justify-between">
                    <span>Antecedentes clínicos</span>
                    <span className="text-foreground">Sí</span>
                  </li>
                )}
              </ul>
            )}
            {scope.libraryAssociations > 0 && (
              <p className="mt-2 text-xs text-muted-foreground">
                Los recursos de Biblioteca asociados nunca se eliminan — solo se quita su asociación con este paciente.
              </p>
            )}
          </div>
        )}

        {scope && error && <p className="mb-4 text-sm text-danger">{error}</p>}

        <TextField
          label={`Escribe ${HARD_DELETE_CONFIRM_WORD} para confirmar`}
          value={confirmText}
          onChange={(e) => setConfirmText(e.target.value)}
          placeholder={HARD_DELETE_CONFIRM_WORD}
        />

        <div className="mt-5 flex justify-end gap-2">
          <Button type="button" variant="secondary" onClick={onCancel} disabled={submitting}>
            Cancelar
          </Button>
          <Button
            type="button"
            variant="secondary"
            className="border-danger text-danger hover:bg-danger-soft"
            onClick={submit}
            disabled={submitting || scope === null || confirmText !== HARD_DELETE_CONFIRM_WORD}
          >
            Eliminar permanentemente
          </Button>
        </div>
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
  const [confirmingHardDelete, setConfirmingHardDelete] = useState(false)

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
            <>
              <Button variant="secondary" onClick={() => setConfirmingRestore(true)}>
                Restaurar
              </Button>
              <Button variant="secondary" className="border-danger text-danger hover:bg-danger-soft" onClick={() => setConfirmingHardDelete(true)}>
                Eliminar permanentemente
              </Button>
            </>
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
          {section === 'procesos' && id && <ProcessesTab patientId={id} patientArchived={isArchived} />}
          {section === 'antecedentes' && id && <ClinicalProfileTab patientId={id} patientArchived={isArchived} />}
          {section === 'sesiones' && id && <SessionsTab patientId={id} patientArchived={isArchived} />}
          {section === 'formulacion' && id && <FormulationTab patientId={id} patientArchived={isArchived} />}
          {section === 'objetivos' && id && <GoalsTab patientId={id} patientArchived={isArchived} />}
          {section === 'evaluaciones' && id && <AssessmentsTab patientId={id} patientArchived={isArchived} />}
          {section === 'documentos' && id && <DocumentsTab patientId={id} patientArchived={isArchived} />}
          {section === 'biblioteca' && id && <PatientLibrarySection patientId={id} patientArchived={isArchived} />}
          {section === 'pagos' && id && <PaymentsTab patientId={id} patientArchived={isArchived} />}
          {section === 'plan_seguridad' && id && <SafetyPlanTab patientId={id} patientArchived={isArchived} />}
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

      {confirmingHardDelete && id && (
        <HardDeletePatientModal patientId={id} onDeleted={() => navigate('/patients')} onCancel={() => setConfirmingHardDelete(false)} />
      )}
    </div>
  )
}
