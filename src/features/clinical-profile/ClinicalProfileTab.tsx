import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { Button } from '../../components/ui/Button'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { clinicalProfileApi } from './api'
import { clinicalProfileFormSchema, type ClinicalProfileFormValues } from './schema'
import type { ClinicalProfile, ClinicalProfileInput } from './types'

function ProfileField({ label, value }: { label: string; value: string | null }) {
  return (
    <div>
      <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{label}</h4>
      <p className="whitespace-pre-wrap text-sm text-foreground">{value || '—'}</p>
    </div>
  )
}

function ClinicalProfileForm({
  patientId,
  initial,
  mode,
  onSaved,
  onCancel,
}: {
  patientId: string
  initial?: ClinicalProfile
  mode: 'create' | 'edit'
  onSaved: (profile: ClinicalProfile) => void
  onCancel: () => void
}) {
  const [error, setError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<ClinicalProfileFormValues>({
    resolver: zodResolver(clinicalProfileFormSchema),
    defaultValues: {
      presentingProblem: initial?.presentingProblem ?? '',
      primaryDiagnosisCode: initial?.primaryDiagnosisCode ?? '',
      diagnosisNotes: initial?.diagnosisNotes ?? '',
      riskFlags: initial?.riskFlags ?? '',
      relevantMedicalNotes: initial?.relevantMedicalNotes ?? '',
    },
  })

  const submit = async (values: ClinicalProfileFormValues) => {
    setError(null)
    try {
      const input: ClinicalProfileInput = {
        presentingProblem: values.presentingProblem || null,
        primaryDiagnosisCode: values.primaryDiagnosisCode || null,
        diagnosisNotes: values.diagnosisNotes || null,
        riskFlags: values.riskFlags || null,
        relevantMedicalNotes: values.relevantMedicalNotes || null,
      }
      const saved = mode === 'create' ? await clinicalProfileApi.create(patientId, input) : await clinicalProfileApi.update(patientId, input)
      onSaved(saved)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudieron guardar los antecedentes.')
    }
  }

  return (
    <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
      <TextField label="Motivo de consulta" {...register('presentingProblem')} error={errors.presentingProblem?.message} />
      <TextField label="Código de diagnóstico principal" {...register('primaryDiagnosisCode')} error={errors.primaryDiagnosisCode?.message} />
      <Textarea label="Notas diagnósticas" {...register('diagnosisNotes')} error={errors.diagnosisNotes?.message} />
      <Textarea
        label="Factores de riesgo"
        placeholder='Opcional. Debe ser JSON válido si se completa, por ejemplo: ["dato uno", "dato dos"]'
        {...register('riskFlags')}
        error={errors.riskFlags?.message}
      />
      <Textarea label="Notas médicas relevantes" {...register('relevantMedicalNotes')} error={errors.relevantMedicalNotes?.message} />
      {error && <p className="text-sm text-danger">{error}</p>}
      <div className="flex justify-end gap-2 pt-2">
        <Button type="button" variant="secondary" onClick={onCancel} disabled={isSubmitting}>
          Cancelar
        </Button>
        <Button type="submit" disabled={isSubmitting}>
          {isSubmitting ? 'Guardando…' : mode === 'create' ? 'Guardar antecedentes' : 'Guardar cambios'}
        </Button>
      </div>
    </form>
  )
}

/**
 * Pestaña "Antecedentes" de la ficha del paciente — reemplaza el
 * placeholder "Próximamente" de la Fase 1.5. A diferencia de Sesiones y
 * Objetivos, aquí no hay listado: es un único registro mutable por
 * paciente, sin versionado (ver `docs/clinical-profile.md`). La creación
 * de antecedentes nuevos se bloquea para un paciente archivado
 * (`patientArchived`), pero editar antecedentes ya existentes no —
 * mismo criterio que editar un objetivo o sus indicadores.
 */
export function ClinicalProfileTab({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const [profile, setProfile] = useState<ClinicalProfile | null | undefined>(undefined)
  const [error, setError] = useState<string | null>(null)
  const [editing, setEditing] = useState(false)
  const [justSaved, setJustSaved] = useState(false)

  const load = () => {
    setError(null)
    clinicalProfileApi
      .get(patientId)
      .then(setProfile)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudieron cargar los antecedentes clínicos.'))
  }

  useEffect(load, [patientId])

  const handleSaved = (saved: ClinicalProfile) => {
    setProfile(saved)
    setEditing(false)
    setJustSaved(true)
  }

  if (error) return <p className="text-sm text-danger">{error}</p>
  if (profile === undefined) return <p className="text-sm text-muted-foreground">Cargando…</p>

  const canCreate = !patientArchived

  if (editing) {
    return (
      <ClinicalProfileForm
        patientId={patientId}
        initial={profile ?? undefined}
        mode={profile ? 'edit' : 'create'}
        onSaved={handleSaved}
        onCancel={() => setEditing(false)}
      />
    )
  }

  if (profile === null) {
    return (
      <div className="flex flex-col items-center gap-3 rounded-lg border border-border py-16 text-center">
        <p className="text-sm text-muted-foreground">No hay antecedentes clínicos registrados.</p>
        {canCreate && <Button onClick={() => setEditing(true)}>Agregar antecedentes</Button>}
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-5 rounded-lg border border-border bg-surface p-6">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Antecedentes clínicos</h3>
        <Button
          variant="secondary"
          onClick={() => {
            setJustSaved(false)
            setEditing(true)
          }}
        >
          Editar
        </Button>
      </div>
      {justSaved && <p className="text-sm text-success">Guardado.</p>}
      <ProfileField label="Motivo de consulta" value={profile.presentingProblem} />
      <ProfileField label="Código de diagnóstico principal" value={profile.primaryDiagnosisCode} />
      <ProfileField label="Notas diagnósticas" value={profile.diagnosisNotes} />
      <ProfileField label="Factores de riesgo" value={profile.riskFlags} />
      <ProfileField label="Notas médicas relevantes" value={profile.relevantMedicalNotes} />
    </div>
  )
}
