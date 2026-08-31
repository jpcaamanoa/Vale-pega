import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { patientsApi } from '../patients/api'
import { ComingSoonCard } from './ComingSoonCard'

/**
 * Pantalla de inicio real de la aplicación (Fase 2). Sección "Resumen"
 * consume `patientsApi.list()` — el mismo comando de la Fase 1.5, sin
 * cambios de backend — para mostrar un conteo real de pacientes activos.
 * "Hoy" y "Pendientes", y las dos métricas de "Resumen" que dependen de
 * Agenda/Sesiones/Pagos (que todavía no existen), se muestran como
 * `ComingSoonCard` — nunca con un número inventado.
 */
export function DashboardScreen() {
  const navigate = useNavigate()
  const [activePatientCount, setActivePatientCount] = useState<number | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    patientsApi
      .list()
      .then((patients) => {
        if (!cancelled) {
          setActivePatientCount(patients.length)
          setError(null)
        }
      })
      .catch((err) => {
        if (!cancelled) setError(typeof err === 'string' ? err : 'No se pudo cargar el resumen de pacientes.')
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-10">
      <h1 className="text-xl font-semibold text-foreground">Inicio</h1>

      {error && <p className="text-sm text-danger">{error}</p>}

      <div className="grid grid-cols-1 gap-6 md:grid-cols-3">
        <ComingSoonCard
          title="Hoy"
          description="Aquí verás las sesiones de hoy, con hora, paciente y estado, cuando exista la funcionalidad de Agenda."
        />

        <ComingSoonCard
          title="Pendientes"
          description="Aquí verás notas sin cerrar, pagos pendientes, tareas clínicas y documentos pendientes, cuando existan esas funcionalidades."
        />

        <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
          <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Resumen</h3>

          <button
            onClick={() => navigate('/patients')}
            className="flex items-center justify-between rounded-lg border border-border p-4 text-left transition-colors hover:border-accent hover:bg-accent-soft"
          >
            <span className="text-sm text-foreground">Pacientes activos</span>
            <span className="text-2xl font-semibold text-accent">{loading ? '—' : (activePatientCount ?? '—')}</span>
          </button>

          <div className="flex items-center justify-between rounded-lg border border-border p-4">
            <span className="text-sm text-foreground">Sesiones del mes</span>
            <span className="rounded-full bg-disabled px-2 py-0.5 text-xs font-medium text-disabled-foreground">
              Próximamente
            </span>
          </div>

          <div className="flex items-center justify-between rounded-lg border border-border p-4">
            <span className="text-sm text-foreground">Ingresos del mes</span>
            <span className="rounded-full bg-disabled px-2 py-0.5 text-xs font-medium text-disabled-foreground">
              Próximamente
            </span>
          </div>
        </section>
      </div>
    </div>
  )
}
