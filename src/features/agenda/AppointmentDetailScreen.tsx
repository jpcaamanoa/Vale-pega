import { useEffect, useState } from 'react'
import { Link, useLocation, useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { sessionsApi } from '../sessions/api'
import type { Session } from '../sessions/types'
import { agendaApi } from './api'
import { formatLocalDateTime } from './datetime'
import { SyncOutcomeBanner } from './SyncOutcomeBanner'
import { APPOINTMENT_MODALITY_LABELS, type Appointment, type SyncOutcome } from './types'

function ConfirmDialog({
  title,
  description,
  confirmLabel,
  onDismiss,
  onConfirm,
}: {
  title: string
  description: string
  confirmLabel: string
  onDismiss: () => void
  onConfirm: () => void
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
      <div className="w-full max-w-sm rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-2 text-base font-semibold text-foreground">{title}</h2>
        <p className="mb-4 text-sm text-muted-foreground">{description}</p>
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onDismiss}>
            Volver
          </Button>
          <Button onClick={onConfirm}>{confirmLabel}</Button>
        </div>
      </div>
    </div>
  )
}

export function AppointmentDetailScreen() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const location = useLocation()
  const [appointment, setAppointment] = useState<Appointment | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [syncOutcome, setSyncOutcome] = useState<SyncOutcome | null>(
    () => (location.state as { syncOutcome?: SyncOutcome } | null)?.syncOutcome ?? null,
  )
  const [confirmingCancel, setConfirmingCancel] = useState(false)
  const [confirmingArchive, setConfirmingArchive] = useState(false)
  const [confirmingRestore, setConfirmingRestore] = useState(false)
  const [retrying, setRetrying] = useState(false)
  // `undefined` = todavía no se consultó; `null` = no hay sesión asociada.
  const [sessionForAppointment, setSessionForAppointment] = useState<Session | null | undefined>(undefined)

  const load = () => {
    if (!id) return
    agendaApi
      .get(id)
      .then(setAppointment)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar la cita.'))
  }

  useEffect(load, [id])

  useEffect(() => {
    if (!id || !appointment?.patientId) {
      setSessionForAppointment(null)
      return
    }
    sessionsApi
      .getForAppointment(id)
      .then(setSessionForAppointment)
      .catch(() => setSessionForAppointment(null))
  }, [id, appointment?.patientId])

  const handleCancel = async () => {
    if (!id) return
    try {
      const result = await agendaApi.cancel(id)
      setAppointment(result)
      setSyncOutcome(result.syncOutcome)
      setConfirmingCancel(false)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo cancelar la cita.')
    }
  }

  const handleArchive = async () => {
    if (!id) return
    try {
      await agendaApi.archive(id)
      navigate('/agenda')
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo archivar la cita.')
    }
  }

  const handleRestore = async () => {
    if (!id) return
    try {
      const result = await agendaApi.restore(id)
      setAppointment(result)
      setSyncOutcome(result.syncOutcome)
      setConfirmingRestore(false)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo restaurar la cita.')
    }
  }

  const handleRetry = async () => {
    if (!id) return
    setRetrying(true)
    try {
      const result = await agendaApi.retrySync(id)
      setAppointment(result)
      setSyncOutcome(result.syncOutcome)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo reintentar la sincronización.')
    } finally {
      setRetrying(false)
    }
  }

  if (error) return <p className="p-10 text-sm text-danger">{error}</p>
  if (!appointment) return <p className="p-10 text-sm text-muted-foreground">Cargando…</p>

  const isArchived = appointment.deletedAt !== null
  const isCancelled = appointment.status === 'cancelada'
  const isLinkedToGoogle = appointment.googleEventId !== null

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      {isArchived && (
        <div className="mb-6 rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
          Esta cita está archivada. No aparece en la agenda activa hasta que se restaure.
        </div>
      )}

      <div className="mb-2">
        <SyncOutcomeBanner outcome={syncOutcome} />
      </div>

      <div className="mb-6 flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold text-foreground">
            {appointment.patientId ? appointment.patientName : 'Bloqueo personal'}
          </h1>
          <p className="text-sm text-muted-foreground">
            {formatLocalDateTime(appointment.startsAt)} – {formatLocalDateTime(appointment.endsAt)}
          </p>
          {isCancelled && (
            <span className="mt-1 inline-block rounded-full bg-disabled px-2 py-0.5 text-xs font-medium text-disabled-foreground">
              Cancelada
            </span>
          )}
        </div>
        <div className="flex flex-wrap justify-end gap-2">
          {isArchived ? (
            <Button variant="secondary" onClick={() => setConfirmingRestore(true)}>
              Restaurar
            </Button>
          ) : (
            <>
              <Button variant="secondary" onClick={() => navigate(`/agenda/${id}/edit`)}>
                Editar
              </Button>
              {!isCancelled && (
                <Button variant="secondary" onClick={() => setConfirmingCancel(true)}>
                  Cancelar cita
                </Button>
              )}
              <Button variant="secondary" onClick={() => setConfirmingArchive(true)}>
                Archivar
              </Button>
            </>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Detalles</h3>
        <div className="grid grid-cols-1 gap-2 text-sm sm:grid-cols-2">
          <div className="flex justify-between border-b border-border py-2 sm:col-span-2">
            <span className="text-muted-foreground">Paciente</span>
            {appointment.patientId ? (
              <Link to={`/patients/${appointment.patientId}`} className="text-accent hover:underline">
                {appointment.patientName}
              </Link>
            ) : (
              <span className="text-foreground">— (bloqueo personal)</span>
            )}
          </div>
          <div className="flex justify-between border-b border-border py-2">
            <span className="text-muted-foreground">Modalidad</span>
            <span className="text-foreground">
              {appointment.modality ? APPOINTMENT_MODALITY_LABELS[appointment.modality] : '—'}
            </span>
          </div>
          <div className="flex justify-between border-b border-border py-2">
            <span className="text-muted-foreground">Estado</span>
            <span className="text-foreground">{isCancelled ? 'Cancelada' : 'Programada'}</span>
          </div>
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-border pt-4">
          <div className="text-sm">
            <span className="text-muted-foreground">Google Calendar: </span>
            <span className="text-foreground">
              {isLinkedToGoogle
                ? `Vinculado${appointment.lastSyncedAt ? ` (última sincronización: ${formatLocalDateTime(appointment.lastSyncedAt)})` : ''}`
                : 'No sincronizado'}
            </span>
          </div>
          <Button variant="ghost" onClick={handleRetry} disabled={retrying}>
            {retrying ? 'Sincronizando…' : 'Reintentar sincronización'}
          </Button>
        </div>

        {appointment.patientId && (
          <div className="flex items-center justify-between gap-3 border-t border-border pt-4">
            <span className="text-sm text-muted-foreground">Sesión clínica</span>
            {sessionForAppointment === undefined ? (
              <span className="text-sm text-muted-foreground">Cargando…</span>
            ) : sessionForAppointment ? (
              <Link
                to={`/patients/${appointment.patientId}/sessions/${sessionForAppointment.id}`}
                className="text-sm text-accent hover:underline"
              >
                Ver sesión
              </Link>
            ) : !isArchived ? (
              <Button
                variant="secondary"
                onClick={() => navigate(`/patients/${appointment.patientId}/sessions/new?appointmentId=${id}`)}
              >
                Iniciar sesión
              </Button>
            ) : (
              <span className="text-sm text-muted-foreground">—</span>
            )}
          </div>
        )}
      </div>

      {confirmingCancel && (
        <ConfirmDialog
          title="Cancelar cita"
          description="La cita quedará marcada como cancelada, pero seguirá visible como registro histórico. Si tenía un evento en Google Calendar, se eliminará."
          confirmLabel="Cancelar cita"
          onDismiss={() => setConfirmingCancel(false)}
          onConfirm={handleCancel}
        />
      )}

      {confirmingArchive && (
        <ConfirmDialog
          title="Archivar cita"
          description="La cita se marcará como archivada y dejará de aparecer en la agenda activa. No se elimina ninguna información — puede recuperarse más adelante. Si tenía un evento en Google Calendar, se eliminará."
          confirmLabel="Archivar"
          onDismiss={() => setConfirmingArchive(false)}
          onConfirm={handleArchive}
        />
      )}

      {confirmingRestore && (
        <ConfirmDialog
          title="Restaurar cita"
          description="La cita volverá a aparecer en la agenda activa, con todos sus datos intactos."
          confirmLabel="Restaurar"
          onDismiss={() => setConfirmingRestore(false)}
          onConfirm={handleRestore}
        />
      )}
    </div>
  )
}
