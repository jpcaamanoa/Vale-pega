import { zodResolver } from '@hookform/resolvers/zod'
import { useState } from 'react'
import { useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '../../components/ui/Button'
import { PasswordField } from '../../components/ui/PasswordField'
import { authApi, errorMessage } from './api'
import { PasswordStrengthMeter } from './PasswordStrengthMeter'

function countCharacterClasses(password: string): number {
  return [/[a-z]/, /[A-Z]/, /[0-9]/, /[^a-zA-Z0-9]/].filter((re) => re.test(password)).length
}

const schema = z
  .object({
    currentPassword: z.string().min(1, 'Ingresa tu contraseña actual'),
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

export function ChangePasswordModal({ onClose, onChanged }: { onClose: () => void; onChanged: () => void }) {
  const [serverError, setServerError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { currentPassword: '', newPassword: '', confirmPassword: '' },
  })

  const newPassword = watch('newPassword')

  const onSubmit = async (values: FormValues) => {
    setServerError(null)
    try {
      await authApi.changeVaultPassword(values.currentPassword, values.newPassword)
      onChanged()
    } catch (err) {
      setServerError(errorMessage(err))
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 px-4">
      <div className="w-full max-w-sm rounded-2xl bg-white p-6 shadow-lg">
        <h2 className="mb-4 text-base font-semibold text-slate-900">Cambiar contraseña</h2>
        <form onSubmit={handleSubmit(onSubmit)} className="flex flex-col gap-4">
          <PasswordField
            label="Contraseña actual"
            autoComplete="current-password"
            {...register('currentPassword')}
            error={errors.currentPassword?.message}
          />
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
          <div className="mt-2 flex gap-2">
            <Button type="button" variant="secondary" onClick={onClose} className="flex-1">
              Cancelar
            </Button>
            <Button type="submit" disabled={isSubmitting} className="flex-1">
              {isSubmitting ? 'Guardando…' : 'Guardar'}
            </Button>
          </div>
        </form>
      </div>
    </div>
  )
}
