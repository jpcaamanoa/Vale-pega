import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { Button } from '../../components/ui/Button'
import { patientsApi } from '../patients/api'
import { buildSafetyPlanExportModel, type SafetyPlanExportContactLine, type SafetyPlanExportModel, type SafetyPlanExportStepModel } from './exportModel'
import type { SafetyPlan, SafetyPlanContact, SafetyPlanListItem } from './types'

/**
 * Exportar/Imprimir el Plan de Seguridad (FASE 3, autorizada explícitamente). Usa el diálogo
 * nativo de impresión del WebView (`window.print()`) en vez de una librería de generación de PDF:
 * "Guardar como PDF" ya es una opción de destino estándar de ese diálogo tanto en Windows
 * (Microsoft Print to PDF) como en macOS — cero dependencias nuevas, sin superficie de ataque
 * adicional. `#print-root` (ver `index.css`) es un portal invisible en pantalla y es lo único
 * visible cuando se imprime — el resto de la aplicación (`#root`) se oculta durante la impresión.
 *
 * Contenido: **solo** el modelo puro construido por `buildSafetyPlanExportModel`
 * (`exportModel.ts`) — nunca diagnóstico, notas clínicas, antecedentes, sesiones, formulación,
 * objetivos, evaluaciones, UUIDs, número de versión interno, ni ninguna otra metadata interna. El
 * nombre del paciente es opcional (checkbox, por defecto marcado) para poder generar también una
 * versión anónima.
 *
 * El header/footer nativo de impresión de Chromium/WebView2 (fecha/hora, título del documento,
 * `location.href` — que con `HashRouter` incluye la ruta interna con el UUID del paciente — y
 * número de página) se elimina estructuralmente vía `@page { margin: 0 }` en `index.css`: sin
 * margen de página no hay superficie donde el motor de impresión pueda dibujarlo. Ver el
 * comentario en `index.css` para el diagnóstico completo.
 */

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

function PrintFreeText({ value }: { value: string | null }) {
  if (!value || !value.trim()) return null
  return <div style={{ fontSize: '13px', color: '#2b2820', whiteSpace: 'pre-wrap', marginBottom: '10px' }}>{value}</div>
}

function PrintItems({ items }: { items: string[] }) {
  if (items.length === 0) return null
  return (
    <ul style={{ margin: '0 0 10px 0', paddingLeft: '18px' }}>
      {items.map((content, idx) => (
        <li key={idx} style={{ fontSize: '13px', color: '#2b2820', marginBottom: '3px' }}>
          {content}
        </li>
      ))}
    </ul>
  )
}

function PrintContacts({ contacts }: { contacts: SafetyPlanExportContactLine[] }) {
  if (contacts.length === 0) return null
  return (
    <div style={{ marginBottom: '10px' }}>
      {contacts.map((c, idx) => (
        <div key={idx} style={{ fontSize: '13px', color: '#2b2820', marginBottom: '6px', paddingLeft: '2px' }}>
          <div style={{ fontWeight: 600 }}>
            {c.name}
            {c.role ? ` · ${c.role}` : ''}
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

function PrintStep({ step }: { step: SafetyPlanExportStepModel }) {
  return (
    <section style={{ marginBottom: '18px', breakInside: 'avoid' }}>
      <h2 style={{ fontSize: '13px', fontWeight: 700, borderBottom: '1px solid #d8d2c4', paddingBottom: '3px', marginBottom: '8px' }}>{step.title}</h2>
      <PrintItems items={step.items} />
      <PrintContacts contacts={step.contacts} />
      <PrintFreeText value={step.freeText} />
    </section>
  )
}

function SafetyPlanPrintDocument({ model }: { model: SafetyPlanExportModel }) {
  return (
    <div style={{ maxWidth: '680px', margin: '0 auto', padding: '32px', fontFamily: 'Georgia, "Times New Roman", serif', color: '#2b2820' }}>
      <h1 style={{ fontSize: '22px', fontWeight: 700, letterSpacing: '0.02em', marginBottom: '4px' }}>{model.documentTitle}</h1>
      {model.patientName && <p style={{ fontSize: '14px', marginBottom: '2px' }}>{model.patientName}</p>}
      {model.updatedLabel && <p style={{ fontSize: '11px', color: '#6b6558', marginBottom: '20px' }}>{model.updatedLabel}</p>}
      {model.steps.map((step) => (
        <PrintStep key={step.title} step={step} />
      ))}
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

  const model = buildSafetyPlanExportModel({ plan, contacts, items, patientName, includeName })

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

      {printing && createPortal(<SafetyPlanPrintDocument model={model} />, getPrintRootElement())}
    </div>
  )
}
