import { zodResolver } from '@hookform/resolvers/zod'
import { useState } from 'react'
import { useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '../../components/ui/Button'
import { PasswordField } from '../../components/ui/PasswordField'
import { authApi, errorMessage } from './api'
import { AuthShell } from './AuthShell'
import { PasswordStrengthMeter } from './PasswordStrengthMeter'

function countCharacterClasses(password: string): number {
  return [/[a-z]/, /[A-Z]/, /[0-9]/, /[^a-zA-Z0-9]/].filter((re) => re.test(password)).length
}

const schema = z
  .object({
    recoveryCode: z.string().min(1, 'Ingresa tu código de recuperación'),
    newPassword: z
      .string()
      .min(12, 'La contraseña debe tener al menos 12 caracteres')
      .refine((p) => countCharacterClasses(p) >= 2, {
        message: 'Combina al menos 2 tipos de carácter (minúsculas, mayúsculas, números, símbolos)',
      }),
    confirmPassword: z.string(),
  })
  .refine((data) => data.newPassword === data.confirmPassword, {
    message: 'Las contraseñas no coinciden',
    path: ['confirmPassword'],
  })

type FormValues = z.infer<typeof schema>

export function RecoverAccessScreen({ onRecovered, onBack }: { onRecovered: () => void; onBack: () => void }) {
  const [serverError, setServerError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { recoveryCode: '', newPassword: '', confirmPassword: '' },
  })

  const newPassword = watch('newPassword')

  const onSubmit = async (values: FormValues) => {
    setServerError(null)
    try {
      await authApi.recoverVaultAccess(values.recoveryCode, values.newPassword)
      onRecovered()
    } catch (err) {
      setServerError(errorMessage(err))
    }
  }

  return (
    <AuthShell
      title="Recuperar acceso"
      subtitle="Ingresa tu código de recuperación y elige una nueva contraseña."
    >
      <form onSubmit={handleSubmit(onSubmit)} className="flex flex-col gap-4">
        <div className="flex flex-col gap-1.5">
          <label htmlFor="recoveryCode" className="text-sm font-medium text-slate-700">
            Código de recuperación
          </label>
          <input
            id="recoveryCode"
            placeholder="XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"
            autoComplete="off"
            className="w-full rounded-lg border border-slate-300 px-3 py-2.5 font-mono text-sm uppercase tracking-wide outline-none focus:border-slate-500 focus:ring-1 focus:ring-slate-500"
            {...register('recoveryCode')}
          />
          {errors.recoveryCode && <p className="text-sm text-red-600">{errors.recoveryCode.message}</p>}
        </div>
        <div className="flex flex-col gap-1.5">
          <PasswordField
            label="Nueva contraseña"
            autoComplete="new-password"
            {...register('newPassword')}
            error={errors.newPassword?.message}
          />
          <PasswordStrengthMeter password={newPassword} />
        </div>
        <PasswordField
          label="Confirmar nueva contraseña"
          autoComplete="new-password"
          {...register('confirmPassword')}
          error={errors.confirmPassword?.message}
        />
        {serverError && <p className="text-sm text-red-600">{serverError}</p>}
        <Button type="submit" disabled={isSubmitting} className="w-full">
          {isSubmitting ? 'Verificando…' : 'Recuperar acceso'}
        </Button>
        <button type="button" onClick={onBack} className="text-center text-xs text-slate-500 hover:text-slate-700">
          Volver a desbloquear con contraseña
        </button>
      </form>
    </AuthShell>
  )
}
