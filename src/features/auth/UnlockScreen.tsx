import { zodResolver } from '@hookform/resolvers/zod'
import { useState } from 'react'
import { useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '../../components/ui/Button'
import { PasswordField } from '../../components/ui/PasswordField'
import { authApi, errorMessage } from './api'
import { AuthShell } from './AuthShell'

const schema = z.object({
  password: z.string().min(1, 'Ingresa tu contraseña'),
})
type FormValues = z.infer<typeof schema>

export function UnlockScreen({ onUnlocked, onForgotPassword }: { onUnlocked: () => void; onForgotPassword: () => void }) {
  const [serverError, setServerError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<FormValues>({ resolver: zodResolver(schema), defaultValues: { password: '' } })

  const onSubmit = async (values: FormValues) => {
    setServerError(null)
    try {
      await authApi.unlockVault(values.password)
      onUnlocked()
    } catch (err) {
      // Mensaje deliberadamente genérico para contraseña incorrecta —
      // ver security::vault_manager::UnlockError.
      setServerError(errorMessage(err))
    }
  }

  return (
    <AuthShell title="Desbloquear">
      <form onSubmit={handleSubmit(onSubmit)} className="flex flex-col gap-4">
        <PasswordField
          label="Contraseña"
          autoComplete="current-password"
          autoFocus
          {...register('password')}
          error={errors.password?.message}
        />
        {serverError && <p className="text-sm text-danger">{serverError}</p>}
        <Button type="submit" disabled={isSubmitting} className="w-full">
          {isSubmitting ? 'Verificando…' : 'Desbloquear'}
        </Button>
        <button
          type="button"
          onClick={onForgotPassword}
          className="text-center text-xs text-muted-foreground hover:text-accent"
        >
          ¿Olvidaste tu contraseña? Recuperar acceso
        </button>
      </form>
    </AuthShell>
  )
}
