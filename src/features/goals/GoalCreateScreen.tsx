import { zodResolver } from '@hookform/resolvers/zod'
import { useState } from 'react'
import { useForm } from 'react-hook-form'
import { useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { goalsApi } from './api'
import { goalCreateFormSchema, type GoalCreateFormValues } from './schema'
import type { GoalInput } from './types'

/**
 * Un objetivo puede guardarse sin indicadores — se agregan después desde
 * `GoalDetailScreen`. No hay autoguardado aquí (a diferencia de las notas
 * de sesión): los objetivos se guardan mediante una acción explícita.
 */
export function GoalCreateScreen() {
  const { patientId } = useParams<{ patientId: string }>()
  const navigate = useNavigate()
  const [error, setError] = useState<string | null>(null)

  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<GoalCreateFormValues>({
    resolver: zodResolver(goalCreateFormSchema),
    defaultValues: { title: '', description: '', targetDate: '' },
  })

  if (!patientId) return null

  const submit = async (values: GoalCreateFormValues) => {
    setError(null)
    try {
      const input: GoalInput = {
        patientId,
        title: values.title,
        description: values.description || null,
        targetDate: values.targetDate || null,
      }
      const created = await goalsApi.create(input)
      navigate(`/patients/${patientId}/goals/${created.id}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo crear el objetivo.')
    }
  }

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="mb-6 text-xl font-semibold text-foreground">Nuevo objetivo</h1>
      {error && <p className="mb-4 text-sm text-danger">{error}</p>}
      <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-6">
        <TextField label="Título" {...register('title')} error={errors.title?.message} />
        <Textarea label="Descripción" {...register('description')} error={errors.description?.message} />
        <TextField
          label="Fecha objetivo"
          type="date"
          {...register('targetDate')}
          error={errors.targetDate?.message}
        />
        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" variant="secondary" onClick={() => navigate(`/patients/${patientId}`)}>
            Cancelar
          </Button>
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? 'Creando…' : 'Crear objetivo'}
          </Button>
        </div>
      </form>
    </div>
  )
}
