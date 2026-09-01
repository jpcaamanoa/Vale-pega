import { useEffect, useState } from 'react'
import { HashRouter, Route, Routes } from 'react-router-dom'
import { Layout } from './app/Layout'
import { AgendaScreen } from './features/agenda/AgendaScreen'
import { AppointmentCreateScreen } from './features/agenda/AppointmentCreateScreen'
import { AppointmentDetailScreen } from './features/agenda/AppointmentDetailScreen'
import { AppointmentEditScreen } from './features/agenda/AppointmentEditScreen'
import { authApi } from './features/auth/api'
import { CreateVaultScreen } from './features/auth/CreateVaultScreen'
import { RecoverAccessScreen } from './features/auth/RecoverAccessScreen'
import { RecoveryCodeScreen } from './features/auth/RecoveryCodeScreen'
import type { VaultStatus } from './features/auth/types'
import { UnlockScreen } from './features/auth/UnlockScreen'
import { useRecordActivity } from './features/auth/useRecordActivity'
import { DashboardScreen } from './features/dashboard/DashboardScreen'
import { GoalCreateScreen } from './features/goals/GoalCreateScreen'
import { GoalDetailScreen } from './features/goals/GoalDetailScreen'
import { PatientCreateScreen } from './features/patients/PatientCreateScreen'
import { PatientDetailScreen } from './features/patients/PatientDetailScreen'
import { PatientEditScreen } from './features/patients/PatientEditScreen'
import { PatientsListScreen } from './features/patients/PatientsListScreen'
import { SessionCreateScreen } from './features/sessions/SessionCreateScreen'
import { SessionDetailScreen } from './features/sessions/SessionDetailScreen'
import { SettingsScreen } from './features/settings/SettingsScreen'

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
        <main className="flex min-h-screen items-center justify-center bg-background">
          <p className="text-sm text-muted-foreground">Cargando…</p>
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
      return (
        <HashRouter>
          <Routes>
            <Route element={<Layout onLocked={() => setScreen({ kind: 'unlock' })} />}>
              <Route path="/" element={<DashboardScreen />} />
              <Route path="/patients" element={<PatientsListScreen />} />
              <Route path="/patients/new" element={<PatientCreateScreen />} />
              <Route path="/patients/:id" element={<PatientDetailScreen />} />
              <Route path="/patients/:id/edit" element={<PatientEditScreen />} />
              <Route path="/patients/:patientId/sessions/new" element={<SessionCreateScreen />} />
              <Route path="/patients/:patientId/sessions/:sessionId" element={<SessionDetailScreen />} />
              <Route path="/patients/:patientId/goals/new" element={<GoalCreateScreen />} />
              <Route path="/patients/:patientId/goals/:goalId" element={<GoalDetailScreen />} />
              <Route path="/agenda" element={<AgendaScreen />} />
              <Route path="/agenda/new" element={<AppointmentCreateScreen />} />
              <Route path="/agenda/:id" element={<AppointmentDetailScreen />} />
              <Route path="/agenda/:id/edit" element={<AppointmentEditScreen />} />
              <Route path="/settings" element={<SettingsScreen />} />
            </Route>
          </Routes>
        </HashRouter>
      )
  }
}

export default App
