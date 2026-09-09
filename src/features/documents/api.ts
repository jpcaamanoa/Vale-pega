import { invoke } from '@tauri-apps/api/core'
import { open, save } from '@tauri-apps/plugin-dialog'
import type { DocumentConsistencyReport, DocumentMetadataInput, DocumentSummary, NewDocumentInput } from './types'

export const documentsApi = {
  /** Diálogo nativo "Abrir…" ya disponible (mismo mecanismo que Backup, Fase 10) — sin
   * dependencia nueva. Sin filtro de extensión: Bloque 20/30 de la aprobación, nunca una lista
   * blanca cerrada de "documento clínico razonable". `null` si la usuaria cancela. */
  pickSourceFile: async () => {
    const selected = await open({ title: 'Seleccionar documento', multiple: false, directory: false })
    return typeof selected === 'string' ? selected : null
  },

  /** Diálogo nativo "Guardar como…" para "Exportar copia" (Bloque 22) — distinto de "Abrir". */
  pickExportDestination: (suggestedName: string) => save({ title: 'Exportar copia', defaultPath: suggestedName }),

  create: (input: NewDocumentInput) => invoke<DocumentSummary>('create_document', { input }),

  get: (id: string) => invoke<DocumentSummary>('get_document', { id }),

  list: (patientId: string) => invoke<DocumentSummary[]>('list_documents', { patientId }),

  listArchived: (patientId: string) => invoke<DocumentSummary[]>('list_archived_documents', { patientId }),

  updateMetadata: (id: string, input: DocumentMetadataInput) => invoke<DocumentSummary>('update_document_metadata', { id, input }),

  archive: (id: string) => invoke<DocumentSummary>('archive_document', { id }),

  restore: (id: string) => invoke<DocumentSummary>('restore_document', { id }),

  /** Solo para imágenes — descifra en memoria y entrega un `data:` URL, sin tocar disco. */
  getDataUrl: (id: string) => invoke<string>('get_document_data_url', { id }),

  /** Para PDF/DOCX/otros — descifra a un temporal opaco y lo abre con la aplicación por defecto
   * del sistema. El temporal se limpia al bloquear el vault o cerrar la aplicación. */
  openExternally: (id: string) => invoke<void>('open_document_externally', { id }),

  /** "Exportar copia" — acción explícita, distinta de "abrir". La copia queda fuera del vault
   * cifrado (Bloque 22 de la aprobación). */
  exportTo: (id: string, destinationPath: string) => invoke<void>('export_document', { id, destinationPath }),

  /** Diagnóstico manual de consistencia DB/filesystem — nunca actúa por sí solo. */
  checkConsistency: () => invoke<DocumentConsistencyReport>('check_document_consistency'),
}
