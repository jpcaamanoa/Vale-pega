import { invoke } from '@tauri-apps/api/core'
import { open, save } from '@tauri-apps/plugin-dialog'
import type { BackupManifest, BackupSummary, RestoreCredentialInput, RestoreSummary } from './types'

const FILTERS = [{ name: 'Respaldo de Cuaderno Clínico', extensions: ['cclinbackup'] }]

function defaultBackupFileName(): string {
  const now = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  const stamp = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`
  return `CuadernoClinico-${stamp}.cclinbackup`
}

export const backupApi = {
  /** Abre el diálogo nativo "Guardar como…" para elegir dónde guardar el respaldo. `null` si la usuaria cancela. */
  pickDestination: () => save({ title: 'Guardar respaldo', defaultPath: defaultBackupFileName(), filters: FILTERS }),

  /** Abre el diálogo nativo "Abrir…" para elegir un archivo `.cclinbackup`. `null` si la usuaria cancela. */
  pickBackupFile: async () => {
    const selected = await open({ title: 'Seleccionar respaldo', multiple: false, directory: false, filters: FILTERS })
    return typeof selected === 'string' ? selected : null
  },

  create: (destinationPath: string) => invoke<BackupSummary>('create_backup', { destinationPath }),

  inspect: (archivePath: string) => invoke<BackupManifest>('inspect_backup', { archivePath }),

  restore: (archivePath: string, credential: RestoreCredentialInput) =>
    invoke<RestoreSummary>('restore_backup', { archivePath, credential }),
}
