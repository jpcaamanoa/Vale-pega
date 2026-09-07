import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { formatSessionDate } from '../sessions/datetime'
import { safetyPlanApi } from './api'
import { safetyPlanContactFormSchema, safetyPlanFormSchema, type SafetyPlanContactFormValues, type SafetyPlanFormValues } from './schema'
import {
  SAFETY_PLAN_CONTACT_TYPE_LABELS,
  SAFETY_PLAN_DISCLAIMER,
  SAFETY_PLAN_STATUS_LABELS,
  type SafetyPlan,
  type SafetyPlanContact,
  type SafetyPlanContactInput,
  type SafetyPlanInput,
  type SafetyPlanSummary,
} from './types'

function formatTimestampDate(iso: string): string {
  return new Date(iso).toLocaleDateString('es-CL', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

function PlanField({ label, value }: { label: string; value: string | null }) {
  return (
    <div>
      <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{label}</h4>
      <p className="whitespace-pre-wrap text-sm text-foreground">{value || '—'}</p>
    </div>
  )
}

/** Vista de solo lectura del contenido de un plan — usada tanto para el vigente como para una versión histórica abierta. */
function PlanContent({ plan }: { plan: SafetyPlan }) {
  return (
    <div className="flex flex-col gap-5">
      <PlanField label="Señales de alerta personales" value={plan.warningSigns} />
      <PlanField label="Estrategias personales" value={plan.internalStrategies} />
      <PlanField label="Personas o lugares que ayudan a acompañarse/distraerse" value={plan.socialSupportStrategies} />
      <PlanField label="Acciones para aumentar la seguridad del entorno" value={plan.meansSafety} />
      <PlanField label="Pasos acordados frente a una crisis" value={plan.crisisSteps} />
      {plan.notes && <PlanField label="Observaciones adicionales" value={plan.notes} />}
    </div>
  )
}

function ContactsList({ contacts }: { contacts: SafetyPlanContact[] }) {
  if (contacts.length === 0) return <p className="text-sm text-muted-foreground">No hay contactos registrados en esta versión.</p>
  return (
    <ul className="flex flex-col divide-y divide-border">
      {contacts.map((c) => (
        <li key={c.id} className="flex flex-col gap-0.5 py-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="font-medium text-foreground">{c.name}</span>
            <span className="text-xs text-muted-foreground">{SAFETY_PLAN_CONTACT_TYPE_LABELS[c.contactType]}</span>
          </div>
          {(c.relationshipOrRole || c.phone) && (
            <span className="text-xs text-muted-foreground">{[c.relationshipOrRole, c.phone].filter(Boolean).join(' · ')}</span>
          )}
          {c.notes && <span className="text-xs text-muted-foreground">{c.notes}</span>}
        </li>
      ))}
    </ul>
  )
}

function ContactFormModal({ initial, onSaved, onCancel }: { initial?: SafetyPlanContact; onSaved: (input: SafetyPlanContactInput) => Promise<void>; onCancel: () => void }) {
  const [error, setError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<SafetyPlanContactFormValues>({
    resolver: zodResolver(safetyPlanContactFormSchema),
    defaultValues: {
      contactType: initial?.contactType ?? 'support_person',
      name: initial?.name ?? '',
      relationshipOrRole: initial?.relationshipOrRole ?? '',
      phone: initial?.phone ?? '',
      notes: initial?.notes ?? '',
    },
  })

  const submit = async (values: SafetyPlanContactFormValues) => {
    setError(null)
    try {
      await onSaved({
        contactType: values.contactType,
        name: values.name,
        relationshipOrRole: values.relationshipOrRole || null,
        phone: values.phone || null,
        notes: values.notes || null,
      })
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar el contacto.')
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
      <div className="w-full max-w-md rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-4 text-base font-semibold text-foreground">{initial ? 'Editar contacto' : 'Agregar contacto'}</h2>
        {/* Nunca un <form> aquí: este modal se renderiza dentro del <form> de
            SafetyPlanEditor, y un <form> anidado es HTML inválido — el
            navegador cierra el <form> exterior al parsear el interior, lo
            que hace que "Guardar contacto" dispare una navegación de página
            completa en vez de guardar el contacto. Por eso el envío se hace
            por onClick, no por onSubmit. */}
        <div className="flex flex-col gap-4">
          <Select label="Tipo" {...register('contactType')} error={errors.contactType?.message}>
            {Object.entries(SAFETY_PLAN_CONTACT_TYPE_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </Select>
          <TextField label="Nombre" {...register('name')} error={errors.name?.message} />
          <TextField label="Relación o rol" {...register('relationshipOrRole')} error={errors.relationshipOrRole?.message} />
          <TextField label="Teléfono" {...register('phone')} error={errors.phone?.message} />
          <TextField label="Notas" {...register('notes')} error={errors.notes?.message} />
          {error && <p className="text-sm text-danger">{error}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onCancel} disabled={isSubmitting}>
              Cancelar
            </Button>
            <Button type="button" onClick={handleSubmit(submit)} disabled={isSubmitting}>
              {isSubmitting ? 'Guardando…' : 'Guardar contacto'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

/** Gestión de contactos de un borrador — solo disponible mientras el plan sigue editable. */
function ContactsEditor({ planId }: { planId: string }) {
  const [contacts, setContacts] = useState<SafetyPlanContact[] | null>(null)
  const [showAdd, setShowAdd] = useState(false)
  const [editing, setEditing] = useState<SafetyPlanContact | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = () => {
    safetyPlanApi
      .listContacts(planId)
      .then(setContacts)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudieron cargar los contactos.'))
  }

  useEffect(load, [planId])

  const handleDelete = async (contactId: string) => {
    try {
      await safetyPlanApi.deleteContact(contactId)
      load()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo eliminar el contacto.')
    }
  }

  if (error) return <p className="text-sm text-danger">{error}</p>
  if (contacts === null) return <p className="text-sm text-muted-foreground">Cargando contactos…</p>

  return (
    <div className="flex flex-col gap-3 rounded-lg border border-border p-4">
      <div className="flex items-center justify-between">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          Personas de apoyo, profesionales y servicios
        </h4>
        <Button type="button" variant="secondary" onClick={() => setShowAdd(true)}>
          Agregar contacto
        </Button>
      </div>
      {contacts.length === 0 ? (
        <p className="text-sm text-muted-foreground">No hay contactos registrados todavía.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border">
          {contacts.map((c) => (
            <li key={c.id} className="flex items-center justify-between gap-3 py-2 text-sm">
              <div>
                <div className="flex items-center gap-2">
                  <span className="font-medium text-foreground">{c.name}</span>
                  <span className="text-xs text-muted-foreground">{SAFETY_PLAN_CONTACT_TYPE_LABELS[c.contactType]}</span>
                </div>
                {(c.relationshipOrRole || c.phone) && (
                  <span className="text-xs text-muted-foreground">{[c.relationshipOrRole, c.phone].filter(Boolean).join(' · ')}</span>
                )}
              </div>
              <div className="flex gap-2">
                <Button type="button" variant="secondary" onClick={() => setEditing(c)}>
                  Editar
                </Button>
                <Button type="button" variant="secondary" onClick={() => handleDelete(c.id)}>
                  Eliminar
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}

      {showAdd && (
        <ContactFormModal
          onSaved={async (input) => {
            await safetyPlanApi.addContact(planId, input)
            setShowAdd(false)
            load()
          }}
          onCancel={() => setShowAdd(false)}
        />
      )}
      {editing && (
        <ContactFormModal
          initial={editing}
          onSaved={async (input) => {
            await safetyPlanApi.updateContact(editing.id, input)
            setEditing(null)
            load()
          }}
          onCancel={() => setEditing(null)}
        />
      )}
    </div>
  )
}

/**
 * Editor de un borrador (crear o actualizar plan). No edita nunca un plan
 * vigente ni reemplazado directamente — "Actualizar plan" crea un borrador
 * nuevo precargado con el contenido del vigente (ver `SafetyPlanTab`).
 */
function SafetyPlanEditor({ plan, onSaved, onConfirmed, onDiscarded, onCancel }: {
  plan: SafetyPlan
  onSaved: (plan: SafetyPlan) => void
  onConfirmed: (plan: SafetyPlan) => void
  onDiscarded: () => void
  onCancel: () => void
}) {
  const [error, setError] = useState<string | null>(null)
  const [justSaved, setJustSaved] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const [discarding, setDiscarding] = useState(false)
  const {
    register,
    handleSubmit,
    getValues,
    formState: { errors, isSubmitting },
  } = useForm<SafetyPlanFormValues>({
    resolver: zodResolver(safetyPlanFormSchema),
    defaultValues: {
      warningSigns: plan.warningSigns ?? '',
      internalStrategies: plan.internalStrategies ?? '',
      socialSupportStrategies: plan.socialSupportStrategies ?? '',
      meansSafety: plan.meansSafety ?? '',
      crisisSteps: plan.crisisSteps ?? '',
      notes: plan.notes ?? '',
      reviewedAt: plan.reviewedAt ?? '',
    },
  })

  const toInput = (values: SafetyPlanFormValues): SafetyPlanInput => ({
    warningSigns: values.warningSigns || null,
    internalStrategies: values.internalStrategies || null,
    socialSupportStrategies: values.socialSupportStrategies || null,
    meansSafety: values.meansSafety || null,
    crisisSteps: values.crisisSteps || null,
    notes: values.notes || null,
    reviewedAt: values.reviewedAt || null,
  })

  const saveDraft = async (values: SafetyPlanFormValues) => {
    setError(null)
    setJustSaved(false)
    try {
      const saved = await safetyPlanApi.updateDraft(plan.id, toInput(values))
      setJustSaved(true)
      onSaved(saved)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar el borrador.')
    }
  }

  const confirmAsCurrent = async () => {
    setError(null)
    setConfirming(true)
    try {
      await safetyPlanApi.updateDraft(plan.id, toInput(getValues()))
      const confirmed = await safetyPlanApi.confirmDraft(plan.id)
      onConfirmed(confirmed)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo confirmar el plan como vigente.')
    } finally {
      setConfirming(false)
    }
  }

  const discard = async () => {
    setError(null)
    try {
      await safetyPlanApi.discardDraft(plan.id)
      onDiscarded()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo descartar el borrador.')
    }
  }

  return (
    <form onSubmit={handleSubmit(saveDraft)} className="flex flex-col gap-5 rounded-lg border border-border bg-surface p-6">
      <p className="rounded-lg border border-border bg-surface-elevated px-4 py-3 text-xs text-muted-foreground">{SAFETY_PLAN_DISCLAIMER}</p>

      <Textarea label="¿Cómo noto que estoy empezando a estar peor? (señales de alerta)" {...register('warningSigns')} error={errors.warningSigns?.message} />
      <Textarea label="Estrategias personales que puedo intentar por mí misma/o" {...register('internalStrategies')} error={errors.internalStrategies?.message} />
      <Textarea label="Personas o lugares que me ayudan a acompañarme o distraerme" {...register('socialSupportStrategies')} error={errors.socialSupportStrategies?.message} />
      <Textarea label="Acciones para aumentar la seguridad del entorno" {...register('meansSafety')} error={errors.meansSafety?.message} />
      <Textarea label="Pasos acordados frente a una crisis" {...register('crisisSteps')} error={errors.crisisSteps?.message} />
      <Textarea label="Observaciones adicionales (opcional)" {...register('notes')} error={errors.notes?.message} />
      <TextField label="Fecha de revisión" type="date" {...register('reviewedAt')} error={errors.reviewedAt?.message} />

      <ContactsEditor planId={plan.id} />

      {error && <p className="text-sm text-danger">{error}</p>}
      {justSaved && <p className="text-sm text-success">Borrador guardado.</p>}

      <div className="flex flex-wrap justify-end gap-2 pt-2">
        <Button type="button" variant="secondary" onClick={() => setDiscarding(true)} disabled={isSubmitting || confirming}>
          Descartar borrador
        </Button>
        <Button type="button" variant="secondary" onClick={onCancel} disabled={isSubmitting || confirming}>
          Salir sin confirmar
        </Button>
        <Button type="submit" variant="secondary" disabled={isSubmitting || confirming}>
          {isSubmitting ? 'Guardando…' : 'Guardar borrador'}
        </Button>
        <Button type="button" onClick={confirmAsCurrent} disabled={isSubmitting || confirming}>
          {confirming ? 'Confirmando…' : 'Guardar como plan vigente'}
        </Button>
      </div>

      {discarding && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
          <div className="w-full max-w-sm rounded-2xl bg-surface-elevated p-6 shadow-lg">
            <h2 className="mb-2 text-base font-semibold text-foreground">Descartar borrador</h2>
            <p className="mb-4 text-sm text-muted-foreground">
              Este borrador nunca llegó a confirmarse como vigente. Se eliminará junto con sus contactos — esta acción no se puede deshacer.
            </p>
            <div className="flex justify-end gap-2">
              <Button type="button" variant="secondary" onClick={() => setDiscarding(false)}>
                Cancelar
              </Button>
              <Button type="button" onClick={discard}>
                Descartar
              </Button>
            </div>
          </div>
        </div>
      )}
    </form>
  )
}

function SafetyPlanHistoryView({ patientId, onBack }: { patientId: string; onBack: () => void }) {
  const [history, setHistory] = useState<SafetyPlanSummary[] | null>(null)
  const [opened, setOpened] = useState<SafetyPlan | null>(null)
  const [openedContacts, setOpenedContacts] = useState<SafetyPlanContact[]>([])
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    safetyPlanApi
      .listHistory(patientId)
      .then(setHistory)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el historial.'))
  }, [patientId])

  const open = async (summary: SafetyPlanSummary) => {
    setError(null)
    try {
      const [plan, contacts] = await Promise.all([safetyPlanApi.get(summary.id), safetyPlanApi.listContacts(summary.id)])
      setOpened(plan)
      setOpenedContacts(contacts)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo abrir esta versión.')
    }
  }

  if (opened) {
    return (
      <div className="flex flex-col gap-5 rounded-lg border border-border bg-surface p-6">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
              Versión {opened.version} — {SAFETY_PLAN_STATUS_LABELS[opened.status]}
            </h3>
            {opened.reviewedAt && <p className="mt-1 text-sm text-muted-foreground">Revisado el {formatSessionDate(opened.reviewedAt)}</p>}
          </div>
          <Button variant="secondary" onClick={() => setOpened(null)}>
            Volver al historial
          </Button>
        </div>
        <PlanContent plan={opened} />
        <div>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Contactos de esta versión</h4>
          <ContactsList contacts={openedContacts} />
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Historial de versiones</h3>
        <Button variant="secondary" onClick={onBack}>
          Volver
        </Button>
      </div>
      {error && <p className="text-sm text-danger">{error}</p>}
      {history === null && <p className="text-sm text-muted-foreground">Cargando…</p>}
      {history !== null && history.length === 0 && <p className="text-sm text-muted-foreground">Todavía no hay versiones registradas.</p>}
      {history !== null && history.length > 0 && (
        <ul className="flex flex-col divide-y divide-border">
          {history.map((s) => (
            <li key={s.id} className="flex items-center justify-between py-3 text-sm">
              <div>
                <span className="font-medium text-foreground">Versión {s.version}</span>
                <span className="ml-2 text-xs text-muted-foreground">{SAFETY_PLAN_STATUS_LABELS[s.status]}</span>
                <div className="text-xs text-muted-foreground">Creado el {formatTimestampDate(s.createdAt)}</div>
              </div>
              <Button variant="secondary" onClick={() => open(s)}>
                Ver
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

/**
 * Pestaña "Plan de seguridad" de la ficha del paciente (Fase 12). Ver
 * `docs/safety-plan.md`. Es una herramienta de documentación clínica — la
 * ausencia de un plan nunca bloquea ningún otro flujo, y esta pestaña nunca
 * infiere ni muestra ningún indicador de riesgo.
 *
 * Un paciente archivado (`patientArchived`) puede seguir consultando su
 * plan vigente y su historial completo, pero no puede crear un plan nuevo
 * ni actualizar el vigente — misma restricción ya aplicada en el backend
 * (`services::safety_plans`), reforzada aquí solo para la experiencia de
 * uso (la autoridad real está en el servicio, no en esta condición).
 */
export function SafetyPlanTab({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const [current, setCurrent] = useState<SafetyPlan | null | undefined>(undefined)
  const [draft, setDraft] = useState<SafetyPlan | null | undefined>(undefined)
  const [currentContacts, setCurrentContacts] = useState<SafetyPlanContact[]>([])
  const [error, setError] = useState<string | null>(null)
  const [view, setView] = useState<'main' | 'edit' | 'history'>('main')
  const [creating, setCreating] = useState(false)

  const load = () => {
    setError(null)
    Promise.all([safetyPlanApi.getCurrent(patientId), safetyPlanApi.getDraft(patientId)])
      .then(([currentPlan, draftPlan]) => {
        setCurrent(currentPlan)
        setDraft(draftPlan)
        if (currentPlan) {
          safetyPlanApi.listContacts(currentPlan.id).then(setCurrentContacts)
        } else {
          setCurrentContacts([])
        }
      })
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el plan de seguridad.'))
  }

  useEffect(load, [patientId])

  const startNewPlan = async () => {
    setCreating(true)
    setError(null)
    try {
      const newDraft = await safetyPlanApi.createDraft(patientId, {})
      setDraft(newDraft)
      setView('edit')
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo crear el borrador.')
    } finally {
      setCreating(false)
    }
  }

  const startUpdate = async () => {
    if (!current) return
    setCreating(true)
    setError(null)
    try {
      const newDraft = await safetyPlanApi.createDraft(patientId, {
        warningSigns: current.warningSigns,
        internalStrategies: current.internalStrategies,
        socialSupportStrategies: current.socialSupportStrategies,
        meansSafety: current.meansSafety,
        crisisSteps: current.crisisSteps,
        notes: current.notes,
        reviewedAt: current.reviewedAt,
      })
      setDraft(newDraft)
      setView('edit')
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo iniciar la actualización del plan.')
    } finally {
      setCreating(false)
    }
  }

  if (error) return <p className="text-sm text-danger">{error}</p>
  if (current === undefined || draft === undefined) return <p className="text-sm text-muted-foreground">Cargando…</p>

  if (view === 'history') {
    return <SafetyPlanHistoryView patientId={patientId} onBack={() => setView('main')} />
  }

  if (view === 'edit' && draft) {
    return (
      <SafetyPlanEditor
        plan={draft}
        onSaved={(saved) => setDraft(saved)}
        onConfirmed={() => {
          setView('main')
          load()
        }}
        onDiscarded={() => {
          setDraft(null)
          setView('main')
        }}
        onCancel={() => setView('main')}
      />
    )
  }

  const canCreate = !patientArchived

  return (
    <div className="flex flex-col gap-6">
      <p className="rounded-lg border border-border bg-surface px-4 py-3 text-xs text-muted-foreground">{SAFETY_PLAN_DISCLAIMER}</p>

      {draft && (
        <div className="flex items-center justify-between rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
          <span>Hay un borrador sin confirmar.</span>
          <Button variant="secondary" onClick={() => setView('edit')}>
            Continuar editando
          </Button>
        </div>
      )}

      {current === null && !draft && (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">No hay un plan de seguridad registrado.</p>
          {canCreate && (
            <Button onClick={startNewPlan} disabled={creating}>
              {creating ? 'Creando…' : 'Crear plan de seguridad'}
            </Button>
          )}
        </div>
      )}

      {current && (
        <div className="flex flex-col gap-5 rounded-lg border border-border bg-surface p-6">
          <div className="flex items-start justify-between">
            <div>
              <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
                Plan vigente — versión {current.version}
              </h3>
              {current.reviewedAt && <p className="mt-1 text-sm text-muted-foreground">Última revisión: {formatSessionDate(current.reviewedAt)}</p>}
            </div>
            <div className="flex gap-2">
              <Button variant="secondary" onClick={() => setView('history')}>
                Ver historial
              </Button>
              {canCreate && !draft && (
                <Button variant="secondary" onClick={startUpdate} disabled={creating}>
                  {creating ? 'Preparando…' : 'Actualizar plan'}
                </Button>
              )}
            </div>
          </div>
          <PlanContent plan={current} />
          <div>
            <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Contactos</h4>
            <ContactsList contacts={currentContacts} />
          </div>
        </div>
      )}
    </div>
  )
}
