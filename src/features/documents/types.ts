/**
 * Fase 16. Un documento es un archivo clínico cifrado — su contenido nunca cruza IPC salvo al
 * pedirlo explícitamente (abrir/exportar). `DocumentSummary` nunca lleva la ruta física, la DEK
 * envuelta, el nonce, el hash ni la versión de formato — ver
 * `src-tauri/src/repositories/documents.rs`.
 */
export type DocumentCategory = 'informe' | 'consentimiento' | 'evaluacion_adjunta' | 'receta' | 'correspondencia' | 'derivacion' | 'otro'

export const DOCUMENT_CATEGORIES: DocumentCategory[] = ['informe', 'consentimiento', 'evaluacion_adjunta', 'receta', 'correspondencia', 'derivacion', 'otro']

export const DOCUMENT_CATEGORY_LABELS: Record<DocumentCategory, string> = {
  informe: 'Informe',
  consentimiento: 'Consentimiento',
  evaluacion_adjunta: 'Evaluación adjunta',
  receta: 'Receta',
  correspondencia: 'Correspondencia',
  derivacion: 'Derivación',
  otro: 'Otro',
}

export interface DocumentSummary {
  id: string
  patientId: string | null
  episodeId: string | null
  sessionId: string | null
  category: DocumentCategory | null
  originalFilename: string
  mimeType: string
  sizeBytes: number
  description: string | null
  createdAt: string
  updatedAt: string
}

export interface NewDocumentInput {
  patientId: string
  episodeId?: string | null
  sessionId?: string | null
  category?: string | null
  description?: string | null
  sourcePath: string
  mimeType?: string | null
}

export interface DocumentMetadataInput {
  category?: string | null
  description?: string | null
}

export interface DocumentConsistencyReport {
  missingCiphertextCount: number
  orphanCiphertextCount: number
}

/** Tamaño legible ("2.4 MB") — nunca bytes crudos en la UI (Bloque 18 de la aprobación). */
export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB']
  let value = bytes / 1024
  let unitIndex = 0
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024
    unitIndex += 1
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`
}
