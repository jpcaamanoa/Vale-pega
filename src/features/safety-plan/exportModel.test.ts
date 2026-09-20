import { describe, expect, it } from 'vitest'
import { buildSafetyPlanExportModel } from './exportModel'
import type { SafetyPlan, SafetyPlanContact, SafetyPlanListItem } from './types'

/**
 * Regresión del leak de privacidad reportado en validación manual de Windows: el PDF/impresión
 * del Plan de Seguridad exponía `localhost`, la ruta interna `#/patients/<uuid>` (generada por el
 * "Headers and footers" nativo de Chromium/WebView2, no por este código) y el texto
 * "Versión {N}" en el cuerpo del documento. Estos tests cubren la parte que SÍ depende de nuestro
 * código: el modelo puro que decide qué contenido entra al documento exportado. Todos los datos
 * son ficticios (ningún nombre, RUT, diagnóstico ni identificador real).
 */

const FICTITIOUS_PATIENT_ID = '11111111-1111-4111-8111-111111111111'
const FICTITIOUS_PLAN_ID = '22222222-2222-4222-8222-222222222222'

function makePlan(overrides: Partial<SafetyPlan> = {}): SafetyPlan {
  return {
    id: FICTITIOUS_PLAN_ID,
    patientId: FICTITIOUS_PATIENT_ID,
    version: 3,
    status: 'vigente',
    warningSigns: 'Aislamiento social, dificultad para dormir.',
    internalStrategies: 'Escuchar música, salir a caminar.',
    socialSupportStrategies: 'Llamar a un familiar de confianza.',
    meansSafety: 'Guardar medicamentos con un familiar.',
    crisisSteps: null,
    notes: null,
    reviewedAt: '2026-09-15',
    confirmedAt: '2026-08-01',
    supersededAt: null,
    createdAt: '2026-08-01T10:00:00Z',
    updatedAt: '2026-09-15T10:00:00Z',
    ...overrides,
  }
}

function makeContacts(): SafetyPlanContact[] {
  return [
    {
      id: 'contact-help-1',
      safetyPlanId: FICTITIOUS_PLAN_ID,
      contactType: 'help_contact',
      name: 'Ana Ejemplo',
      relationshipOrRole: 'Hermana',
      phone: '+56 9 1234 5678',
      notes: null,
      address: null,
      servicePhone: null,
      isEmergencyContact: true,
      isCrisisService: false,
      sortOrder: 0,
    },
    {
      id: 'contact-prof-1',
      safetyPlanId: FICTITIOUS_PLAN_ID,
      contactType: 'professional',
      name: 'Consultorio Ejemplo',
      relationshipOrRole: 'Psicólogo tratante',
      phone: null,
      notes: 'Atiende de lunes a viernes.',
      address: 'Avenida Ficticia 123, Ciudad Ejemplo',
      servicePhone: '+56 2 2345 6789',
      isEmergencyContact: false,
      isCrisisService: false,
      sortOrder: 0,
    },
    {
      id: 'contact-service-1',
      safetyPlanId: FICTITIOUS_PLAN_ID,
      contactType: 'service',
      name: 'Línea de Ejemplo',
      relationshipOrRole: null,
      phone: null,
      notes: null,
      address: null,
      servicePhone: '600 000 0000',
      isEmergencyContact: false,
      isCrisisService: true,
      sortOrder: 1,
    },
    {
      id: 'contact-distraction-1',
      safetyPlanId: FICTITIOUS_PLAN_ID,
      contactType: 'distraction_person',
      name: 'Beto Ejemplo',
      relationshipOrRole: 'Amigo',
      phone: '+56 9 8765 4321',
      notes: null,
      address: null,
      servicePhone: null,
      isEmergencyContact: false,
      isCrisisService: false,
      sortOrder: 0,
    },
  ]
}

function makeItems(): SafetyPlanListItem[] {
  return [
    { id: 'item-warning-1', safetyPlanId: FICTITIOUS_PLAN_ID, itemType: 'warning_sign', content: 'Irritabilidad creciente', sortOrder: 0 },
    { id: 'item-strategy-1', safetyPlanId: FICTITIOUS_PLAN_ID, itemType: 'strategy', content: 'Respiración guiada', sortOrder: 0 },
    { id: 'item-distraction-1', safetyPlanId: FICTITIOUS_PLAN_ID, itemType: 'distraction_place', content: 'Parque Ejemplo', sortOrder: 0 },
  ]
}

function stringifyModel(model: unknown): string {
  return JSON.stringify(model)
}

