import { zodResolver } from '@hookform/resolvers/zod'
import type { ReactNode } from 'react'
import { useForm } from 'react-hook-form'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { patientFormSchema, type PatientFormValues } from './schema'
import { PATIENT_STATUS_LABELS, type Patient } from './types'

function patientToFormValues(patient?: Patient): Partial<PatientFormValues> {
  if (!patient) return { status: 'activo' }
  return {
    fullName: patient.fullName,
    preferredName: patient.preferredName ?? undefined,
    rut: patient.rut ?? undefined,
    birthDate: patient.birthDate ?? undefined,
    phone: patient.phone ?? undefined,
    email: patient.email ?? undefined,
    address: patient.address ?? undefined,
    emergencyContactName: patient.emergencyContactName ?? undefined,
    emergencyContactPhone: patient.emergencyContactPhone ?? undefined,
    emergencyContactRelationship: patient.emergencyContactRelationship ?? undefined,
    status: patient.status,
    referredBy: patient.referredBy ?? undefined,
    intakeDate: patient.intakeDate ?? undefined,
  }
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-4 border-b border-border pb-6 last:border-b-0 last:pb-0">
      <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">{title}</h3>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">{children}</div>
    </section>
  )
}

export function PatientForm({
  patient,
  onSubmit,
  onCancel,
  submitLabel,
}: {
  patient?: Patient
  onSubmit: (values: PatientFormValues) => Promise<void>
  onCancel: () => void
  submitLabel: string
}) {
  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<PatientFormValues>({
    resolver: zodResolver(patientFormSchema),
    defaultValues: patientToFormValues(patient),
  })

  return (
    <form onSubmit={handleSubmit(onSubmit)} className="flex flex-col gap-6">
      <Section title="Datos personales">
        <TextField label="Nombre completo *" {...register('fullName')} error={errors.fullName?.message} />
        <TextField label="Nombre preferido" {...register('preferredName')} error={errors.preferredName?.message} />
        <TextField label="RUT" placeholder="12.345.678-5" {...register('rut')} error={errors.rut?.message} />
        <TextField label="Fecha de nacimiento" type="date" {...register('birthDate')} error={errors.birthDate?.message} />
      </Section>

      <Section title="Contacto">
        <TextField label="Teléfono" {...register('phone')} error={errors.phone?.message} />
        <TextField label="Correo electrónico" {...register('email')} error={errors.email?.message} />
        <div className="sm:col-span-2">
          <TextField label="Dirección" {...register('address')} error={errors.address?.message} />
        </div>
      </Section>

      <Section title="Contacto de emergencia">
        <TextField
          label="Nombre"
          {...register('emergencyContactName')}
          error={errors.emergencyContactName?.message}
        />
        <TextField
          label="Teléfono"
          {...register('emergencyContactPhone')}
          error={errors.emergencyContactPhone?.message}
        />
        <TextField
          label="Relación"
          {...register('emergencyContactRelationship')}
          error={errors.emergencyContactRelationship?.message}
        />
      </Section>

      <Section title="Información administrativa">
        <Select label="Estado" {...register('status')} error={errors.status?.message}>
          {Object.entries(PATIENT_STATUS_LABELS).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </Select>
        <TextField label="Derivado por" {...register('referredBy')} error={errors.referredBy?.message} />
        <TextField label="Fecha de ingreso" type="date" {...register('intakeDate')} error={errors.intakeDate?.message} />
      </Section>

      <div className="flex justify-end gap-2 pt-2">
        <Button type="button" variant="secondary" onClick={onCancel}>
          Cancelar
        </Button>
        <Button type="submit" disabled={isSubmitting}>
          {isSubmitting ? 'Guardando…' : submitLabel}
        </Button>
      </div>
    </form>
  )
}
