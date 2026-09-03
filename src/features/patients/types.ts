export type PatientStatus = 'activo' | 'inactivo' | 'alta' | 'archivado'

/** Ficha completa — solo se pide cuando hace falta (detalle/edición). Nunca para el listado. */
export interface Patient {
  id: string
  fullName: string
  preferredName: string | null
  rut: string | null
  birthDate: string | null
  phone: string | null
  email: string | null
  address: string | null
  emergencyContactName: string | null
  emergencyContactPhone: string | null
  emergencyContactRelationship: string | null
  status: PatientStatus
  referredBy: string | null
  intakeDate: string | null
  /** Nombre exacto de una región del catálogo cerrado, o "Extranjero", o
   * `null` ("no informado"). Ver `./geo.ts`. */
  region: string | null
  /** Comuna de residencia, siempre `null` si `region` es "Extranjero" o `null`. */
  commune: string | null
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/** Lo que devuelve el listado — deliberadamente sin RUT ni datos de contacto. */
export interface PatientListItem {
  id: string
  fullName: string
  preferredName: string | null
  status: PatientStatus
  intakeDate: string | null
}

export interface PatientInput {
  fullName: string
  preferredName?: string | null
  rut?: string | null
  birthDate?: string | null
  phone?: string | null
  email?: string | null
  address?: string | null
  emergencyContactName?: string | null
  emergencyContactPhone?: string | null
  emergencyContactRelationship?: string | null
  status?: PatientStatus | null
  referredBy?: string | null
  intakeDate?: string | null
  region?: string | null
  commune?: string | null
}

export const PATIENT_STATUS_LABELS: Record<PatientStatus, string> = {
  activo: 'Activo',
  inactivo: 'Inactivo',
  alta: 'Alta',
  archivado: 'Archivado',
}
