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

// Réplica en el cliente de la validación real de Rust (security::password_policy),
// solo para dar retroalimentación inmediata — la autoritativa sigue siendo la de Rust.
const schema = z
  .object({
    password: z
      .string()
      .min(12, 'La contraseña debe tener al menos 12 caracteres')
      .refine((p) => countCharacterClasses(p) >= 2, {
        message: 'Combina al menos 2 tipos de carácter (minúsculas, mayúsculas, números, símbolos)',
      }),
    confirmPassword: z.string(),
  })
  .refine((data) => data.password === data.confirmPassword, {
    message: 'Las contraseñas no coinciden',
    path: ['confirmPassword'],
  })

type FormValues = z.infer<typeof schema>

export function CreateVaultScreen({ onCreated }: { onCreated: (recoveryCode: string) => void }) {
  const [serverError, setServerError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<FormValues>({ resolver: zodResolver(schema), defaultValues: { password: '', confirmPassword: '' } })

  const password = watch('password')

  const onSubmit = async (values: FormValues) => {
    setServerError(null)
    try {
      const recoveryCode = await authApi.beginVaultCreation(values.password)
      onCreated(recoveryCode)
    } catch (err) {
      setServerError(errorMessage(err))
    }
  }

  return (
    <AuthShell title="Crear tu cuaderno clínico" subtitle="Elige una contraseña maestra para cifrar tus datos.">
      <form onSubmit={handleSubmit(onSubmit)} className="flex flex-col gap-4">
        <div className="flex flex-col gap-1.5">
          <PasswordField
            label="Contraseña"
            autoComplete="new-password"
            {...register('password')}
            error={errors.password?.message}
          />
          <PasswordStrengthMeter password={password} />
        </div>
        <PasswordField
          label="Confirmar contraseña"
          autoComplete="new-password"
          {...register('confirmPassword')}
          error={errors.confirmPassword?.message}
        />
        {serverError && <p className="text-sm text-danger">{serverError}</p>}
        <Button type="submit" disabled={isSubmitting} className="mt-2 w-full">
          {isSubmitting ? 'Creando…' : 'Continuar'}
        </Button>
      </form>
    </AuthShell>
  )
}
