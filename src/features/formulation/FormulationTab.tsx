import { useEffect, useState } from 'react'
import { Button } from '../../components/ui/Button'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { formatSessionDate } from '../sessions/datetime'
import { treatmentEpisodesApi } from '../treatment-episodes/api'
import { TREATMENT_EPISODE_STATUS_LABELS, type TreatmentEpisode } from '../treatment-episodes/types'
import { formulationsApi } from './api'
import { FormulationHistory } from './FormulationHistory'
import { formulationCreateSchema } from './schema'
import { parseSections, serializeSections } from './sections'
import { FORMULATION_MODEL_SUGGESTIONS, FORMULATION_SECTIONS, type CaseFormulation, type FormulationSummary, type FormulationVersion } from './types'

function formatTimestampDate(iso: string): string {
  return new Date(iso).toLocaleDateString('es-CL', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

function SectionsFields({ sections, onChange }: { sections: Record<string, string>; onChange: (key: string, value: string) => void }) {
  return (
    <div className="flex flex-col gap-4">
      {FORMULATION_SECTIONS.map((s) => (
        <Textarea key={s.key} label={`${s.label} (opcional)`} value={sections[s.key] ?? ''} onChange={(e) => onChange(s.key, e.target.value)} />
      ))}
    </div>
  )
}

function emptySections(): Record<string, string> {
  return Object.fromEntries(FORMULATION_SECTIONS.map((s) => [s.key, '']))
}

/** Crea la formulación principal del proceso actual, con su primera versión. */
function CreateFormulationModal({ patientId, episodeId, onCreated, onCancel }: { patientId: string; episodeId: string; onCreated: (formulation: CaseFormulation) => void; onCancel: () => void }) {
  const [title, setTitle] = useState('')
  const [modelType, setModelType] = useState('')
  const [sections, setSections] = useState<Record<string, string>>(emptySections())
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const submit = async () => {
    setError(null)
    const parsed = formulationCreateSchema.safeParse({ title, modelType })
    if (!parsed.success) {
      setError(parsed.error.issues[0]?.message ?? 'Datos inválidos.')
      return
    }
    setSubmitting(true)
    try {
      const [formulation] = await formulationsApi.create({
        patientId,
        episodeId,
        title: parsed.data.title,
        modelType: parsed.data.modelType || null,
        summaryText: serializeSections(sections) || null,
      })
      onCreated(formulation)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo crear la formulación.')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="w-full max-w-2xl rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-2 text-base font-semibold text-foreground">Nueva formulación</h2>
        <p className="mb-4 text-sm text-muted-foreground">
          Una hipótesis clínica de trabajo escrita por ti, que puede cambiar con nueva información. Ninguna sección es obligatoria.
        </p>
        <div className="flex flex-col gap-4">
          <TextField label="Título" value={title} onChange={(e) => setTitle(e.target.value)} />
          <TextField label="Modelo teórico (opcional)" value={modelType} onChange={(e) => setModelType(e.target.value)} list="formulation-model-suggestions" placeholder="Ej. TCC, 5P, transdiagnóstico…" />
          <datalist id="formulation-model-suggestions">
            {FORMULATION_MODEL_SUGGESTIONS.map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
          <SectionsFields sections={sections} onChange={(key, value) => setSections((s) => ({ ...s, [key]: value }))} />
          {error && <p className="text-sm text-danger">{error}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onCancel} disabled={submitting}>
              Cancelar
            </Button>
            <Button type="button" onClick={submit} disabled={submitting}>
              {submitting ? 'Creando…' : 'Crear formulación'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

/** "Actualizar formulación": precarga el contenido de la versión actual y crea una versión nueva al guardar — la anterior nunca se toca. */
function UpdateFormulationModal({ formulationId, currentVersion, onUpdated, onCancel }: { formulationId: string; currentVersion: FormulationVersion; onUpdated: () => void; onCancel: () => void }) {
  const [sections, setSections] = useState<Record<string, string>>(parseSections(currentVersion.summaryText))
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const submit = async () => {
    setError(null)
    setSubmitting(true)
    try {
      await formulationsApi.createVersion(formulationId, { summaryText: serializeSections(sections) || null })
      onUpdated()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo actualizar la formulación.')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="w-full max-w-2xl rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-2 text-base font-semibold text-foreground">Actualizar formulación</h2>
        <p className="mb-4 text-sm text-muted-foreground">
          Se precargó el contenido de la versión {currentVersion.versionNumber}. Guardar crea la versión {currentVersion.versionNumber + 1} — la anterior queda intacta en el historial.
        </p>
        <div className="flex flex-col gap-4">
          <SectionsFields sections={sections} onChange={(key, value) => setSections((s) => ({ ...s, [key]: value }))} />
          {error && <p className="text-sm text-danger">{error}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onCancel} disabled={submitting}>
              Cancelar
            </Button>
            <Button type="button" onClick={submit} disabled={submitting}>
              {submitting ? 'Guardando…' : 'Guardar como versión nueva'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

function FormulationContent({ version }: { version: FormulationVersion }) {
  const sections = parseSections(version.summaryText)
  const visible = FORMULATION_SECTIONS.filter((s) => sections[s.key]?.trim())
  if (visible.length === 0) {
    return <p className="text-sm text-muted-foreground">Esta versión no tiene contenido registrado todavía.</p>
  }
  return (
    <div className="flex flex-col gap-4">
      {visible.map((s) => (
        <div key={s.key}>
          <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{s.label}</h4>
          <p className="whitespace-pre-wrap text-sm text-foreground">{sections[s.key]}</p>
        </div>
      ))}
    </div>
  )
}

/** Una formulación completa: metadatos, contenido de la versión actual, y las acciones de actualizar/ver historial. */
function FormulationCard({ formulation, episode, canUpdate, patientArchived }: { formulation: FormulationSummary; episode: TreatmentEpisode | undefined; canUpdate: boolean; patientArchived: boolean }) {
  const [currentVersion, setCurrentVersion] = useState<FormulationVersion | null>(null)
  const [view, setView] = useState<'content' | 'update' | 'history'>('content')
  const [error, setError] = useState<string | null>(null)

  const load = () => {
    formulationsApi
      .getCurrentVersion(formulation.id)
      .then(setCurrentVersion)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el contenido de la formulación.'))
  }

  useEffect(load, [formulation.id, formulation.currentVersion])

  if (error) return <p className="text-sm text-danger">{error}</p>

  if (view === 'history') {
    return (
      <div className="rounded-lg border border-border bg-surface p-6">
        <FormulationHistory formulationId={formulation.id} onBack={() => setView('content')} />
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-5 rounded-lg border border-border bg-surface p-6">
      <div className="flex items-start justify-between">
        <div>
          <h3 className="text-sm font-semibold text-foreground">{formulation.title}</h3>
          <p className="text-xs text-muted-foreground">
            {formulation.modelType ? `${formulation.modelType} · ` : ''}
            Versión {formulation.currentVersion} · Actualizada el {formatTimestampDate(formulation.updatedAt)}
            {episode && ` · Proceso iniciado el ${formatSessionDate(episode.startedAt)} (${TREATMENT_EPISODE_STATUS_LABELS[episode.status]})`}
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => setView('history')}>
            Ver historial
          </Button>
          {canUpdate && !patientArchived && (
            <Button onClick={() => setView('update')}>Actualizar formulación</Button>
          )}
        </div>
      </div>

      {currentVersion === null ? <p className="text-sm text-muted-foreground">Cargando…</p> : <FormulationContent version={currentVersion} />}

      {view === 'update' && currentVersion && (
        <UpdateFormulationModal
          formulationId={formulation.id}
          currentVersion={currentVersion}
          onUpdated={() => {
            setView('content')
            load()
          }}
          onCancel={() => setView('content')}
        />
      )}
    </div>
  )
}

/**
 * Pestaña "Formulación" de la ficha del paciente (Fase 15). Distingue
 * claramente la formulación del **proceso actual** (el proceso con estado
 * `activo`, mismo criterio ya usado por `ProcessesTab`) de las
 * formulaciones de **procesos anteriores** — nunca duplica el listado de
 * procesos en sí (eso vive en la pestaña "Procesos"), solo lista sus
 * formulaciones. La editabilidad real siempre depende del backend
 * (`services::formulations`), nunca solo de esta agrupación visual: una
 * formulación de un proceso pausado (no "el actual") sigue ofreciendo
 * "Actualizar formulación" si el backend lo permite.
 */
export function FormulationTab({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const [episodes, setEpisodes] = useState<TreatmentEpisode[] | null>(null)
  const [formulations, setFormulations] = useState<FormulationSummary[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)

  const load = () => {
    setError(null)
    Promise.all([treatmentEpisodesApi.list(patientId), formulationsApi.list(patientId)])
      .then(([eps, forms]) => {
        setEpisodes(eps)
        setFormulations(forms)
      })
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar la formulación.'))
  }

  useEffect(load, [patientId])

  if (error) return <p className="text-sm text-danger">{error}</p>
  if (episodes === null || formulations === null) return <p className="text-sm text-muted-foreground">Cargando…</p>

  const activeEpisode = episodes.find((e) => e.status === 'activo')
  const episodeById = new Map(episodes.map((e) => [e.id, e]))
  const currentFormulation = activeEpisode ? formulations.find((f) => f.episodeId === activeEpisode.id) : undefined
  const otherFormulations = formulations.filter((f) => f.id !== currentFormulation?.id)

  return (
    <div className="flex flex-col gap-6">
      <section>
        <h3 className="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">Formulación del proceso actual</h3>
        {!activeEpisode ? (
          <p className="text-sm text-muted-foreground">No hay un proceso activo. Inicia un proceso desde la pestaña "Procesos" para crear una formulación.</p>
        ) : currentFormulation ? (
          <FormulationCard formulation={currentFormulation} episode={activeEpisode} canUpdate patientArchived={patientArchived} />
        ) : (
          <div className="flex flex-col items-center gap-3 rounded-lg border border-border py-16 text-center">
            <p className="text-sm text-muted-foreground">Este proceso todavía no tiene una formulación registrada.</p>
            {!patientArchived && <Button onClick={() => setCreating(true)}>Crear formulación</Button>}
          </div>
        )}
      </section>

      {otherFormulations.length > 0 && (
        <section>
          <h3 className="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">Formulaciones de otros procesos</h3>
          <div className="flex flex-col gap-4">
            {otherFormulations.map((f) => (
              <FormulationCard key={f.id} formulation={f} episode={f.episodeId ? episodeById.get(f.episodeId) : undefined} canUpdate patientArchived={patientArchived} />
            ))}
          </div>
        </section>
      )}

      {creating && activeEpisode && (
        <CreateFormulationModal
          patientId={patientId}
          episodeId={activeEpisode.id}
          onCreated={() => {
            setCreating(false)
            load()
          }}
          onCancel={() => setCreating(false)}
        />
      )}
    </div>
  )
}
