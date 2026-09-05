/**
 * Fase 10 — Backup y restauración. Ver `docs/backup-restore.md`.
 */
export interface BackupSummary {
  backupId: string
  createdAt: string
}

export interface RestoreSummary {
  restoredAt: string
}

/**
 * Refleja el formato en disco de `manifest.json` dentro de un `.cclinbackup`
 * (ver `src-tauri/src/backup/manifest.rs`) — a propósito en snake_case, no
 * camelCase como el resto de los tipos de IPC: son los nombres literales
 * que ya existen dentro del archivo de respaldo, no un tipo de transporte
 * propio de esta pantalla.
 */
export interface BackupFileEntry {
  path: string
  size_bytes: number
  sha256: string
}

export interface BackupManifest {
  backup_format_version: number
  backup_id: string
  created_at: string
  app_version: string
  schema_version: number
  vault_meta_format_version: number
  files: BackupFileEntry[]
}

export type RestoreCredentialInput =
  | { kind: 'password'; password: string }
  | { kind: 'recovery_code'; code: string; new_password: string }
