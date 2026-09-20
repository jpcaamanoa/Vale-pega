import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { Button } from '../../components/ui/Button'
import { patientsApi } from '../patients/api'
import { formatSessionDate } from '../sessions/datetime'
import type { SafetyPlan, SafetyPlanContact, SafetyPlanContactType, SafetyPlanListItem } from './types'

/**
 * Exportar/Imprimir el Plan de Seguridad (FASE 3, autorizada explícitamente). Usa el diálogo
 * nativo de impresión del WebView (`window.print()`) en vez de una librería de generación de PDF:
 * "Guardar como PDF" ya es una opción de destino estándar de ese diálogo tanto en Windows
 * (Microsoft Print to PDF) como en macOS — cero dependencias nuevas, sin superficie de ataque
 * adicional. `#print-root` (ver `index.css`) es un portal invisible en pantalla y es lo único
 * visible cuando se imprime — el resto de la aplicación (`#root`) se oculta durante la impresión.
 *
 * Contenido: **solo** el plan vigente que ya se le muestra a la usuaria en pantalla (los mismos
 * `plan`/`contacts`/`items` que `PlanContent`) — nunca diagnóstico, notas clínicas, antecedentes,
 * sesiones, formulación, objetivos, evaluaciones, UUIDs, ni metadata interna. El nombre del
 * paciente es opcional (checkbox, por defecto marcado) para poder generar también una versión
 * anónima.
 */

function ymd(iso: string): string {
  return iso.slice(0, 10)
}

/** `#print-root` (ver `index.css`) tiene que existir como hijo directo de `<body>` — hermano de
 * `#root`, no dentro de él — para que la regla `@media print { #root { display: none } }` no lo
 * oculte también a él por herencia del DOM. Se crea una sola vez y se reutiliza. */
function getPrintRootElement(): HTMLElement {
  let el = document.getElementById('print-root')
  if (!el) {
    el = document.createElement('div')
    el.id = 'print-root'
    document.body.appendChild(el)
  }
  return el
}

