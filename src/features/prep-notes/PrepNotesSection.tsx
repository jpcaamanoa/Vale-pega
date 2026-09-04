import { useEffect, useState } from 'react'
import { Button } from '../../components/ui/Button'
import { Textarea } from '../../components/ui/Textarea'
import { prepNotesApi } from './api'
import type { PatientPrepNote } from './types'

/**
 * "Para próxima sesión" — preparaciones pendientes de un paciente, con
 * acción para agregar una nueva y para resolverlas (abordada/descartada).
 * Se usa tanto dentro de una sesión concreta (con `sessionId`, que queda
 * como `originSessionId` de lo que se cree aquí) como en la ficha del
 * paciente fuera de cualquier sesión (`sessionId` ausente) — regla 7 de la
 * aprobación de Fase 8: nunca depende de una cita futura agendada.
 */
export function PrepNotesSection({ patientId, sessionId, patientArchived }: { patientId: string; sessionId?: string; patientArchived: boolean }) {
  const [pending, setPending] = useState<PatientPrepNote[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const [draft, setDraft] = useState('')
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editDraft, setEditDraft] = useState('')
  const [showHistory, setShowHistory] = useState(false)
  const [history, setHistory] = useState<PatientPrepNote[] | null>(null)

  const load = () => {
    prepNotesApi
      .listPending(patientId)
      .then(setPending)
      .catch(() => setError('No se pudieron cargar las preparaciones pendientes.'))
  }

  useEffect(load, [patientId])

  const handleCreate = async () => {
    if (!draft.trim()) return
    try {
      await prepNotesApi.create({ patientId, originSessionId: sessionId ?? null, content: draft })
      setDraft('')
      setAdding(false)
      load()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar la preparación.')
    }
  }

  const handleSaveEdit = async (id: string) => {
    if (!editDraft.trim()) return
    try {
      await prepNotesApi.update(id, editDraft)
      setEditingId(null)
      load()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar la edición.')
    }
  }

  const resolve = async (id: string, status: 'abordado' | 'descartado') => {
    try {
      await prepNotesApi.setStatus(id, status)
      load()
      if (showHistory) loadHistory()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo actualizar la preparación.')
    }
  }

  const loadHistory = () => {
    prepNotesApi.list(patientId).then(setHistory)
  }

  const toggleHistory = () => {
    if (showHistory) {
      setShowHistory(false)
      return
    }
    setShowHistory(true)
    loadHistory()
  }

  const reopen = async (id: string) => {
    try {
      await prepNotesApi.setStatus(id, 'pendiente')
      load()
      loadHistory()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo reabrir la preparación.')
    }
  }

  return (
    <section className="flex flex-col gap-3 rounded-lg border border-border bg-surface p-6">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Para próxima sesión</h3>
        {!adding && !patientArchived && (
          <Button variant="secondary" onClick={() => setAdding(true)}>
            Agregar
          </Button>
        )}
      </div>

      {error && <p className="text-sm text-danger">{error}</p>}

      {adding && (
        <div className="flex flex-col gap-2 rounded-lg border border-border p-4">
          <Textarea label="Qué quieres recordar o preparar" value={draft} onChange={(e) => setDraft(e.target.value)} />
          <div className="flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={() => setAdding(false)}>
              Cancelar
            </Button>
            <Button type="button" onClick={handleCreate} disabled={!draft.trim()}>
              Guardar
            </Button>
          </div>
        </div>
      )}

      {pending === null && <p className="text-sm text-muted-foreground">Cargando…</p>}
      {pending !== null && pending.length === 0 && !adding && (
        <p className="text-sm text-muted-foreground">Nada pendiente para la próxima sesión.</p>
      )}

      {pending !== null && pending.length > 0 && (
        <ul className="flex flex-col gap-2">
          {pending.map((note) => (
            <li key={note.id} className="rounded-lg border border-border p-3">
              {editingId === note.id ? (
                <div className="flex flex-col gap-2">
                  <Textarea label="Editar" value={editDraft} onChange={(e) => setEditDraft(e.target.value)} />
                  <div className="flex justify-end gap-2">
                    <Button type="button" variant="secondary" onClick={() => setEditingId(null)}>
                      Cancelar
                    </Button>
                    <Button type="button" onClick={() => handleSaveEdit(note.id)} disabled={!editDraft.trim()}>
                      Guardar
                    </Button>
                  </div>
                </div>
              ) : (
                <>
                  <p className="whitespace-pre-wrap text-sm text-foreground">{note.content}</p>
                  <div className="mt-2 flex flex-wrap items-center gap-3 text-xs">
                    <button
                      onClick={() => {
                        setEditingId(note.id)
                        setEditDraft(note.content)
                      }}
                      className="text-accent hover:underline"
                    >
                      Editar
                    </button>
                    <button onClick={() => resolve(note.id, 'abordado')} className="text-success hover:underline">
                      ✓ Abordado
                    </button>
                    <button onClick={() => resolve(note.id, 'descartado')} className="text-muted-foreground hover:underline">
                      ✕ Descartado
                    </button>
                  </div>
                </>
              )}
            </li>
          ))}
        </ul>
      )}

      <button onClick={toggleHistory} className="self-start text-xs text-accent hover:underline">
        {showHistory ? 'Ocultar historial' : 'Ver historial'}
      </button>

      {showHistory && (
        <div className="flex flex-col gap-2 border-t border-border pt-3">
          {history === null && <p className="text-sm text-muted-foreground">Cargando…</p>}
          {history !== null && history.length === 0 && <p className="text-sm text-muted-foreground">Sin preparaciones registradas.</p>}
          {history?.map((note) => (
            <div key={note.id} className="rounded-lg border border-border p-3">
              <div className="flex items-start justify-between gap-3">
                <p className="whitespace-pre-wrap text-sm text-foreground">{note.content}</p>
                <span
                  className={`shrink-0 rounded-full px-2 py-0.5 text-xs font-medium ${
                    note.status === 'pendiente'
                      ? 'bg-warning-soft text-warning'
                      : note.status === 'abordado'
                        ? 'bg-success-soft text-success'
                        : 'bg-disabled text-disabled-foreground'
                  }`}
                >
                  {note.status === 'pendiente' ? 'Pendiente' : note.status === 'abordado' ? 'Abordado' : 'Descartado'}
                </span>
              </div>
              {note.status !== 'pendiente' && (
                <button onClick={() => reopen(note.id)} className="mt-2 text-xs text-accent hover:underline">
                  ↩ Volver a pendiente
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </section>
  )
}
