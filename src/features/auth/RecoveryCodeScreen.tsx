import { useState } from 'react'
import { Button } from '../../components/ui/Button'
import { authApi, errorMessage } from './api'
import { AuthShell } from './AuthShell'

export function RecoveryCodeScreen({
  recoveryCode,
  onConfirmed,
  onCancelled,
}: {
  recoveryCode: string
  onConfirmed: () => void
  onCancelled: () => void
}) {
  const [confirmed, setConfirmed] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const handleContinue = async () => {
    setSubmitting(true)
    setError(null)
    try {
      await authApi.confirmVaultCreation()
      onConfirmed()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setSubmitting(false)
    }
  }

  const handleCancel = async () => {
    await authApi.cancelVaultCreation()
    onCancelled()
  }

  return (
    <AuthShell
      title="Guarda tu código de recuperación"
      subtitle="Es la única forma de recuperar el acceso si olvidas tu contraseña. No se puede volver a mostrar después de este paso."
    >
      <div className="flex flex-col gap-4">
        <div className="rounded-lg border border-amber-300 bg-amber-50 p-4 text-center">
          <code className="text-base font-semibold tracking-wider text-slate-900">{recoveryCode}</code>
        </div>
        <p className="text-xs text-slate-500">
          Guárdalo en un gestor de contraseñas o en un lugar físico seguro, fuera de esta aplicación. Si pierdes la
          contraseña y este código a la vez, tus datos serán irrecuperables.
        </p>
        <label className="flex items-start gap-2 text-sm text-slate-700">
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(e) => setConfirmed(e.target.checked)}
            className="mt-0.5"
          />
          Ya guardé mi código de recuperación en un lugar seguro.
        </label>
        {error && <p className="text-sm text-red-600">{error}</p>}
        <Button onClick={handleContinue} disabled={!confirmed || submitting} className="w-full">
          {submitting ? 'Creando el cuaderno…' : 'Continuar'}
        </Button>
        <button
          type="button"
          onClick={handleCancel}
          disabled={submitting}
          className="text-center text-xs text-slate-400 hover:text-slate-600"
        >
          Cancelar y empezar de nuevo
        </button>
      </div>
    </AuthShell>
  )
}
