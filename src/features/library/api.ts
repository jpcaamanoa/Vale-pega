import { invoke } from '@tauri-apps/api/core'
import { open, save } from '@tauri-apps/plugin-dialog'
import type { PatientListItem } from '../patients/types'
import type { LibraryResourceMetadataInput, LibraryResourceSummary, NewLibraryResourceInput } from './types'

export const libraryApi = {
  /** Diálogo nativo "Abrir…" — mismo mecanismo que Documentos (Fase 16), sin dependencia nueva. */
  pickSourceFile: async () => {
    const selected = await open({ title: 'Seleccionar archivo', multiple: false, directory: false })
    return typeof selected === 'string' ? selected : null
  },

  pickExportDestination: (suggestedName: string) => save({ title: 'Exportar copia', defaultPath: suggestedName }),

  create: (input: NewLibraryResourceInput) => invoke<LibraryResourceSummary>('create_library_resource', { input }),

  get: (id: string) => invoke<LibraryResourceSummary>('get_library_resource', { id }),

  list: () => invoke<LibraryResourceSummary[]>('list_library_resources'),

  listArchived: () => invoke<LibraryResourceSummary[]>('list_archived_library_resources'),

  updateMetadata: (id: string, input: LibraryResourceMetadataInput) => invoke<LibraryResourceSummary>('update_library_resource_metadata', { id, input }),

  /** "Eliminar" un recurso: archivado reversible. Si `force` es `false` y el recurso sigue
   * asociado a pacientes, rechaza con un mensaje que incluye cuántos — la UI lo muestra como
   * advertencia y solo reintenta con `force: true` si la usuaria confirma explícitamente. */
  archive: (id: string, force: boolean) => invoke<LibraryResourceSummary>('archive_library_resource', { id, force }),

  restore: (id: string) => invoke<LibraryResourceSummary>('restore_library_resource', { id }),

  /** Solo para imágenes — descifra en memoria y entrega un `data:` URL, sin tocar disco. */
  getDataUrl: (id: string) => invoke<string>('get_library_resource_data_url', { id }),

  /** Para PDF/DOCX/otros — descifra a un temporal opaco y lo abre con la aplicación por defecto. */
  openExternally: (id: string) => invoke<void>('open_library_resource_externally', { id }),

  exportTo: (id: string, destinationPath: string) => invoke<void>('export_library_resource', { id, destinationPath }),

  linkToPatient: (resourceId: string, patientId: string) => invoke<void>('link_library_resource_to_patient', { resourceId, patientId }),

  unlinkFromPatient: (resourceId: string, patientId: string) => invoke<void>('unlink_library_resource_from_patient', { resourceId, patientId }),

  listPatientsForResource: (resourceId: string) => invoke<PatientListItem[]>('list_patients_for_library_resource', { resourceId }),

  listResourcesForPatient: (patientId: string) => invoke<LibraryResourceSummary[]>('list_library_resources_for_patient', { patientId }),
}
