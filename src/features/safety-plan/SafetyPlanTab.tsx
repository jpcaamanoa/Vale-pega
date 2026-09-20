import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { useForm } from 'react-hook-form'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { formatSessionDate } from '../sessions/datetime'
import { safetyPlanApi } from './api'
import { SafetyPlanExportModal } from './SafetyPlanExport'
import { safetyPlanContactFormSchema, safetyPlanFormSchema, type SafetyPlanContactFormValues, type SafetyPlanFormValues } from './schema'
import {
  CREATABLE_SAFETY_PLAN_CONTACT_TYPES,
  SAFETY_PLAN_CONTACT_TYPE_LABELS,
  SAFETY_PLAN_DISCLAIMER,
  SAFETY_PLAN_STATUS_LABELS,
  type SafetyPlan,
  type SafetyPlanContact,
  type SafetyPlanContactInput,
  type SafetyPlanContactType,
  type SafetyPlanInput,
  type SafetyPlanItemType,
  type SafetyPlanListItem,
  type SafetyPlanSummary,
} from './types'

function formatTimestampDate(iso: string): string {
  return new Date(iso).toLocaleDateString('es-CL', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

function PlanField({ label, value }: { label: string; value: string | null }) {
  if (!value) return null
  return (
    <div>
      <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{label}</h4>
      <p className="whitespace-pre-wrap text-sm text-foreground">{value}</p>
    </div>
  )
}

// ---- Ítems agregables de los Pasos 1/2/3-lugares (edición) ----

/**
 * Lista agregable de textos cortos con persistencia inmediata por ítem
 * (cada uno es una fila propia en `safety_plan_list_items`, nunca un
 * textarea único) — señales de alerta, estrategias individuales y lugares
 * de distracción. Click sobre un ítem lo vuelve editable en línea; Enter o
 * blur confirma, Escape cancela.
 */
function ItemListEditor({ planId, itemType, addPlaceholder, emptyMessage }: { planId: string; itemType: SafetyPlanItemType; addPlaceholder: string; emptyMessage: string }) {
  const [items, setItems] = useState<SafetyPlanListItem[] | null>(null)
  const [draft, setDraft] = useState('')
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editingValue, setEditingValue] = useState('')
  const [error, setError] = useState<string | null>(null)

  const load = () => {
    setError(null)
    safetyPlanApi
      .listItems(planId)
      .then((all) => setItems(all.filter((i) => i.itemType === itemType)))
      .catch((err) => {
        setItems(null)
        setError(typeof err === 'string' ? err : 'No se pudo cargar esta sección. Intenta nuevamente.')
      })
  }
  useEffect(load, [planId, itemType])

  const addOne = async () => {
    const content = draft.trim()
    if (!content) return
    setError(null)
    try {
      await safetyPlanApi.addItem(planId, { itemType, content })
      setDraft('')
      load()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo agregar.')
    }
  }

  const commitEdit = async () => {
    if (!editingId) return
    const content = editingValue.trim()
    if (!content) {
      setEditingId(null)
      return
    }
    setError(null)
    try {
      await safetyPlanApi.updateItem(editingId, content)
      setEditingId(null)
      load()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo editar.')
    }
  }

  const removeOne = async (id: string) => {
    setError(null)
    try {
      await safetyPlanApi.deleteItem(id)
      load()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo eliminar.')
    }
  }

  if (items === null && error) {
    return (
      <div className="flex flex-col gap-2">
        <p className="text-sm text-danger">{error}</p>
        <div>
          <Button type="button" variant="secondary" onClick={load}>
            Reintentar
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-2">
      {items === null && <p className="text-sm text-muted-foreground">Cargando…</p>}
      {items !== null && items.length === 0 && <p className="text-sm text-muted-foreground">{emptyMessage}</p>}
      {items !== null && items.length > 0 && (
        <ul className="flex flex-col gap-2">
          {items.map((item) =>
            editingId === item.id ? (
              <li key={item.id}>
                <input
                  autoFocus
                  value={editingValue}
                  onChange={(e) => setEditingValue(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault()
                      commitEdit()
                    } else if (e.key === 'Escape') {
                      setEditingId(null)
                    }
                  }}
                  onBlur={commitEdit}
                  className="w-full rounded-lg border border-accent bg-surface px-3 py-1.5 text-sm text-foreground outline-none focus:ring-1 focus:ring-accent"
                />
              </li>
            ) : (
              <li key={item.id} className="flex items-center justify-between gap-2 rounded-lg border border-border bg-surface px-3 py-1.5 text-sm">
                <button
                  type="button"
                  onClick={() => {
                    setEditingId(item.id)
                    setEditingValue(item.content)
                  }}
                  className="flex-1 text-left text-foreground"
                >
                  {item.content}
                </button>
                <button type="button" onClick={() => removeOne(item.id)} aria-label={`Eliminar "${item.content}"`} className="shrink-0 text-muted-foreground hover:text-danger">
                  ×
                </button>
              </li>
            ),
          )}
        </ul>
      )}
      <div className="flex gap-2">
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              addOne()
            }
          }}
          placeholder={addPlaceholder}
          className="flex-1 rounded-lg border border-border bg-surface px-3 py-2 text-sm text-foreground outline-none focus:border-accent focus:ring-1 focus:ring-accent"
        />
        <Button type="button" variant="secondary" onClick={addOne}>
          Agregar
        </Button>
      </div>
      {error && <p className="text-sm text-danger">{error}</p>}
    </div>
  )
}

