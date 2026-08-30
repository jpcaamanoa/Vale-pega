import { useState } from 'react'
import { Button } from '../../components/ui/Button'
import { authApi } from './api'
import { ChangePasswordModal } from './ChangePasswordModal'

/**
 * Fase 1.4: todavía no hay dashboard ni funcionalidades de pacientes — eso
 * es de fases posteriores. Esta pantalla solo confirma que el desbloqueo
 * funcionó y da acceso a bloquear / cambiar contraseña.
 */
export function UnlockedPlaceholder({ onLocked }: { onLocked: () => void }) {
  const [showChangePassword, setShowChangePassword] = useState(false)
  const [changedMessage, setChangedMessage] = useState(false)

  const handleLock = async () => {
    await authApi.lockVault()
    onLocked()
  }

  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-4 bg-slate-50 px-4">
      <h1 className="text-xl font-semibold text-slate-900">Cuaderno Clínico</h1>
      <p className="max-w-sm text-center text-sm text-slate-500">
        El vault está desbloqueado. Las funcionalidades clínicas (pacientes, agenda, sesiones) se construyen en las
        fases siguientes — por ahora esta pantalla solo confirma la autenticación.
      </p>
      {changedMessage && <p className="text-sm text-emerald-600">Contraseña actualizada correctamente.</p>}
      <div className="flex gap-2">
        <Button variant="secondary" onClick={() => setShowChangePassword(true)}>
          Cambiar contraseña
        </Button>
        <Button variant="secondary" onClick={handleLock}>
          Bloquear
        </Button>
      </div>

      {showChangePassword && (
        <ChangePasswordModal
          onClose={() => setShowChangePassword(false)}
          onChanged={() => {
            setShowChangePassword(false)
            setChangedMessage(true)
          }}
        />
      )}
    </main>
  )
}
