/**
 * Biblioteca global de recursos (fase de continuación post-Fase 19). Distinta de Documentos por
 * paciente: un recurso se sube una sola vez y se asocia a cualquier número de pacientes sin
 * duplicar nunca el archivo físico — ver `src-tauri/src/services/library.rs`.
 */
export type LibraryResourceType = 'articulo' | 'libro' | 'protocolo' | 'escala' | 'video' | 'enlace'

export const LIBRARY_RESOURCE_TYPES: LibraryResourceType[] = ['articulo', 'libro', 'protocolo', 'escala', 'video', 'enlace']

export const LIBRARY_RESOURCE_TYPE_LABELS: Record<LibraryResourceType, string> = {
  articulo: 'Artículo',
  libro: 'Libro',
  protocolo: 'Protocolo',
  escala: 'Escala',
  video: 'Video',
  enlace: 'Enlace',
}

export interface LibraryResourceSummary {
  id: string
  title: string
  resourceType: LibraryResourceType | null
  author: string | null
  sourceUrl: string | null
  summary: string | null
  fileDocumentId: string | null
  originalFilename: string | null
  mimeType: string | null
  sizeBytes: number | null
  createdAt: string
  updatedAt: string
}

export interface NewLibraryResourceInput {
  title: string
  resourceType?: string | null
  author?: string | null
  sourceUrl?: string | null
  summary?: string | null
  sourcePath?: string | null
  mimeType?: string | null
}

export interface LibraryResourceMetadataInput {
  title: string
  resourceType?: string | null
  author?: string | null
  sourceUrl?: string | null
  summary?: string | null
}

/** Tamaño legible ("2.4 MB") — mismo criterio que `documents::formatFileSize`, nunca bytes crudos en la UI. */
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