/** Vista de solo lectura de los ítems de un tipo — usada para el plan vigente y para el historial. */
function ItemsReadOnlyList({ items, itemType, emptyMessage }: { items: SafetyPlanListItem[]; itemType: SafetyPlanItemType; emptyMessage: string }) {
  const filtered = items.filter((i) => i.itemType === itemType)
  if (filtered.length === 0) return <p className="text-sm text-muted-foreground">{emptyMessage}</p>
  return (
    <ul className="list-disc space-y-1 pl-5 text-sm text-foreground">
      {filtered.map((i) => (
        <li key={i.id}>{i.content}</li>
      ))}
    </ul>
  )
}

// ---- Contactos (Pasos 3-personas, 4 y 5) ----

function ContactFormModal({
  initial,
  allowedTypes,
  showRichFields,
  onSaved,
  onCancel,
}: {
  initial?: SafetyPlanContact
  allowedTypes: SafetyPlanContactType[]
  showRichFields?: boolean
  onSaved: (input: SafetyPlanContactInput) => Promise<void>
  onCancel: () => void
}) {
  const [error, setError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<SafetyPlanContactFormValues>({
    resolver: zodResolver(safetyPlanContactFormSchema),
    defaultValues: {
      contactType: initial?.contactType ?? allowedTypes[0],
      name: initial?.name ?? '',
      relationshipOrRole: initial?.relationshipOrRole ?? '',
      phone: initial?.phone ?? '',
      notes: initial?.notes ?? '',
      address: initial?.address ?? '',
      servicePhone: initial?.servicePhone ?? '',
      isEmergencyContact: initial?.isEmergencyContact ?? false,
      isCrisisService: initial?.isCrisisService ?? false,
    },
  })

  const contactType = watch('contactType')
  const showRich = Boolean(showRichFields) && (contactType === 'professional' || contactType === 'service')

  const submit = async (values: SafetyPlanContactFormValues) => {
    setError(null)
    try {
      await onSaved({
        contactType: values.contactType,
        name: values.name,
        relationshipOrRole: values.relationshipOrRole || null,
        phone: values.phone || null,
        notes: values.notes || null,
        address: showRich ? values.address || null : null,
        servicePhone: showRich ? values.servicePhone || null : null,
        isEmergencyContact: showRich ? Boolean(values.isEmergencyContact) : false,
        isCrisisService: showRich ? Boolean(values.isCrisisService) : false,
      })
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar el contacto.')
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="my-auto max-h-[85vh] w-full max-w-md overflow-y-auto rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-4 text-base font-semibold text-foreground">{initial ? 'Editar contacto' : 'Agregar contacto'}</h2>
        {/* Nunca un <form> aquí: este modal se renderiza dentro del <form> de
            SafetyPlanEditor, y un <form> anidado es HTML inválido. El envío
            se hace por onClick, no por onSubmit. */}
        <div className="flex flex-col gap-4">
          {allowedTypes.length > 1 && (
            <Select label="Tipo" {...register('contactType')} error={errors.contactType?.message}>
              {allowedTypes.map((value) => (
                <option key={value} value={value}>
                  {SAFETY_PLAN_CONTACT_TYPE_LABELS[value]}
                </option>
              ))}
            </Select>
          )}
          <TextField label="Nombre" {...register('name')} error={errors.name?.message} />
          <TextField label="Relación o rol" {...register('relationshipOrRole')} error={errors.relationshipOrRole?.message} />
          <TextField label="Teléfono" {...register('phone')} error={errors.phone?.message} />
          <TextField label="Notas" {...register('notes')} error={errors.notes?.message} />
          {showRich && (
            <>
              <p className="text-xs text-muted-foreground">
                Estos datos se completan manualmente para cada institución — Cuaderno Clínico nunca sugiere ni completa automáticamente un número de emergencia.
              </p>
              <TextField label="Dirección" {...register('address')} error={errors.address?.message} />
              <TextField label="Teléfono del servicio" {...register('servicePhone')} error={errors.servicePhone?.message} />
              <label className="flex items-center gap-2 text-sm text-foreground">
                <input type="checkbox" {...register('isEmergencyContact')} className="h-4 w-4 rounded border-border" />
                Es un contacto de emergencia
              </label>
              <label className="flex items-center gap-2 text-sm text-foreground">
                <input type="checkbox" {...register('isCrisisService')} className="h-4 w-4 rounded border-border" />
                Es un servicio de atención de crisis
              </label>
            </>
          )}
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

/** Vista de solo lectura de contactos de uno o más tipos — plan vigente e historial. */
function ContactsReadOnlyList({ contacts, types, emptyMessage }: { contacts: SafetyPlanContact[]; types: SafetyPlanContactType[]; emptyMessage: string }) {
  const filtered = contacts.filter((c) => types.includes(c.contactType))
  if (filtered.length === 0) return <p className="text-sm text-muted-foreground">{emptyMessage}</p>
  return (
    <ul className="flex flex-col divide-y divide-border">
      {filtered.map((c) => (
        <li key={c.id} className="flex flex-col gap-0.5 py-2 text-sm">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium text-foreground">{c.name}</span>
            {types.length > 1 && <span className="text-xs text-muted-foreground">{SAFETY_PLAN_CONTACT_TYPE_LABELS[c.contactType]}</span>}
            {c.isEmergencyContact && <span className="rounded-full bg-danger/10 px-2 py-0.5 text-xs font-medium text-danger">Contacto de emergencia</span>}
            {c.isCrisisService && <span className="rounded-full bg-accent-soft px-2 py-0.5 text-xs font-medium text-accent">Servicio de crisis</span>}
          </div>
          {(c.relationshipOrRole || c.phone) && <span className="text-xs text-muted-foreground">{[c.relationshipOrRole, c.phone].filter(Boolean).join(' · ')}</span>}
          {(c.address || c.servicePhone) && <span className="text-xs text-muted-foreground">{[c.address, c.servicePhone].filter(Boolean).join(' · ')}</span>}
          {c.notes && <span className="text-xs text-muted-foreground">{c.notes}</span>}
        </li>
      ))}
    </ul>
  )
}

/** Gestión de contactos de un tipo (o conjunto de tipos) — self-contido, solo disponible mientras el plan sigue editable. */
function ContactsSection({
  planId,
  allowedTypes,
  helperText,
  showRichFields,
}: {
  planId: string
  allowedTypes: SafetyPlanContactType[]
  helperText?: string
  showRichFields?: boolean
}) {
  const [contacts, setContacts] = useState<SafetyPlanContact[] | null>(null)
  const [showAdd, setShowAdd] = useState(false)
  const [editing, setEditing] = useState<SafetyPlanContact | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = () => {
    setError(null)
    safetyPlanApi
      .listContacts(planId)
      .then((all) => {
        setContacts(all)
        setError(null)
      })
      .catch((err) => {
        setContacts(null)
        setError(typeof err === 'string' ? err : 'No se pudo cargar esta sección. Intenta nuevamente.')
      })
  }
  useEffect(load, [planId])

  const filtered = (contacts ?? []).filter((c) => allowedTypes.includes(c.contactType))
  const creatableTypes = allowedTypes.filter((t) => CREATABLE_SAFETY_PLAN_CONTACT_TYPES.includes(t))

  const handleDelete = async (contactId: string) => {
    setError(null)
    try {
      await safetyPlanApi.deleteContact(contactId)
      load()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo eliminar el contacto.')
    }
  }

  if (contacts === null && error) {
    return (
      <div className="flex flex-col gap-2">
        <p className="text-sm text-danger">{error}</p>
        <div>
          <Button type="button" variant="secondary" onClick={load}>
            Reintentar
          </Button>
        </div>
      </div>
    )
  }

  if (contacts === null) return <p className="text-sm text-muted-foreground">Cargando…</p>

  return (
    <div className="flex flex-col gap-3">
      {helperText && <p className="text-xs text-muted-foreground">{helperText}</p>}
      {creatableTypes.length > 0 && (
        <div>
          <Button type="button" variant="secondary" onClick={() => setShowAdd(true)}>
            Agregar
          </Button>
        </div>
      )}
      {filtered.length === 0 ? (
        <p className="text-sm text-muted-foreground">No hay contactos registrados todavía.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border">
          {filtered.map((c) => (
            <li key={c.id} className="flex items-start justify-between gap-3 py-2 text-sm">
              <div>
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium text-foreground">{c.name}</span>
                  {allowedTypes.length > 1 && <span className="text-xs text-muted-foreground">{SAFETY_PLAN_CONTACT_TYPE_LABELS[c.contactType]}</span>}
                  {c.isEmergencyContact && <span className="rounded-full bg-danger/10 px-2 py-0.5 text-xs font-medium text-danger">Contacto de emergencia</span>}
                  {c.isCrisisService && <span className="rounded-full bg-accent-soft px-2 py-0.5 text-xs font-medium text-accent">Servicio de crisis</span>}
                </div>
                {(c.relationshipOrRole || c.phone) && <div className="text-xs text-muted-foreground">{[c.relationshipOrRole, c.phone].filter(Boolean).join(' · ')}</div>}
                {(c.address || c.servicePhone) && <div className="text-xs text-muted-foreground">{[c.address, c.servicePhone].filter(Boolean).join(' · ')}</div>}
                {c.notes && <div className="text-xs text-muted-foreground">{c.notes}</div>}
              </div>
              <div className="flex shrink-0 gap-2">
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
      {error && <p className="text-sm text-danger">{error}</p>}

      {showAdd && (
        <ContactFormModal
          allowedTypes={creatableTypes}
          showRichFields={showRichFields}
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
          allowedTypes={allowedTypes}
          showRichFields={showRichFields}
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

/** Vista de solo lectura de todo el contenido de una versión — vigente o histórica. */
function PlanContent({ plan, contacts, items }: { plan: SafetyPlan; contacts: SafetyPlanContact[]; items: SafetyPlanListItem[] }) {
  const legacySupportContacts = contacts.filter((c) => c.contactType === 'support_person')
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 1 · Señales de alerta</h3>
        <ItemsReadOnlyList items={items} itemType="warning_sign" emptyMessage="Sin señales de alerta registradas." />
        <PlanField label="Registrado antes del rediseño" value={plan.warningSigns} />
      </div>

      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 2 · Estrategias individuales</h3>
        <ItemsReadOnlyList items={items} itemType="strategy" emptyMessage="Sin estrategias registradas." />
        <PlanField label="Registrado antes del rediseño" value={plan.internalStrategies} />
      </div>

      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 3 · Personas y lugares de distracción</h3>
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Lugares</h4>
        <ItemsReadOnlyList items={items} itemType="distraction_place" emptyMessage="Sin lugares registrados." />
        <h4 className="mt-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Personas</h4>
        <ContactsReadOnlyList contacts={contacts} types={['distraction_person']} emptyMessage="Sin personas registradas." />
        <PlanField label="Registrado antes del rediseño" value={plan.socialSupportStrategies} />
        {legacySupportContacts.length > 0 && (
          <>
            <h4 className="mt-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Contactos de apoyo registrados antes del rediseño</h4>
            <ContactsReadOnlyList contacts={contacts} types={['support_person']} emptyMessage="" />
          </>
        )}
      </div>

      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 4 · Personas a las que pedir ayuda</h3>
        <p className="text-xs text-muted-foreground">Personas a quienes contarles directamente que se está en crisis y pedirles ayuda.</p>
        <ContactsReadOnlyList contacts={contacts} types={['help_contact']} emptyMessage="Sin personas registradas." />
      </div>

      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 5 · Profesionales e instituciones de crisis</h3>
        <ContactsReadOnlyList contacts={contacts} types={['professional', 'service']} emptyMessage="Sin profesionales o servicios registrados." />
      </div>

      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 6 · Construyendo un ambiente seguro</h3>
        <PlanField label="Acciones para aumentar la seguridad del entorno" value={plan.meansSafety} />
      </div>

      {(plan.crisisSteps || plan.notes) && (
        <div className="flex flex-col gap-3 rounded-lg border border-dashed border-border p-4">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Contenido heredado sin sección propia en el rediseño</h3>
          <PlanField label="Pasos acordados frente a una crisis (versión anterior)" value={plan.crisisSteps} />
          <PlanField label="Observaciones adicionales" value={plan.notes} />
        </div>
      )}
    </div>
  )
}

/**
 * Campo de texto libre heredado de antes del rediseño de seis pasos —
 * colapsado por defecto salvo que ya tenga contenido. Nunca se elimina ni
 * se oculta por completo: sigue siendo editable a través del mismo
 * `<form>` narrativo (`register`), exactamente como antes de esta fase.
 */
function LegacyTextField({ label, hasContent, children }: { label: string; hasContent: boolean; children: ReactNode }) {
  return (
    <details open={hasContent} className="rounded-lg border border-dashed border-border p-3">
      <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-muted-foreground">{label} (registrado antes del rediseño)</summary>
      <div className="mt-3">{children}</div>
    </details>
  )
}

/**
 * Editor de un borrador (crear o actualizar plan), organizado en los seis
 * pasos del modelo de Stanley & Brown. No edita nunca un plan vigente ni
 * reemplazado directamente — "Actualizar plan" crea un borrador nuevo
 * precargado con el contenido del vigente (ver `SafetyPlanTab`).
 *
 * Los ítems agregables (Pasos 1/2/3-lugares) y los contactos (Pasos
 * 3-personas/4/5) se persisten de inmediato por su cuenta, igual que ya
 * ocurría con los contactos desde la Fase 12 — el `<form>` de este
 * componente solo gobierna el contenido narrativo restante (heredado y
 * Paso 6) y la fecha de revisión.
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
    <form onSubmit={handleSubmit(saveDraft)} className="flex flex-col gap-8 rounded-lg border border-border bg-surface p-6">
      <p className="rounded-lg border border-border bg-surface-elevated px-4 py-3 text-xs text-muted-foreground">{SAFETY_PLAN_DISCLAIMER}</p>

      <section className="flex flex-col gap-3">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 1 · Señales de alerta</h3>
        <p className="text-xs text-muted-foreground">¿Cómo noto que estoy empezando a estar peor? (pensamientos, sensaciones, comportamientos)</p>
        <ItemListEditor planId={plan.id} itemType="warning_sign" addPlaceholder="Escribe una señal y presiona Enter…" emptyMessage="Todavía no hay señales registradas." />
        {plan.warningSigns && (
          <LegacyTextField label="Señales de alerta" hasContent>
            <Textarea label="Señales de alerta (texto libre heredado)" {...register('warningSigns')} error={errors.warningSigns?.message} />
          </LegacyTextField>
        )}
      </section>

      <section className="flex flex-col gap-3">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 2 · Estrategias individuales</h3>
        <p className="text-xs text-muted-foreground">Cosas que puedo intentar por mí misma/o, sin necesidad de contactar a nadie.</p>
        <ItemListEditor planId={plan.id} itemType="strategy" addPlaceholder="Escribe una estrategia y presiona Enter…" emptyMessage="Todavía no hay estrategias registradas." />
        {plan.internalStrategies && (
          <LegacyTextField label="Estrategias individuales" hasContent>
            <Textarea label="Estrategias individuales (texto libre heredado)" {...register('internalStrategies')} error={errors.internalStrategies?.message} />
          </LegacyTextField>
        )}
      </section>

      <section className="flex flex-col gap-3">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 3 · Personas y lugares de distracción</h3>
        <p className="text-xs text-muted-foreground">
          Personas o lugares que ayudan a distraerse y sentirse acompañada/o — sin que esto implique necesariamente contarles que se está en crisis (eso es el Paso 4).
        </p>
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Lugares</h4>
        <ItemListEditor planId={plan.id} itemType="distraction_place" addPlaceholder="Escribe un lugar y presiona Enter…" emptyMessage="Todavía no hay lugares registrados." />
        <h4 className="mt-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Personas</h4>
        <ContactsSection planId={plan.id} allowedTypes={['distraction_person']} />
        {plan.socialSupportStrategies && (
          <LegacyTextField label="Personas o lugares de apoyo/distracción" hasContent>
            <Textarea label="Personas o lugares de apoyo/distracción (texto libre heredado)" {...register('socialSupportStrategies')} error={errors.socialSupportStrategies?.message} />
          </LegacyTextField>
        )}
        <LegacySupportContactsPanel planId={plan.id} />
      </section>

      <section className="flex flex-col gap-3">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 4 · Personas a las que pedir ayuda</h3>
        <p className="text-xs text-muted-foreground">Personas a quienes se les puede contar directamente que se está pasando por una crisis y pedirles ayuda.</p>
        <ContactsSection planId={plan.id} allowedTypes={['help_contact']} />
      </section>

      <section className="flex flex-col gap-3">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 5 · Profesionales e instituciones de crisis</h3>
        <ContactsSection planId={plan.id} allowedTypes={['professional', 'service']} showRichFields helperText="Incluye dirección, teléfono del servicio y si corresponde marcarlo como contacto de emergencia o servicio de crisis." />
      </section>

      <section className="flex flex-col gap-3">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Paso 6 · Construyendo un ambiente seguro</h3>
        <Textarea label="Acciones para aumentar la seguridad del entorno" {...register('meansSafety')} error={errors.meansSafety?.message} />
      </section>

      {(plan.crisisSteps || plan.notes) && (
        <section className="flex flex-col gap-3 rounded-lg border border-dashed border-border p-4">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Contenido heredado sin sección propia en el rediseño</h3>
          {plan.crisisSteps && <Textarea label="Pasos acordados frente a una crisis (versión anterior)" {...register('crisisSteps')} error={errors.crisisSteps?.message} />}
          {plan.notes && <Textarea label="Observaciones adicionales" {...register('notes')} error={errors.notes?.message} />}
        </section>
      )}

      <TextField label="Fecha de revisión" type="date" {...register('reviewedAt')} error={errors.reviewedAt?.message} />

      {error && <p className="text-sm text-danger">{error}</p>}
      {justSaved && <p className="text-sm text-success">Contenido guardado.</p>}

      <div className="flex flex-wrap justify-end gap-2 pt-2">
        <Button type="button" variant="secondary" onClick={() => setDiscarding(true)} disabled={isSubmitting || confirming}>
          Descartar borrador
        </Button>
        <Button type="button" variant="secondary" onClick={onCancel} disabled={isSubmitting || confirming}>
          Salir sin confirmar
        </Button>
        <Button type="submit" variant="secondary" disabled={isSubmitting || confirming}>
          {isSubmitting ? 'Guardando…' : 'Guardar cambios'}
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
              Este borrador nunca llegó a confirmarse como vigente. Se eliminará junto con sus contactos e ítems — esta acción no se puede deshacer.
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

/**
 * Contactos `support_person` (tipo original de Fase 12, ya no creable)
 * asociados a este borrador — nunca se ocultan ni se reclasifican
 * automáticamente en un tipo nuevo. Solo se muestra el panel si existe al
 * menos uno.
 */
function LegacySupportContactsPanel({ planId }: { planId: string }) {
  const [contacts, setContacts] = useState<SafetyPlanContact[] | null>(null)

  useEffect(() => {
    safetyPlanApi
      .listContacts(planId)
      .then(setContacts)
      .catch(() => setContacts([]))
  }, [planId])

  const legacy = (contacts ?? []).filter((c) => c.contactType === 'support_person')
  if (legacy.length === 0) return null

  return (
    <LegacyTextField label="Contactos de apoyo" hasContent>
      <ContactsSection planId={planId} allowedTypes={['support_person']} helperText="Registrados antes del rediseño de seis pasos — siguen siendo editables, pero ya no se ofrece este tipo para contactos nuevos." />
    </LegacyTextField>
  )
}

function SafetyPlanHistoryView({ patientId, onBack }: { patientId: string; onBack: () => void }) {
  const [history, setHistory] = useState<SafetyPlanSummary[] | null>(null)
  const [opened, setOpened] = useState<SafetyPlan | null>(null)
  const [openedContacts, setOpenedContacts] = useState<SafetyPlanContact[]>([])
  const [openedItems, setOpenedItems] = useState<SafetyPlanListItem[]>([])
  const [error, setError] = useState<string | null>(null)

  const loadHistory = () => {
    setError(null)
    safetyPlanApi
      .listHistory(patientId)
      .then((all) => {
        setHistory(all)
        setError(null)
      })
      .catch((err) => {
        setHistory(null)
        setError(typeof err === 'string' ? err : 'No se pudo cargar el historial. Intenta nuevamente.')
      })
  }
  useEffect(loadHistory, [patientId])

  const open = async (summary: SafetyPlanSummary) => {
    setError(null)
    try {
      const [plan, contacts, items] = await Promise.all([safetyPlanApi.get(summary.id), safetyPlanApi.listContacts(summary.id), safetyPlanApi.listItems(summary.id)])
      setOpened(plan)
      setOpenedContacts(contacts)
      setOpenedItems(items)
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
        <PlanContent plan={opened} contacts={openedContacts} items={openedItems} />
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
      {error && (
        <div className="flex flex-col gap-2">
          <p className="text-sm text-danger">{error}</p>
          <div>
            <Button type="button" variant="secondary" onClick={loadHistory}>
              Reintentar
            </Button>
          </div>
        </div>
      )}
      {history === null && !error && <p className="text-sm text-muted-foreground">Cargando…</p>}
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
 * Pestaña "Plan de seguridad" de la ficha del paciente (Fase 12,
 * rediseñada a los seis pasos de Stanley & Brown en la fase de
 * continuación post-Fase 19). Ver `docs/safety-plan.md`. Es una
 * herramienta de documentación clínica — la ausencia de un plan nunca
 * bloquea ningún otro flujo, y esta pestaña nunca infiere ni muestra
 * ningún indicador de riesgo.
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
  const [currentItems, setCurrentItems] = useState<SafetyPlanListItem[]>([])
  const [error, setError] = useState<string | null>(null)
  const [view, setView] = useState<'main' | 'edit' | 'history'>('main')
  const [creating, setCreating] = useState(false)
  const [exporting, setExporting] = useState(false)

  const load = () => {
    setError(null)
    Promise.all([safetyPlanApi.getCurrent(patientId), safetyPlanApi.getDraft(patientId)])
      .then(([currentPlan, draftPlan]) => {
        setCurrent(currentPlan)
        setDraft(draftPlan)
        if (currentPlan) {
          Promise.all([safetyPlanApi.listContacts(currentPlan.id), safetyPlanApi.listItems(currentPlan.id)]).then(([contacts, items]) => {
            setCurrentContacts(contacts)
            setCurrentItems(items)
          })
        } else {
          setCurrentContacts([])
          setCurrentItems([])
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
    setCreating(true)
    setError(null)
    try {
      const newDraft = await safetyPlanApi.createDraftFromCurrent(patientId)
      setDraft(newDraft)
      setView('edit')
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo iniciar la actualización del plan.')
    } finally {
      setCreating(false)
    }
  }

  if (error) {
    return (
      <div className="flex flex-col gap-2">
        <p className="text-sm text-danger">{error}</p>
        <div>
          <Button type="button" variant="secondary" onClick={load}>
            Reintentar
          </Button>
        </div>
      </div>
    )
  }
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
              {canCreate && !draft && (
                <Button variant="secondary" onClick={startUpdate} disabled={creating}>
                  {creating ? 'Preparando…' : 'Actualizar plan'}
                </Button>
              )}
              <Button variant="secondary" onClick={() => setExporting(true)}>
                Exportar plan
              </Button>
              <Button variant="secondary" onClick={() => setView('history')}>
                Ver historial
              </Button>
            </div>
          </div>
          <PlanContent plan={current} contacts={currentContacts} items={currentItems} />
        </div>
      )}

      {exporting && current && (
        <SafetyPlanExportModal patientId={patientId} plan={current} contacts={currentContacts} items={currentItems} onClose={() => setExporting(false)} />
      )}
    </div>
  )
}
