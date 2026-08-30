import { useEffect, useState } from 'react'
import { authApi } from './features/auth/api'
import { CreateVaultScreen } from './features/auth/CreateVaultScreen'
import { RecoverAccessScreen } from './features/auth/RecoverAccessScreen'
import { RecoveryCodeScreen } from './features/auth/RecoveryCodeScreen'
import type { VaultStatus } from './features/auth/types'
import { UnlockScreen } from './features/auth/UnlockScreen'
import { UnlockedPlaceholder } from './features/auth/UnlockedPlaceholder'
import { useRecordActivity } from './features/auth/useRecordActivity'

type Screen =
  | { kind: 'loading' }
  | { kind: 'create' }
  | { kind: 'recovery-code'; code: string }
  | { kind: 'unlock' }
  | { kind: 'recover-access' }
  | { kind: 'unlocked' }

function screenForStatus(status: VaultStatus): Screen {
  switch (status) {
    case 'no_vault':
      return { kind: 'create' }
    case 'locked':
      return { kind: 'unlock' }
    case 'pending_creation':
      // No persiste entre reinicios (ver security::vault_manager): si la app
      // arranca en este estado es porque la sesión anterior quedó a mitad de
      // crear un vault sin confirmar. Se reinicia el flujo de creación.
      return { kind: 'create' }
    case 'unlocked':
      return { kind: 'unlocked' }
  }
}

/** Cada cuánto se revisa si el backend bloqueó por inactividad mientras la UI seguía mostrando "desbloqueado". */
const STATUS_POLL_MS = 10_000

function App() {
  const [screen, setScreen] = useState<Screen>({ kind: 'loading' })

  useEffect(() => {
    authApi.vaultStatus().then((status) => setScreen(screenForStatus(status)))
  }, [])

  useEffect(() => {
    if (screen.kind !== 'unlocked') return
    const id = window.setInterval(async () => {
      const status = await authApi.vaultStatus()
      if (status !== 'unlocked') setScreen(screenForStatus(status))
    }, STATUS_POLL_MS)
    return () => window.clearInterval(id)
  }, [screen.kind])

  useRecordActivity(screen.kind === 'unlocked')

  switch (screen.kind) {
    case 'loading':
      return (
        <main className="flex min-h-screen items-center justify-center bg-slate-50">
          <p className="text-sm text-slate-400">Cargando…</p>
        </main>
      )
    case 'create':
      return <CreateVaultScreen onCreated={(code) => setScreen({ kind: 'recovery-code', code })} />
    case 'recovery-code':
      return (
        <RecoveryCodeScreen
          recoveryCode={screen.code}
          onConfirmed={() => setScreen({ kind: 'unlocked' })}
          onCancelled={() => setScreen({ kind: 'create' })}
        />
      )
    case 'unlock':
      return (
        <UnlockScreen
          onUnlocked={() => setScreen({ kind: 'unlocked' })}
          onForgotPassword={() => setScreen({ kind: 'recover-access' })}
        />
      )
    case 'recover-access':
      return (
        <RecoverAccessScreen
          onRecovered={() => setScreen({ kind: 'unlocked' })}
          onBack={() => setScreen({ kind: 'unlock' })}
        />
      )
    case 'unlocked':
      return <UnlockedPlaceholder onLocked={() => setScreen({ kind: 'unlock' })} />
  }
}

export default App