function PrintField({ label, value }: { label: string; value: string | null }) {
  if (!value || !value.trim()) return null
  return (
    <div style={{ marginBottom: '10px' }}>
      <div style={{ fontSize: '10px', fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.04em', color: '#6b6558', marginBottom: '2px' }}>{label}</div>
      <div style={{ fontSize: '13px', color: '#2b2820', whiteSpace: 'pre-wrap' }}>{value}</div>
    </div>
  )
}

function PrintItemList({ items, itemType }: { items: SafetyPlanListItem[]; itemType: SafetyPlanListItem['itemType'] }) {
  const filtered = items.filter((i) => i.itemType === itemType).sort((a, b) => a.sortOrder - b.sortOrder)
  if (filtered.length === 0) return null
  return (
    <ul style={{ margin: '0 0 10px 0', paddingLeft: '18px' }}>
      {filtered.map((i) => (
        <li key={i.id} style={{ fontSize: '13px', color: '#2b2820', marginBottom: '3px' }}>
          {i.content}
        </li>
      ))}
    </ul>
  )
}

function PrintContactList({ contacts, types }: { contacts: SafetyPlanContact[]; types: SafetyPlanContactType[] }) {
  const filtered = contacts.filter((c) => types.includes(c.contactType)).sort((a, b) => a.sortOrder - b.sortOrder)
  if (filtered.length === 0) return null
  return (
    <div style={{ marginBottom: '10px' }}>
      {filtered.map((c) => (
        <div key={c.id} style={{ fontSize: '13px', color: '#2b2820', marginBottom: '6px', paddingLeft: '2px' }}>
          <div style={{ fontWeight: 600 }}>
            {c.name}
            {c.relationshipOrRole ? ` · ${c.relationshipOrRole}` : ''}
            {c.isEmergencyContact ? ' · Contacto de emergencia' : ''}
          </div>
          {c.phone && <div>Teléfono: {c.phone}</div>}
          {c.servicePhone && <div>Teléfono del servicio: {c.servicePhone}{c.isCrisisService ? ' (servicio de crisis)' : ''}</div>}
          {c.address && <div>Dirección: {c.address}</div>}
          {c.notes && <div style={{ color: '#6b6558' }}>{c.notes}</div>}
        </div>
      ))}
    </div>
  )
}

function SafetyPlanPrintDocument({
  plan,
  contacts,
  items,
  patientName,
  includeName,
}: {
  plan: SafetyPlan
  contacts: SafetyPlanContact[]
  items: SafetyPlanListItem[]
  patientName: string | null
  includeName: boolean
}) {
  const legacySupportContacts = contacts.filter((c) => c.contactType === 'support_person')
  return (
    <div style={{ maxWidth: '680px', margin: '0 auto', padding: '32px', fontFamily: 'Georgia, "Times New Roman", serif', color: '#2b2820' }}>
      <h1 style={{ fontSize: '22px', fontWeight: 700, letterSpacing: '0.02em', marginBottom: '4px' }}>PLAN DE SEGURIDAD</h1>
      {includeName && patientName && <p style={{ fontSize: '14px', marginBottom: '2px' }}>{patientName}</p>}
      <p style={{ fontSize: '11px', color: '#6b6558', marginBottom: '20px' }}>
        Versión {plan.version}
        {plan.confirmedAt ? ` · Confirmado el ${formatSessionDate(ymd(plan.confirmedAt))}` : ''}
        {plan.reviewedAt ? ` · Última revisión: ${formatSessionDate(ymd(plan.reviewedAt))}` : ''}
      </p>

      <section style={{ marginBottom: '18px' }}>
        <h2 style={{ fontSize: '13px', fontWeight: 700, borderBottom: '1px solid #d8d2c4', paddingBottom: '3px', marginBottom: '8px' }}>Paso 1 · Señales de alerta</h2>
        <PrintItemList items={items} itemType="warning_sign" />
        <PrintField label="" value={plan.warningSigns} />
      </section>

      <section style={{ marginBottom: '18px' }}>
        <h2 style={{ fontSize: '13px', fontWeight: 700, borderBottom: '1px solid #d8d2c4', paddingBottom: '3px', marginBottom: '8px' }}>Paso 2 · Estrategias individuales</h2>
        <PrintItemList items={items} itemType="strategy" />
        <PrintField label="" value={plan.internalStrategies} />
      </section>

      <section style={{ marginBottom: '18px' }}>
        <h2 style={{ fontSize: '13px', fontWeight: 700, borderBottom: '1px solid #d8d2c4', paddingBottom: '3px', marginBottom: '8px' }}>Paso 3 · Personas y lugares de distracción</h2>
        <PrintItemList items={items} itemType="distraction_place" />
        <PrintContactList contacts={contacts} types={['distraction_person']} />
        <PrintField label="" value={plan.socialSupportStrategies} />
        {legacySupportContacts.length > 0 && <PrintContactList contacts={contacts} types={['support_person']} />}
      </section>

      <section style={{ marginBottom: '18px' }}>
        <h2 style={{ fontSize: '13px', fontWeight: 700, borderBottom: '1px solid #d8d2c4', paddingBottom: '3px', marginBottom: '8px' }}>Paso 4 · Personas a las que pedir ayuda</h2>
        <PrintContactList contacts={contacts} types={['help_contact']} />
      </section>

      <section style={{ marginBottom: '18px' }}>
        <h2 style={{ fontSize: '13px', fontWeight: 700, borderBottom: '1px solid #d8d2c4', paddingBottom: '3px', marginBottom: '8px' }}>Paso 5 · Profesionales e instituciones de crisis</h2>
        <PrintContactList contacts={contacts} types={['professional', 'service']} />
      </section>

      <section style={{ marginBottom: '18px', breakInside: 'avoid' }}>
        <h2 style={{ fontSize: '13px', fontWeight: 700, borderBottom: '1px solid #d8d2c4', paddingBottom: '3px', marginBottom: '8px' }}>Paso 6 · Acciones para aumentar la seguridad del entorno</h2>
        <PrintField label="" value={plan.meansSafety} />
      </section>
    </div>
  )
}

export function SafetyPlanExportModal({
  patientId,
  plan,
  contacts,
  items,
  onClose,
}: {
  patientId: string
  plan: SafetyPlan
  contacts: SafetyPlanContact[]
  items: SafetyPlanListItem[]
  onClose: () => void
}) {
  const [includeName, setIncludeName] = useState(true)
  const [patientName, setPatientName] = useState<string | null>(null)
  const [printing, setPrinting] = useState(false)

  useEffect(() => {
    patientsApi
      .get(patientId)
      .then((p) => setPatientName(p.preferredName || p.fullName))
      .catch(() => setPatientName(null))
  }, [patientId])

  useEffect(() => {
    if (!printing) return
    const afterPrint = () => setPrinting(false)
    window.addEventListener('afterprint', afterPrint)
    const timer = window.setTimeout(() => {
      window.print()
    }, 50)
    return () => {
      window.removeEventListener('afterprint', afterPrint)
      window.clearTimeout(timer)
    }
  }, [printing])

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="my-auto w-full max-w-md rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-3 text-base font-semibold text-foreground">Exportar plan de seguridad</h2>

        <label className="mb-4 flex items-center gap-2 text-sm text-foreground">
          <input type="checkbox" checked={includeName} onChange={(e) => setIncludeName(e.target.checked)} />
          Incluir nombre del paciente
        </label>

        <p className="mb-5 rounded-lg border border-warning/40 bg-warning-soft px-3 py-2 text-xs text-warning">
          La copia exportada ya no está protegida por el cifrado de Cuaderno Clínico. Guárdala y compártela de forma segura.
        </p>

        <div className="flex justify-end gap-2">
          <Button type="button" variant="secondary" onClick={onClose}>
            Cancelar
          </Button>
          <Button type="button" onClick={() => setPrinting(true)}>
            Exportar / Imprimir
          </Button>
        </div>
      </div>

      {printing && createPortal(<SafetyPlanPrintDocument plan={plan} contacts={contacts} items={items} patientName={patientName} includeName={includeName} />, getPrintRootElement())}
    </div>
  )
}
