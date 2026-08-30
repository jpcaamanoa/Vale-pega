import { useEffect, useState } from 'react'
import { authApi } from './api'
import type { PasswordStrength } from './types'

const LABELS: Record<PasswordStrength['label'], string> = {
  debil: 'Débil',
  aceptable: 'Aceptable',
  fuerte: 'Fuerte',
}

const COLORS: Record<PasswordStrength['label'], string> = {
  debil: 'bg-red-500',
  aceptable: 'bg-amber-500',
  fuerte: 'bg-emerald-500',
}

/**
 * Solo retroalimentación visual — la validación real (mínimo 12 caracteres,
 * al menos 2 tipos de carácter) ocurre en Rust y es la que de verdad bloquea
 * la contraseña, no este medidor.
 */
export function PasswordStrengthMeter({ password }: { password: string }) {
  const [strength, setStrength] = useState<PasswordStrength | null>(null)

  useEffect(() => {
    if (!password) {
      setStrength(null)
      return
    }
    let cancelled = false
    authApi.evaluatePasswordStrength(password).then((s) => {
      if (!cancelled) setStrength(s)
    })
    return () => {
      cancelled = true
    }
  }, [password])

  if (!strength) return null

  return (
    <div className="flex items-center gap-2">
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-slate-200">
        <div
          className={`h-full transition-all ${COLORS[strength.label]}`}
          style={{ width: `${strength.score}%` }}
        />
      </div>
      <span className="w-16 text-right text-xs text-slate-500">{LABELS[strength.label]}</span>
    </div>
  )
}
