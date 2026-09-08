import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { formatSessionDate } from '../sessions/datetime'
import type { TreatmentEpisode } from '../treatment-episodes/types'
import { treatmentEpisodesApi } from '../treatment-episodes/api'
import { assessmentsApi } from './api'
import { administrationFormSchema, instrumentFormSchema, type AdministrationFormValues, type InstrumentFormValues } from './schema'
import {
  ASSESSMENT_CONTEXT_LABELS,
  type AdministrationInput,
  type AdministrationUpdateInput,
  type AssessmentAdministration,
  type AssessmentAdministrationSummary,
  type AssessmentContext,
  type AssessmentInstrument,
  type InstrumentInput,
} from './types'

function episodeLabel(e: TreatmentEpisode): string {
  const statusLabel = e.status === 'activo' ? 'activo' : e.status === 'pausado' ? 'pausado' : 'cerrado'
  return `${formatSessionDate(e.startedAt)} (${statusLabel})`
}

/** Nunca un `<form>` aquí: se renderiza dentro del `<form>` de AdministrationFormModal en el flujo "nuevo instrumento desde el formulario de evaluación" — mismo criterio que `ContactFormModal` en `safety-plan/SafetyPlanTab.tsx`. */
function InstrumentFormModal({ initial, onSaved, onCancel }: { initial?: AssessmentInstrument; onSaved: (input: InstrumentInput) => Promise<void>; onCancel: () => void }) {
  const [error, setError] = useState<string | null>(null)
  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<InstrumentFormValues>({
    resolver: zodResolver(instrumentFormSchema),
    defaultValues: {
      name: initial?.name ?? '',
      abbreviation: initial?.abbreviation ?? '',
      description: initial?.description ?? '',
      category: initial?.category ?? '',
    },
  })

  const submit = async (values: InstrumentFormValues) => {
    setError(null)
    try {
      await onSaved({
        name: values.name,
        abbreviation: values.abbreviation || null,
        description: values.description || null,
        category: values.category || null,
      })
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar el instrumento.')
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
      <div className="w-full max-w-md rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-4 text-base font-semibold text-foreground">{initial ? 'Editar instrumento' : 'Nuevo instrumento del catálogo'}</h2>
        <p className="mb-4 text-xs text-muted-foreground">
          Solo metadatos: nombre, abreviatura, descripción y categoría. Cuaderno Clínico nunca almacena ni
          reproduce el contenido real de un instrumento (ítems, preguntas, opciones ni tablas de normas).
        </p>
        <div className="flex flex-col gap-4">
          <TextField label="Nombre" {...register('name')} error={errors.name?.message} />
          <TextField label="Abreviatura (opcional)" {...register('abbreviation')} error={errors.abbreviation?.message} />
          <TextField label="Categoría (opcional)" {...register('category')} error={errors.category?.message} placeholder="Ej. Depresión, Ansiedad, Trauma…" />
          <Textarea label="Descripción (opcional)" {...register('description')} error={errors.description?.message} />
          {error && <p className="text-sm text-danger">{error}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onCancel} disabled={isSubmitting}>
              Cancelar
            </Button>
            <Button type="button" onClick={handleSubmit(submit)} disabled={isSubmitting}>
              {isSubmitting ? 'Guardando…' : 'Guardar instrumento'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

/** Catálogo de instrumentos — compartido entre todos los pacientes, nunca filtrado por paciente. */
function InstrumentCatalogModal({ instruments, onChanged, onClose }: { instruments: AssessmentInstrument[]; onChanged: () => void; onClose: () => void }) {
  const [showAdd, setShowAdd] = useState(false)
  const [editing, setEditing] = useState<AssessmentInstrument | null>(null)
  const [error, setError] = useState<string | null>(null)

  const handleCreate = async (input: InstrumentInput) => {
    await assessmentsApi.createInstrument(input)
    setShowAdd(false)
    onChanged()
  }

  const handleUpdate = async (input: InstrumentInput) => {
    if (!editing) return
    await assessmentsApi.updateInstrument(editing.id, input)
    setEditing(null)
    onChanged()
  }

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-foreground/40 px-4">
      <div className="w-full max-w-lg rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold text-foreground">Catálogo de instrumentos</h2>
          <Button type="button" variant="secondary" onClick={() => setShowAdd(true)}>
            Nuevo instrumento
          </Button>
        </div>
        {error && <p className="mb-3 text-sm text-danger">{error}</p>}
        {instruments.length === 0 ? (
          <p className="text-sm text-muted-foreground">No hay instrumentos registrados todavía.</p>
        ) : (
          <ul className="flex max-h-96 flex-col divide-y divide-border overflow-y-auto">
            {instruments.map((i) => (
              <li key={i.id} className="flex items-center justify-between gap-3 py-2 text-sm">
                <div>
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-foreground">{i.name}</span>
                    {i.abbreviation && <span className="text-xs text-muted-foreground">({i.abbreviation})</span>}
                  </div>
                  {i.category && <span className="text-xs text-muted-foreground">{i.category}</span>}
                </div>
                <Button type="button" variant="secondary" onClick={() => setEditing(i)}>
                  Editar
                </Button>
              </li>
            ))}
          </ul>
        )}
        <div className="mt-4 flex justify-end">
          <Button type="button" variant="secondary" onClick={onClose}>
            Cerrar
          </Button>
        </div>
      </div>
      {showAdd && (
        <InstrumentFormModal
          onSaved={async (input) => {
            try {
              await handleCreate(input)
            } catch (err) {
              setError(typeof err === 'string' ? err : 'No se pudo crear el instrumento.')
              throw err
            }
          }}
          onCancel={() => setShowAdd(false)}
        />
      )}
      {editing && (
        <InstrumentFormModal
          initial={editing}
          onSaved={async (input) => {
            try {
              await handleUpdate(input)
            } catch (err) {
              setError(typeof err === 'string' ? err : 'No se pudo editar el instrumento.')
              throw err
            }
          }}
          onCancel={() => setEditing(null)}
        />
      )}
    </div>
  )
}

function AdministrationFormModal({
  patientId,
  instruments,
  episodes,
  initial,
  onSaved,
  onCancel,
  onCatalogChanged,
}: {
  patientId: string
  instruments: AssessmentInstrument[]
  episodes: TreatmentEpisode[]
  initial?: AssessmentAdministration
  onSaved: (input: AdministrationInput | AdministrationUpdateInput) => Promise<void>
  onCancel: () => void
  onCatalogChanged: () => void
}) {
  const [error, setError] = useState<string | null>(null)
  const [showNewInstrument, setShowNewInstrument] = useState(false)
  const {
    register,
    handleSubmit,
    setValue,
    formState: { errors, isSubmitting },
  } = useForm<AdministrationFormValues>({
    resolver: zodResolver(administrationFormSchema),
    defaultValues: {
      instrumentId: initial?.instrumentId ?? '',
      episodeId: initial?.episodeId ?? '',
      administeredAt: initial?.administeredAt ?? '',
      context: initial?.context ?? '',
      totalScore: initial?.totalScore != null ? String(initial.totalScore) : '',
      subscaleScores: initial?.subscaleScores ?? '',
      interpretationText: initial?.interpretationText ?? '',
    },
  })

  const submit = async (values: AdministrationFormValues) => {
    setError(null)
    const shared = {
      episodeId: values.episodeId || null,
      administeredAt: values.administeredAt,
      context: (values.context || null) as AssessmentContext | null,
      totalScore: values.totalScore && values.totalScore.trim() !== '' ? Number(values.totalScore) : null,
      subscaleScores: values.subscaleScores || null,
      interpretationText: values.interpretationText || null,
    }
    try {
      if (initial) {
        await onSaved(shared)
      } else {
        await onSaved({ patientId, instrumentId: values.instrumentId, ...shared })
      }
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo guardar la evaluación.')
    }
  }

  return (
    <div className="fixed inset-0 z-30 flex items-center justify-center bg-foreground/40 px-4">
      <div className="w-full max-w-lg rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-4 text-base font-semibold text-foreground">{initial ? 'Editar evaluación' : 'Registrar evaluación'}</h2>
        <div className="flex flex-col gap-4">
          {!initial && (
            <div className="flex items-end gap-2">
              <div className="flex-1">
                <Select label="Instrumento" {...register('instrumentId')} error={errors.instrumentId?.message}>
                  <option value="">Selecciona un instrumento…</option>
                  {instruments.map((i) => (
                    <option key={i.id} value={i.id}>
                      {i.abbreviation ? `${i.name} (${i.abbreviation})` : i.name}
                    </option>
                  ))}
                </Select>
              </div>
              <Button type="button" variant="secondary" onClick={() => setShowNewInstrument(true)}>
                Nuevo
              </Button>
            </div>
          )}
          <Select label="Proceso vinculado (opcional)" {...register('episodeId')} error={errors.episodeId?.message}>
            <option value="">Sin proceso vinculado</option>
            {episodes.map((e) => (
              <option key={e.id} value={e.id}>
                {episodeLabel(e)}
              </option>
            ))}
          </Select>
          <TextField label="Fecha de administración" type="date" {...register('administeredAt')} error={errors.administeredAt?.message} />
          <Select label="Contexto (opcional)" {...register('context')} error={errors.context?.message}>
            <option value="">Sin especificar</option>
            {Object.entries(ASSESSMENT_CONTEXT_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </Select>
          <TextField label="Puntaje total (opcional)" type="number" step="any" {...register('totalScore')} error={errors.totalScore?.message} />
          <Textarea
            label="Subescalas (opcional, JSON libre — ej. {&quot;cognitivo&quot;: 10})"
            {...register('subscaleScores')}
            error={errors.subscaleScores?.message}
          />
          <Textarea label="Interpretación redactada por la profesional (opcional)" {...register('interpretationText')} error={errors.interpretationText?.message} />
          {error && <p className="text-sm text-danger">{error}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onCancel} disabled={isSubmitting}>
              Cancelar
            </Button>
            <Button type="button" onClick={handleSubmit(submit)} disabled={isSubmitting}>
              {isSubmitting ? 'Guardando…' : 'Guardar evaluación'}
            </Button>
          </div>
        </div>
      </div>
      {showNewInstrument && (
        <InstrumentFormModal
          onSaved={async (input) => {
            const created = await assessmentsApi.createInstrument(input)
            onCatalogChanged()
            setValue('instrumentId', created.id)
            setShowNewInstrument(false)
          }}
          onCancel={() => setShowNewInstrument(false)}
        />
      )}
    </div>
  )
}

/** Evolución longitudinal de un mismo instrumento — nunca mezcla instrumentos distintos. Tabla simple (decisión conservadora de dependencias, ver `docs/assessments.md`): sin librería de gráficos. */
function EvolutionView({ patientId, instrumentId, instrumentName, onBack }: { patientId: string; instrumentId: string; instrumentName: string; onBack: () => void }) {
  const [rows, setRows] = useState<AssessmentAdministrationSummary[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    assessmentsApi
      .listAdministrationsForInstrument(patientId, instrumentId)
      .then(setRows)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar la evolución.'))
  }, [patientId, instrumentId])

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-foreground">Evolución — {instrumentName}</h3>
        <Button variant="secondary" onClick={onBack}>
          Volver
        </Button>
      </div>
      {error && <p className="text-sm text-danger">{error}</p>}
      {rows === null ? (
        <p className="text-sm text-muted-foreground">Cargando…</p>
      ) : rows.length === 0 ? (
        <p className="text-sm text-muted-foreground">No hay administraciones de este instrumento todavía.</p>
      ) : (
        <div className="overflow-x-auto rounded-lg border border-border">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-surface text-left text-xs uppercase tracking-wide text-muted-foreground">
                <th className="px-4 py-2 font-medium">Fecha</th>
                <th className="px-4 py-2 font-medium">Contexto</th>
                <th className="px-4 py-2 font-medium">Puntaje total</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.id} className="border-b border-border last:border-b-0">
                  <td className="px-4 py-2">{formatSessionDate(r.administeredAt)}</td>
                  <td className="px-4 py-2">{r.context ? ASSESSMENT_CONTEXT_LABELS[r.context] : '—'}</td>
                  <td className="px-4 py-2">{r.totalScore ?? '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}

export function AssessmentsTab({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const [administrations, setAdministrations] = useState<AssessmentAdministrationSummary[] | null>(null)
  const [instruments, setInstruments] = useState<AssessmentInstrument[]>([])
  const [episodes, setEpisodes] = useState<TreatmentEpisode[]>([])
  const [error, setError] = useState<string | null>(null)
  const [showArchived, setShowArchived] = useState(false)
  const [showForm, setShowForm] = useState(false)
  const [showCatalog, setShowCatalog] = useState(false)
  const [editing, setEditing] = useState<AssessmentAdministration | null>(null)
  const [evolutionFor, setEvolutionFor] = useState<{ instrumentId: string; instrumentName: string } | null>(null)

  const loadInstruments = () => {
    assessmentsApi.listInstruments().then(setInstruments)
  }

  const loadAdministrations = () => {
    setError(null)
    const fetcher = showArchived ? assessmentsApi.listArchivedAdministrations : assessmentsApi.listAdministrations
    fetcher(patientId)
      .then(setAdministrations)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudieron cargar las evaluaciones.'))
  }

  useEffect(loadAdministrations, [patientId, showArchived])
  useEffect(loadInstruments, [])
  useEffect(() => {
    treatmentEpisodesApi.list(patientId).then(setEpisodes)
  }, [patientId])

  const handleCreate = async (input: AdministrationInput | AdministrationUpdateInput) => {
    await assessmentsApi.createAdministration(input as AdministrationInput)
    setShowForm(false)
    loadAdministrations()
  }

  const handleUpdate = async (input: AdministrationInput | AdministrationUpdateInput) => {
    if (!editing) return
    await assessmentsApi.updateAdministration(editing.id, input as AdministrationUpdateInput)
    setEditing(null)
    loadAdministrations()
  }

  const handleArchive = async (id: string) => {
    try {
      await assessmentsApi.archiveAdministration(id)
      loadAdministrations()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo archivar la evaluación.')
    }
  }

  const handleRestore = async (id: string) => {
    try {
      await assessmentsApi.restoreAdministration(id)
      loadAdministrations()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo restaurar la evaluación.')
    }
  }

  const openEdit = async (id: string) => {
    try {
      const full = await assessmentsApi.getAdministration(id)
      setEditing(full)
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo abrir la evaluación.')
    }
  }

  if (evolutionFor) {
    return (
      <EvolutionView
        patientId={patientId}
        instrumentId={evolutionFor.instrumentId}
        instrumentName={evolutionFor.instrumentName}
        onBack={() => setEvolutionFor(null)}
      />
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <div className="flex gap-2">
          <Button variant={showArchived ? 'secondary' : 'primary'} onClick={() => setShowArchived(false)}>
            Activas
          </Button>
          <Button variant={showArchived ? 'primary' : 'secondary'} onClick={() => setShowArchived(true)}>
            Archivadas
          </Button>
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => setShowCatalog(true)}>
            Catálogo de instrumentos
          </Button>
          {!showArchived && (
            <Button onClick={() => setShowForm(true)} disabled={patientArchived}>
              Registrar evaluación
            </Button>
          )}
        </div>
      </div>

      {patientArchived && !showArchived && (
        <p className="text-sm text-muted-foreground">
          Este paciente está archivado: puedes consultar y corregir evaluaciones ya registradas, pero no
          registrar evaluaciones nuevas.
        </p>
      )}

      {error && <p className="text-sm text-danger">{error}</p>}

      {administrations === null ? (
        <p className="text-sm text-muted-foreground">Cargando…</p>
      ) : administrations.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {showArchived ? 'No hay evaluaciones archivadas.' : 'No hay evaluaciones registradas todavía.'}
        </p>
      ) : (
        <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
          {administrations.map((a) => (
            <li key={a.id} className="flex items-center justify-between gap-3 px-4 py-3 text-sm">
              <div>
                <div className="flex items-center gap-2">
                  <span className="font-medium text-foreground">
                    {a.instrumentAbbreviation ? `${a.instrumentName} (${a.instrumentAbbreviation})` : a.instrumentName}
                  </span>
                  {a.context && <span className="text-xs text-muted-foreground">{ASSESSMENT_CONTEXT_LABELS[a.context]}</span>}
                </div>
                <span className="text-xs text-muted-foreground">
                  {formatSessionDate(a.administeredAt)}
                  {a.totalScore != null && ` · Puntaje total: ${a.totalScore}`}
                </span>
              </div>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="secondary"
                  onClick={() => setEvolutionFor({ instrumentId: a.instrumentId, instrumentName: a.instrumentName })}
                >
                  Ver evolución
                </Button>
                {showArchived ? (
                  <Button type="button" variant="secondary" onClick={() => handleRestore(a.id)}>
                    Restaurar
                  </Button>
                ) : (
                  <>
                    <Button type="button" variant="secondary" onClick={() => openEdit(a.id)}>
                      Editar
                    </Button>
                    <Button type="button" variant="secondary" onClick={() => handleArchive(a.id)}>
                      Archivar
                    </Button>
                  </>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}

      {showForm && (
        <AdministrationFormModal
          patientId={patientId}
          instruments={instruments}
          episodes={episodes}
          onSaved={handleCreate}
          onCancel={() => setShowForm(false)}
          onCatalogChanged={loadInstruments}
        />
      )}
      {editing && (
        <AdministrationFormModal
          patientId={patientId}
          instruments={instruments}
          episodes={episodes}
          initial={editing}
          onSaved={handleUpdate}
          onCancel={() => setEditing(null)}
          onCatalogChanged={loadInstruments}
        />
      )}
      {showCatalog && <InstrumentCatalogModal instruments={instruments} onChanged={loadInstruments} onClose={() => setShowCatalog(false)} />}
    </div>
  )
}
