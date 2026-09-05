import { useState } from 'react'
import { Button } from '../../components/ui/Button'
import { PasswordField } from '../../components/ui/PasswordField'
import { backupApi } from './api'
import type { RestoreCredentialInput } from './types'

type CredentialMode = 'password' | 'recovery_code'

/**
 * "Ajustes → Respaldo y restauración" (Fase 10). Deliberadamente sin
 * lenguaje técnico (nada de SQLCipher/DEK/KEK/manifest/schema) — ver
 * `docs/backup-restore.md`. Restaurar es un reemplazo completo, nunca una
 * fusión: el texto de confirmación lo deja explícito antes de pedir
 * cualquier credencial.
 */
export function BackupRestoreSection() {
  // --- Crear respaldo ---
  const [creating, setCreating] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)
  const [createdAt, setCreatedAt] = useState<string | null>(null)

  const handleCreateBackup = async () => {
    setCreateError(null)
    setCreatedAt(null)
    const destination = await backupApi.pickDestination()
    if (!destination) return
    setCreating(true)
    try {
      const summary = await backupApi.create(destination)
      setCreatedAt(summary.createdAt)
    } catch (err) {
      setCreateError(typeof err === 'string' ? err : 'No se pudo crear el respaldo.')
    } finally {
      setCreating(false)
    }
  }

  // --- Restaurar respaldo ---
  const [archivePath, setArchivePath] = useState<string | null>(null)
  const [confirming, setConfirming] = useState(false)
  const [credentialMode, setCredentialMode] = useState<CredentialMode>('password')
  const [password, setPassword] = useState('')
  const [recoveryCode, setRecoveryCode] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [restoring, setRestoring] = useState(false)
  const [restoreError, setRestoreError] = useState<string | null>(null)
  const [restoredOk, setRestoredOk] = useState(false)

  const handlePickBackupFile = async () => {
    setRestoreError(null)
    setRestoredOk(false)
    const selected = await backupApi.pickBackupFile()
    if (!selected) return
    setArchivePath(selected)
    setConfirming(true)
  }

  const closeConfirm = () => {
    setConfirming(false)
    setArchivePath(null)
    setPassword('')
    setRecoveryCode('')
    setNewPassword('')
    setCredentialMode('password')
  }

  const handleConfirmRestore = async () => {
    if (!archivePath) return
    setRestoreError(null)
    setRestoring(true)
    try {
      const credential: RestoreCredentialInput =
        credentialMode === 'password' ? { kind: 'password', password } : { kind: 'recovery_code', code: recoveryCode, new_password: newPassword }
      await backupApi.restore(archivePath, credential)
      setRestoredOk(true)
      closeConfirm()
    } catch (err) {
      setRestoreError(typeof err === 'string' ? err : 'No se pudo restaurar el respaldo.')
    } finally {
      setRestoring(false)
    }
  }

  return (
    <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
      <div>
        <h2 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Respaldo y restauración</h2>
      </div>

      <div className="border-t border-border pt-4">
        <h3 className="mb-2 text-sm font-medium text-foreground">Crear respaldo</h3>
        <p className="mb-3 text-xs text-muted-foreground">
          Crea una copia cifrada recuperable de los datos de Cuaderno Clínico. Guarda este archivo en un lugar
          seguro, idealmente distinto del disco donde está Cuaderno Clínico — un disco externo, una carpeta
          personal, o cualquier almacenamiento que revises después. Trátalo como información confidencial: sigue
          cifrado, pero contiene una copia completa de tus datos.
        </p>
        <Button onClick={handleCreateBackup} disabled={creating}>
          {creating ? 'Creando respaldo…' : 'Crear respaldo'}
        </Button>
        {createError && <p className="mt-2 text-sm text-danger">{createError}</p>}
        {createdAt && <p className="mt-2 text-sm text-success">Respaldo creado correctamente.</p>}
      </div>

      <div className="border-t border-border pt-4">
        <h3 className="mb-2 text-sm font-medium text-foreground">Restaurar respaldo</h3>
        <p className="mb-3 text-xs text-muted-foreground">
          Restaurar reemplazará los datos locales actuales por los contenidos en el respaldo elegido. Nunca combina
          ni sincroniza — reemplaza por completo.
        </p>
        <Button variant="secondary" onClick={handlePickBackupFile} disabled={restoring}>
          Restaurar respaldo
        </Button>
        {restoreError && <p className="mt-2 text-sm text-danger">{restoreError}</p>}
        {restoredOk && <p className="mt-2 text-sm text-success">Respaldo restaurado correctamente. Desbloquea para continuar.</p>}
      </div>

      {confirming && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
          <div className="w-full max-w-md rounded-2xl bg-surface-elevated p-6 shadow-lg">
            <h2 className="mb-2 text-base font-semibold text-foreground">Restaurar respaldo</h2>
            <p className="mb-4 text-sm text-muted-foreground">
              Los datos actuales de este dispositivo serán reemplazados por el respaldo seleccionado. Antes de
              reemplazarlos, Cuaderno Clínico verificará el respaldo y conservará temporalmente el estado actual
              para poder revertir si ocurre un error.
            </p>

            <div className="mb-3 flex gap-4 text-sm">
              <label className="flex items-center gap-1.5">
                <input type="radio" checked={credentialMode === 'password'} onChange={() => setCredentialMode('password')} />
                Tengo la contraseña de ese respaldo
              </label>
              <label className="flex items-center gap-1.5">
                <input type="radio" checked={credentialMode === 'recovery_code'} onChange={() => setCredentialMode('recovery_code')} />
                Uso mi código de recuperación
              </label>
            </div>

            {credentialMode === 'password' ? (
              <PasswordField label="Contraseña del respaldo" autoComplete="off" value={password} onChange={(e) => setPassword(e.target.value)} />
            ) : (
              <div className="flex flex-col gap-3">
                <div className="flex flex-col gap-1.5">
                  <label className="text-sm font-medium text-foreground">Código de recuperación</label>
                  <input
                    placeholder="XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"
                    autoComplete="off"
                    className="w-full rounded-lg border border-border bg-surface px-3 py-2.5 font-mono text-sm uppercase tracking-wide text-foreground outline-none focus:border-accent focus:ring-1 focus:ring-accent"
                    value={recoveryCode}
                    onChange={(e) => setRecoveryCode(e.target.value)}
                  />
                </div>
                <PasswordField label="Nueva contraseña" autoComplete="new-password" value={newPassword} onChange={(e) => setNewPassword(e.target.value)} />
              </div>
            )}

            <p className="mt-3 text-xs text-muted-foreground">
              Puede ser necesario volver a conectar Google Calendar después de restaurar un respaldo en otro
              dispositivo.
            </p>

            {restoreError && <p className="mt-2 text-sm text-danger">{restoreError}</p>}

            <div className="mt-4 flex justify-end gap-2">
              <Button variant="secondary" onClick={closeConfirm} disabled={restoring}>
                Cancelar
              </Button>
              <Button
                onClick={handleConfirmRestore}
                disabled={restoring || (credentialMode === 'password' ? !password : !recoveryCode || !newPassword)}
              >
                {restoring ? 'Restaurando…' : 'Restaurar respaldo'}
              </Button>
            </div>
          </div>
        </div>
      )}
    </section>
  )
}