describe('buildSafetyPlanExportModel — privacidad estructural del documento exportado', () => {
  it('nunca incluye el UUID del paciente ni del plan', () => {
    const model = buildSafetyPlanExportModel({
      plan: makePlan(),
      contacts: makeContacts(),
      items: makeItems(),
      patientName: 'Paciente de Prueba',
      includeName: true,
    })
    const serialized = stringifyModel(model)
    expect(serialized).not.toContain(FICTITIOUS_PATIENT_ID)
    expect(serialized).not.toContain(FICTITIOUS_PLAN_ID)
  })

  it('nunca incluye rutas internas de la aplicación ni "localhost"', () => {
    const model = buildSafetyPlanExportModel({
      plan: makePlan(),
      contacts: makeContacts(),
      items: makeItems(),
      patientName: 'Paciente de Prueba',
      includeName: true,
    })
    const serialized = stringifyModel(model)
    expect(serialized).not.toContain('/patients/')
    expect(serialized.toLowerCase()).not.toContain('localhost')
    expect(serialized).not.toContain('#/')
    expect(serialized.toLowerCase()).not.toMatch(/https?:\/\//)
  })

  it('nunca muestra el número de versión interno del plan ("Versión N")', () => {
    const modelV7 = buildSafetyPlanExportModel({
      plan: makePlan({ version: 7 }),
      contacts: [],
      items: [],
      patientName: 'Paciente de Prueba',
      includeName: true,
    })
    const modelV3 = buildSafetyPlanExportModel({
      plan: makePlan({ version: 3 }),
      contacts: [],
      items: [],
      patientName: 'Paciente de Prueba',
      includeName: true,
    })
    expect(stringifyModel(modelV7)).not.toContain('Versión')
    // El modelo no debe depender de `plan.version` en absoluto: dos planes que solo difieren en
    // la versión interna producen exactamente el mismo documento exportado.
    expect(modelV7).toEqual(modelV3)
    expect(modelV3.updatedLabel).toBe('Confirmado el 01-08-2026 · Última revisión: 15-09-2026')
  })

  it('respeta el checkbox de nombre: incluido cuando includeName=true', () => {
    const model = buildSafetyPlanExportModel({
      plan: makePlan(),
      contacts: [],
      items: [],
      patientName: 'Paciente de Prueba',
      includeName: true,
    })
    expect(model.patientName).toBe('Paciente de Prueba')
  })

  it('respeta el checkbox de nombre: ausente cuando includeName=false, sin filtrar identidad indirectamente', () => {
    const model = buildSafetyPlanExportModel({
      plan: makePlan(),
      contacts: [],
      items: [],
      patientName: 'Paciente de Prueba',
      includeName: false,
    })
    expect(model.patientName).toBeNull()
    const serialized = stringifyModel(model)
    expect(serialized).not.toContain('Paciente de Prueba')
  })

  it('incluye los seis pasos, en orden, con sus títulos esperados', () => {
    const model = buildSafetyPlanExportModel({
      plan: makePlan(),
      contacts: makeContacts(),
      items: makeItems(),
      patientName: null,
      includeName: false,
    })
    expect(model.steps.map((s) => s.title)).toEqual([
      'Paso 1 · Señales de alerta',
      'Paso 2 · Estrategias individuales',
      'Paso 3 · Personas y lugares de distracción',
      'Paso 4 · Personas a las que pedir ayuda',
      'Paso 5 · Profesionales e instituciones de crisis',
      'Paso 6 · Acciones para aumentar la seguridad del entorno',
    ])
  })

  it('incluye teléfonos y direcciones de los contactos del plan cuando están presentes', () => {
    const model = buildSafetyPlanExportModel({
      plan: makePlan(),
      contacts: makeContacts(),
      items: makeItems(),
      patientName: null,
      includeName: false,
    })
    const helpStep = model.steps.find((s) => s.title === 'Paso 4 · Personas a las que pedir ayuda')!
    expect(helpStep.contacts).toEqual([
      expect.objectContaining({ name: 'Ana Ejemplo', phone: '+56 9 1234 5678', isEmergencyContact: true }),
    ])

    const professionalsStep = model.steps.find((s) => s.title === 'Paso 5 · Profesionales e instituciones de crisis')!
    expect(professionalsStep.contacts).toEqual([
      expect.objectContaining({ name: 'Consultorio Ejemplo', address: 'Avenida Ficticia 123, Ciudad Ejemplo', servicePhone: '+56 2 2345 6789' }),
      expect.objectContaining({ name: 'Línea de Ejemplo', servicePhone: '600 000 0000', isCrisisService: true }),
    ])
  })

  it('no incluye información clínica fuera del contenido propio del plan (contenido exacto, nada añadido)', () => {
    const model = buildSafetyPlanExportModel({
      plan: makePlan(),
      contacts: makeContacts(),
      items: makeItems(),
      patientName: 'Paciente de Prueba',
      includeName: true,
    })
    const warningStep = model.steps[0]
    expect(warningStep.items).toEqual(['Irritabilidad creciente'])
    expect(warningStep.freeText).toBe('Aislamiento social, dificultad para dormir.')

    // Ningún paso trae contactos de tipos que no le corresponden (p. ej. contactos de ayuda no
    // deben aparecer en el paso de distracción).
    const distractionStep = model.steps[2]
    expect(distractionStep.contacts.map((c) => c.name)).toEqual(['Beto Ejemplo'])
  })

  it('omite la etiqueta de fecha cuando el plan no tiene confirmedAt ni reviewedAt', () => {
    const model = buildSafetyPlanExportModel({
      plan: makePlan({ confirmedAt: null, reviewedAt: null }),
      contacts: [],
      items: [],
      patientName: null,
      includeName: false,
    })
    expect(model.updatedLabel).toBeNull()
  })
})
