import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useRef, useState } from 'react'
import { useForm } from 'react-hook-form'
import { useNavigate, useParams } from 'react-router-dom'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { Textarea } from '../../components/ui/Textarea'
import { TextField } from '../../components/ui/TextField'
import { sessionsApi } from './api'
import { formatSessionDate } from './datetime'
import { sessionMetadataFormSchema, type SessionMetadataFormValues } from './schema'
import { SESSION_MODALITY_LABELS, SESSION_STATUS_LABELS, type Session, type SessionMetadataInput, type SessionNote } from './types'

const AUTOSAVE_DEBOUNCE_MS = 800

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

function SessionMetadataForm({ session, onSaved }: { session: Session; onSaved: (session: Session) => void }) {
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<SessionMetadataFormValues>({
    resolver: zodResolver(sessionMetadataFormSchema),
    defaultValues: {
      sessionDate: session.sessionDate,
      startTime: session.startTime ?? '',
      durationMinutes: session.durationMinutes ? String(session.durationMinutes) : '',
      modality: session.modality ?? '',
      status: session.status,
    },
  })

  const submit = async (values: SessionMetadataFormValues) => {
    setError(null)
    setSaved(false)
    try {
      const input: SessionMetadataInput = {
        sessionDate: values.sessionDate,
        startTime: values.startTime || null,
        durationMinutes: values.durationMinutes ? Number(values.durationMinutes) : null,
        modality: (values.modality || null) as SessionMetadataInput['modality'],
        status: values.status,
      }
      const updated = await sessionsApi.updateMetadata(session.id, input)
      onSaved(updated)
      setSaved(true)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar la sesión.')
    }
  }

  return (
    <form onSubmit={handleSubmit(submit)} className="flex flex-col gap-4">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <TextField label="Fecha" type="date" {...register('sessionDate')} error={errors.sessionDate?.message} />
        <TextField label="Hora de inicio" type="time" {...register('startTime')} error={errors.startTime?.message} />
        <TextField
          label="Duración (minutos)"
          type="number"
          min={1}
          {...register('durationMinutes')}
          error={errors.durationMinutes?.message}
        />
        <Select label="Modalidad" {...register('modality')}>
          <option value="">— Sin especificar —</option>
          {Object.entries(SESSION_MODALITY_LABELS).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </Select>
        <Select label="Estado" {...register('status')} error={errors.status?.message}>
          {Object.entries(SESSION_STATUS_LABELS).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </Select>
      </div>
      {error && <p className="text-sm text-danger">{error}</p>}
      <div className="flex items-center gap-3">
        <Button type="submit" variant="secondary" disabled={isSubmitting}>
          {isSubmitting ? 'Guardando…' : 'Guardar cambios'}
        </Button>
        {saved && !isSubmitting && <span className="text-sm text-success">Guardado.</span>}
      </div>
    </form>
  )
}

type SaveStatus = 'idle' | 'saving' | 'saved' | 'error'

function saveStatusLabel(status: SaveStatus): string | null {
  switch (status) {
    case 'saving':
      return 'Guardando…'
    case 'saved':
      return 'Guardado'
    case 'error':
      return 'Error al guardar — se reintentará con el próximo cambio'
    case 'idle':
      return null
  }
}

interface DraftFields {
  content: string
  interventions: string
  homeworkTasks: string
  nextFocus: string
}

const emptyDraft: DraftFields = { content: '', interventions: '', homeworkTasks: '', nextFocus: '' }

export function SessionDetailScreen() {
  const { patientId, sessionId } = useParams<{ patientId: string; sessionId: string }>()
  const navigate = useNavigate()
  const [session, setSession] = useState<Session | null>(null)
  const [note, setNote] = useState<SessionNote | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [closeError, setCloseError] = useState<string | null>(null)
  const [confirmingArchive, setConfirmingArchive] = useState(false)
  const [confirmingRestore, setConfirmingRestore] = useState(false)
  const [confirmingNewVersion, setConfirmingNewVersion] = useState(false)
  const [showHistory, setShowHistory] = useState(false)
  const [history, setHistory] = useState<SessionNote[] | null>(null)
  const [loadingHistory, setLoadingHistory] = useState(false)

  const [draft, setDraft] = useState<DraftFields>(emptyDraft)
  const [saveStatus, setSaveStatus] = useState<SaveStatus>('idle')
  const skipNextAutosaveRef = useRef(true)

  const load = () => {
    if (!sessionId) return
    setError(null)
    Promise.all([sessionsApi.get(sessionId), sessionsApi.getCurrentNote(sessionId)])
      .then(([s, n]) => {
        setSession(s)
        setNote(n)
      })
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar la sesión.'))
  }

  useEffect(load, [sessionId])

  // Cada vez que cambia la identidad de la nota vigente (carga inicial, o
  // tras cerrar/crear una versión nueva), se resetea el borrador local y se
  // marca el próximo cambio de `draft` como "no disparar autoguardado" —
  // ese primer cambio es el propio reset, no una edición de la usuaria.
  useEffect(() => {
    if (note && !note.isLocked) {
      skipNextAutosaveRef.current = true
      setDraft({
        content: note.content ?? '',
        interventions: note.interventions ?? '',
        homeworkTasks: note.homeworkTasks ?? '',
        nextFocus: note.nextFocus ?? '',
      })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [note?.id])

  useEffect(() => {
    if (skipNextAutosaveRef.current) {
      skipNextAutosaveRef.current = false
      return
    }
    if (!sessionId || !note || note.isLocked) return
    setSaveStatus('saving')
    const timeout = window.setTimeout(() => {
      sessionsApi
        .autosaveDraft(sessionId, draft)
        .then(() => setSaveStatus('saved'))
        .catch(() => setSaveStatus('error'))
    }, AUTOSAVE_DEBOUNCE_MS)
    return () => window.clearTimeout(timeout)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft])

  const handleArchive = async () => {
    if (!sessionId) return
    try {
      await sessionsApi.archive(sessionId)
      navigate(`/patients/${patientId}`)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo archivar la sesión.')
    }
  }

  const handleRestore = async () => {
    if (!sessionId) return
    try {
      const restored = await sessionsApi.restore(sessionId)
      setSession(restored)
      setConfirmingRestore(false)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo restaurar la sesión.')
    }
  }

  const handleClose = async () => {
    if (!sessionId) return
    setCloseError(null)
    try {
      const closed = await sessionsApi.closeCurrentNote(sessionId)
      setNote(closed)
    } catch (err) {
      setCloseError(typeof err === 'string' ? err : 'No se pudo cerrar la nota.')
    }
  }

  const handleCreateNewVersion = async () => {
    if (!sessionId) return
    try {
      const newNote = await sessionsApi.createNewNoteVersion(sessionId)
      setNote(newNote)
      setConfirmingNewVersion(false)
      setShowHistory(false)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo crear una nueva versión de la nota.')
    }
  }

  const toggleHistory = () => {
    if (showHistory) {
      setShowHistory(false)
      return
    }
    if (!sessionId) return
    setShowHistory(true)
    setLoadingHistory(true)
    sessionsApi
      .listNoteHistory(sessionId)
      .then(setHistory)
      .catch(() => setHistory([]))
      .finally(() => setLoadingHistory(false))
  }

  if (error) return <p className="p-10 text-sm text-danger">{error}</p>
  if (!session || !note) return <p className="p-10 text-sm text-muted-foreground">Cargando…</p>

  const isArchived = session.deletedAt !== null

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-8 px-6 py-10">
      {isArchived && (
        <div className="rounded-lg border border-warning/40 bg-warning-soft px-4 py-3 text-sm text-warning">
          Esta sesión está archivada. No aparece en el listado activo hasta que se restaure. Su nota e historial
          siguen intactos.
        </div>
      )}

      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold text-foreground">Sesión del {formatSessionDate(session.sessionDate)}</h1>
          <button onClick={() => navigate(`/patients/${patientId}`)} className="text-sm text-accent hover:underline">
            Volver a la ficha del paciente
          </button>
        </div>
        <div className="flex gap-2">
          {isArchived ? (
            <Button variant="secondary" onClick={() => setConfirmingRestore(true)}>
              Restaurar
            </Button>
          ) : (
            <Button variant="secondary" onClick={() => setConfirmingArchive(true)}>
              Archivar
            </Button>
          )}
        </div>
      </div>

      <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Información de la sesión</h3>
        <SessionMetadataForm session={session} onSaved={setSession} />
      </section>

      <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Nota clínica</h3>
          <span
            className={`rounded-full px-2.5 py-1 text-xs font-medium ${
              note.isLocked ? 'bg-success-soft text-success' : 'bg-warning-soft text-warning'
            }`}
          >
            {note.isLocked ? `Cerrada (versión ${note.version})` : `Borrador (versión ${note.version})`}
          </span>
        </div>

        {note.isLocked ? (
          <>
            <NoteFieldsReadOnly note={note} />
            <div className="flex items-center gap-3 border-t border-border pt-4">
              <Button variant="secondary" onClick={() => setConfirmingNewVersion(true)}>
                Editar
              </Button>
              <button onClick={toggleHistory} className="text-sm text-accent hover:underline">
                {showHistory ? 'Ocultar historial' : 'Ver historial'}
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="flex flex-col gap-4">
              <Textarea
                label="Contenido"
                value={draft.content}
                onChange={(e) => setDraft((d) => ({ ...d, content: e.target.value }))}
              />
              <Textarea
                label="Intervenciones"
                value={draft.interventions}
                onChange={(e) => setDraft((d) => ({ ...d, interventions: e.target.value }))}
              />
              <Textarea
                label="Tareas para la casa"
                value={draft.homeworkTasks}
                onChange={(e) => setDraft((d) => ({ ...d, homeworkTasks: e.target.value }))}
              />
              <Textarea
                label="Foco de la próxima sesión"
                value={draft.nextFocus}
                onChange={(e) => setDraft((d) => ({ ...d, nextFocus: e.target.value }))}
              />
            </div>

            <div className="flex items-center justify-between border-t border-border pt-4">
              <div className="flex items-center gap-4">
                <Button onClick={handleClose}>Cerrar nota</Button>
                {history !== null || note.version > 1 ? (
                  <button onClick={toggleHistory} className="text-sm text-accent hover:underline">
                    {showHistory ? 'Ocultar historial' : 'Ver historial'}
                  </button>
                ) : null}
              </div>
              {saveStatusLabel(saveStatus) && (
                <span className={`text-xs ${saveStatus === 'error' ? 'text-danger' : 'text-muted-foreground'}`}>
                  {saveStatusLabel(saveStatus)}
                </span>
              )}
            </div>
            {closeError && <p className="text-sm text-danger">{closeError}</p>}
          </>
        )}

        {showHistory && (
          <div className="flex flex-col gap-3 border-t border-border pt-4">
            <h4 className="text-sm font-semibold text-foreground">Historial de versiones</h4>
            {loadingHistory && <p className="text-sm text-muted-foreground">Cargando…</p>}
            {!loadingHistory &&
              history?.map((version) => (
                <div key={version.id} className="rounded-lg border border-border p-4">
                  <div className="mb-2 flex items-center justify-between">
                    <span className="text-sm font-medium text-foreground">Versión {version.version}</span>
                    {version.isCurrent ? (
                      <span className="rounded-full bg-accent-soft px-2 py-0.5 text-xs font-medium text-accent">Vigente</span>
                    ) : (
                      <span className="text-xs text-muted-foreground">
                        Reemplazada{version.supersededAt ? ` el ${formatSessionDate(version.supersededAt.slice(0, 10))}` : ''}
                      </span>
                    )}
                  </div>
                  <NoteFieldsReadOnly note={version} />
                </div>
              ))}
          </div>
        )}
      </section>

      {confirmingArchive && (
        <ConfirmDialog
          title="Archivar sesión"
          description="La sesión se marcará como archivada y dejará de aparecer en el listado activo. No se elimina ninguna información — la nota y su historial completo permanecen intactos y pueden recuperarse más adelante."
          confirmLabel="Archivar"
          onDismiss={() => setConfirmingArchive(false)}
          onConfirm={handleArchive}
        />
      )}

      {confirmingRestore && (
        <ConfirmDialog
          title="Restaurar sesión"
          description="La sesión volverá a aparecer en el listado activo, con la nota y todo su historial intactos."
          confirmLabel="Restaurar"
          onDismiss={() => setConfirmingRestore(false)}
          onConfirm={handleRestore}
        />
      )}

      {confirmingNewVersion && (
        <ConfirmDialog
          title="Editar esta nota"
          description="Editar esta nota creará una nueva versión. La versión anterior permanecerá intacta y seguirá disponible en el historial."
          confirmLabel="Crear nueva versión"
          onDismiss={() => setConfirmingNewVersion(false)}
          onConfirm={handleCreateNewVersion}
        />
      )}
    </div>
  )
}

function NoteFieldsReadOnly({ note }: { note: SessionNote }) {
  const fields: [string, string | null][] = [
    ['Contenido', note.content],
    ['Intervenciones', note.interventions],
    ['Tareas para la casa', note.homeworkTasks],
    ['Foco de la próxima sesión', note.nextFocus],
  ]
  return (
    <div className="flex flex-col gap-3">
      {fields.map(([label, value]) => (
        <div key={label}>
          <p className="mb-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">{label}</p>
          <p className="whitespace-pre-wrap text-sm text-foreground">{value?.trim() ? value : '—'}</p>
        </div>
      ))}
    </div>
  )
}
