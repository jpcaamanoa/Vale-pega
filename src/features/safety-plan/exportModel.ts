import { formatSessionDate } from '../sessions/datetime'
import type { SafetyPlan, SafetyPlanContact, SafetyPlanContactType, SafetyPlanListItem } from './types'

/**
 * Modelo de datos puro (sin JSX, sin DOM) del documento exportable/imprimible del Plan de
 * Seguridad. `SafetyPlanExport.tsx` solo renderiza este modelo — toda la lógica de qué
 * información entra al documento vive aquí, donde se puede testear directamente sin React ni
 * `window.print()`.
 *
 * Invariante de privacidad: este modelo NUNCA incluye `plan.id`, `plan.patientId`,
 * `plan.version`, ids de contacto/item, ni ningún otro identificador interno — solo el
 * contenido clínico que la usuaria ya ve en pantalla y, si corresponde, el nombre del paciente.
 */

export interface SafetyPlanExportContactLine {
  name: string
  role: string | null
  isEmergencyContact: boolean
  phone: string | null
  servicePhone: string | null
  isCrisisService: boolean
  address: string | null
  notes: string | null
}

export interface SafetyPlanExportStepModel {
  title: string
  items: string[]
  contacts: SafetyPlanExportContactLine[]
  freeText: string | null
}

export interface SafetyPlanExportModel {
  documentTitle: string
  patientName: string | null
  /** Etiqueta de fecha humana derivada solo de datos del plan (nunca de `plan.version`, ni de
   *  metadata del navegador/impresión). `null` cuando el plan no tiene ninguna fecha registrada. */
  updatedLabel: string | null
  steps: SafetyPlanExportStepModel[]
}

function ymd(iso: string): string {
  return iso.slice(0, 10)
}

function buildUpdatedLabel(plan: Pick<SafetyPlan, 'confirmedAt' | 'reviewedAt'>): string | null {
  const parts: string[] = []
  if (plan.confirmedAt) parts.push(`Confirmado el ${formatSessionDate(ymd(plan.confirmedAt))}`)
  if (plan.reviewedAt) parts.push(`Última revisión: ${formatSessionDate(ymd(plan.reviewedAt))}`)
  return parts.length > 0 ? parts.join(' · ') : null
}

function itemsOfType(items: SafetyPlanListItem[], itemType: SafetyPlanListItem['itemType']): string[] {
  return items
    .filter((i) => i.itemType === itemType)
    .sort((a, b) => a.sortOrder - b.sortOrder)
    .map((i) => i.content)
}

function contactsOfTypes(contacts: SafetyPlanContact[], types: SafetyPlanContactType[]): SafetyPlanExportContactLine[] {
  return contacts
    .filter((c) => types.includes(c.contactType))
    .sort((a, b) => a.sortOrder - b.sortOrder)
    .map((c) => ({
      name: c.name,
      role: c.relationshipOrRole,
      isEmergencyContact: c.isEmergencyContact,
      phone: c.phone,
      servicePhone: c.servicePhone,
      isCrisisService: c.isCrisisService,
      address: c.address,
      notes: c.notes,
    }))
}

export function buildSafetyPlanExportModel({
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
}): SafetyPlanExportModel {
  return {
    documentTitle: 'PLAN DE SEGURIDAD',
    patientName: includeName ? patientName : null,
    updatedLabel: buildUpdatedLabel(plan),
    steps: [
      {
        title: 'Paso 1 · Señales de alerta',
        items: itemsOfType(items, 'warning_sign'),
        contacts: [],
        freeText: plan.warningSigns,
      },
      {
        title: 'Paso 2 · Estrategias individuales',
        items: itemsOfType(items, 'strategy'),
        contacts: [],
        freeText: plan.internalStrategies,
      },
      {
        title: 'Paso 3 · Personas y lugares de distracción',
        items: itemsOfType(items, 'distraction_place'),
        contacts: [...contactsOfTypes(contacts, ['distraction_person']), ...contactsOfTypes(contacts, ['support_person'])],
        freeText: plan.socialSupportStrategies,
      },
      {
        title: 'Paso 4 · Personas a las que pedir ayuda',
        items: [],
        contacts: contactsOfTypes(contacts, ['help_contact']),
        freeText: null,
      },
      {
        title: 'Paso 5 · Profesionales e instituciones de crisis',
        items: [],
        contacts: contactsOfTypes(contacts, ['professional', 'service']),
        freeText: null,
      },
      {
        title: 'Paso 6 · Acciones para aumentar la seguridad del entorno',
        items: [],
        contacts: [],
        freeText: plan.meansSafety,
      },
    ],
  }
}
