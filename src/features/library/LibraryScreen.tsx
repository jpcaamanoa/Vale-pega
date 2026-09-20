import { useEffect, useMemo, useState } from 'react'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { Textarea } from '../../components/ui/Textarea'
import { patientsApi } from '../patients/api'
import type { PatientListItem } from '../patients/types'
import { libraryApi } from './api'
import { formatFileSize, LIBRARY_RESOURCE_TYPES, LIBRARY_RESOURCE_TYPE_LABELS, type LibraryResourceSummary } from './types'

type ViewMode = 'active' | 'archived'
type SortMode = 'title' | 'date' | 'size'

function formatTimestampDate(iso: string): string {
  return new Date(iso).toLocaleDateString('es-CL', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

function isImage(mimeType: string | null): boolean {
  return mimeType?.startsWith('image/') ?? false
}

function matchesSearch(resource: LibraryResourceSummary, query: string): boolean {
  const q = query.trim().toLowerCase()
  if (!q) return true
  return [resource.title, resource.author, resource.originalFilename, resource.summary].some((field) => field?.toLowerCase().includes(q))
}

function sortResources(resources: LibraryResourceSummary[], sort: SortMode): LibraryResourceSummary[] {
  const sorted = [...resources]
  switch (sort) {
    case 'title':
      return sorted.sort((a, b) => a.title.localeCompare(b.title, 'es'))
    case 'date':
      return sorted.sort((a, b) => b.createdAt.localeCompare(a.createdAt))
    case 'size':
      return sorted.sort((a, b) => (b.sizeBytes ?? 0) - (a.sizeBytes ?? 0))
  }
}

/** Formulario "Agregar recurso" — el archivo es opcional (un recurso puede ser solo un enlace/
 * referencia con `sourceUrl`, sin ningún archivo adjunto). Igual que Documentos: nunca crea una
 * copia en plano, solo lee el archivo de origen y escribe ciphertext dentro del vault. */
function AddResourceModal({ onCreated, onCancel }: { onCreated: () => void; onCancel: () => void }) {
  const [title, setTitle] = useState('')
  const [resourceType, setResourceType] = useState('')
  const [author, setAuthor] = useState('')
  const [sourceUrl, setSourceUrl] = useState('')
  const [summary, setSummary] = useState('')
  const [sourcePath, setSourcePath] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const pickFile = async () => {
    const selected = await libraryApi.pickSourceFile()
    if (selected) setSourcePath(selected)
  }

  const submit = async () => {
    if (!title.trim()) {
      setError('El recurso necesita un título.')
      return
    }
    setError(null)
    setSubmitting(true)
    try {
      await libraryApi.create({
        title: title.trim(),
        resourceType: resourceType || null,
        author: author.trim() || null,
        sourceUrl: sourceUrl.trim() || null,
        summary: summary.trim() || null,
        sourcePath,
      })
      onCreated()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo crear el recurso.')
    } finally {
      setSubmitting(false)
    }
  }

  const displayName = sourcePath?.split(/[/\\]/).pop() ?? null

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="my-auto max-h-[85vh] w-full max-w-lg overflow-y-auto rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-4 text-base font-semibold text-foreground">Agregar recurso a la Biblioteca</h2>
        <div className="flex flex-col gap-4">
          <TextField label="Título *" value={title} onChange={(e) => setTitle(e.target.value)} />
          <Select label="Tipo (opcional)" value={resourceType} onChange={(e) => setResourceType(e.target.value)}>
            <option value="">Sin tipo</option>
            {LIBRARY_RESOURCE_TYPES.map((t) => (
              <option key={t} value={t}>
                {LIBRARY_RESOURCE_TYPE_LABELS[t]}
              </option>
            ))}
          </Select>
          <TextField label="Autor (opcional)" value={author} onChange={(e) => setAuthor(e.target.value)} />
          <TextField label="Enlace (opcional)" placeholder="https://…" value={sourceUrl} onChange={(e) => setSourceUrl(e.target.value)} />
          <Textarea label="Resumen (opcional)" value={summary} onChange={(e) => setSummary(e.target.value)} />
          <div className="flex flex-col gap-1.5">
            <span className="text-sm font-medium text-foreground">Archivo (opcional)</span>
            <div className="flex items-center gap-2">
              <Button type="button" variant="secondary" onClick={pickFile}>
                {displayName ? 'Cambiar archivo…' : 'Elegir archivo…'}
              </Button>
              {displayName && <span className="truncate text-sm text-muted-foreground">{displayName}</span>}
            </div>
            <p className="text-xs text-muted-foreground">Un recurso puede ser solo un enlace, solo un archivo, o ambos.</p>
          </div>
          {error && <p className="text-sm text-danger">{error}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onCancel} disabled={submitting}>
              Cancelar
            </Button>
            <Button type="button" onClick={submit} disabled={submitting}>
              {submitting ? 'Guardando…' : 'Agregar recurso'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

function EditResourceModal({ resource, onUpdated, onCancel }: { resource: LibraryResourceSummary; onUpdated: () => void; onCancel: () => void }) {
  const [title, setTitle] = useState(resource.title)
  const [resourceType, setResourceType] = useState(resource.resourceType ?? '')
  const [author, setAuthor] = useState(resource.author ?? '')
  const [sourceUrl, setSourceUrl] = useState(resource.sourceUrl ?? '')
  const [summary, setSummary] = useState(resource.summary ?? '')
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const submit = async () => {
    if (!title.trim()) {
      setError('El recurso necesita un título.')
      return
    }
    setError(null)
    setSubmitting(true)
    try {
      await libraryApi.updateMetadata(resource.id, {
        title: title.trim(),
        resourceType: resourceType || null,
        author: author.trim() || null,
        sourceUrl: sourceUrl.trim() || null,
        summary: summary.trim() || null,
      })
      onUpdated()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo editar el recurso.')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="my-auto max-h-[85vh] w-full max-w-lg overflow-y-auto rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-4 text-base font-semibold text-foreground">Editar recurso</h2>
        {resource.originalFilename && <p className="mb-4 text-sm text-muted-foreground">Archivo: {resource.originalFilename}</p>}
        <div className="flex flex-col gap-4">
          <TextField label="Título *" value={title} onChange={(e) => setTitle(e.target.value)} />
          <Select label="Tipo (opcional)" value={resourceType} onChange={(e) => setResourceType(e.target.value)}>
            <option value="">Sin tipo</option>
            {LIBRARY_RESOURCE_TYPES.map((t) => (
              <option key={t} value={t}>
                {LIBRARY_RESOURCE_TYPE_LABELS[t]}
              </option>
            ))}
          </Select>
          <TextField label="Autor (opcional)" value={author} onChange={(e) => setAuthor(e.target.value)} />
          <TextField label="Enlace (opcional)" value={sourceUrl} onChange={(e) => setSourceUrl(e.target.value)} />
          <Textarea label="Resumen (opcional)" value={summary} onChange={(e) => setSummary(e.target.value)} />
          {error && <p className="text-sm text-danger">{error}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onCancel} disabled={submitting}>
              Cancelar
            </Button>
            <Button type="button" onClick={submit} disabled={submitting}>
              {submitting ? 'Guardando…' : 'Guardar cambios'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

function ImagePreviewModal({ resource, onClose }: { resource: LibraryResourceSummary; onClose: () => void }) {
  const [dataUrl, setDataUrl] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    libraryApi
      .getDataUrl(resource.id)
      .then(setDataUrl)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo abrir la imagen.'))
  }, [resource.id])

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="flex max-h-full w-full max-w-3xl flex-col gap-4 rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <div className="flex items-center justify-between">
          <h2 className="text-base font-semibold text-foreground">{resource.title}</h2>
          <Button type="button" variant="secondary" onClick={onClose}>
            Cerrar
          </Button>
        </div>
        {error && <p className="text-sm text-danger">{error}</p>}
        {!error && !dataUrl && <p className="text-sm text-muted-foreground">Cargando…</p>}
        {dataUrl && <img src={dataUrl} alt={resource.title} className="max-h-[70vh] w-full rounded-lg object-contain" />}
      </div>
    </div>
  )
}

/** Asociar/desasociar pacientes desde el propio recurso — la misma operación también está
 * disponible desde la ficha del paciente (`PatientLibrarySection`), ambas llaman a los mismos
 * comandos idempotentes. */
function LinkedPatientsModal({ resource, onClose }: { resource: LibraryResourceSummary; onClose: () => void }) {
  const [linked, setLinked] = useState<PatientListItem[] | null>(null)
  const [search, setSearch] = useState('')
  const [candidates, setCandidates] = useState<PatientListItem[]>([])
  const [error, setError] = useState<string | null>(null)

  const loadLinked = () => {
    libraryApi
      .listPatientsForResource(resource.id)
      .then(setLinked)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudieron cargar los pacientes asociados.'))
  }

  useEffect(loadLinked, [resource.id])

  useEffect(() => {
    if (!search.trim()) {
      setCandidates([])
      return
    }
    let cancelled = false
    patientsApi.list(search).then((results) => {
      if (!cancelled) setCandidates(results)
    })
    return () => {
      cancelled = true
    }
  }, [search])

  const linkedIds = useMemo(() => new Set((linked ?? []).map((p) => p.id)), [linked])

  const link = async (patientId: string) => {
    setError(null)
    try {
      await libraryApi.linkToPatient(resource.id, patientId)
      loadLinked()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo asociar el paciente.')
    }
  }

  const unlink = async (patientId: string) => {
    setError(null)
    try {
      await libraryApi.unlinkFromPatient(resource.id, patientId)
      loadLinked()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo desasociar el paciente.')
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="my-auto max-h-[85vh] w-full max-w-lg overflow-y-auto rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold text-foreground">Pacientes asociados a «{resource.title}»</h2>
          <Button type="button" variant="secondary" onClick={onClose}>
            Cerrar
          </Button>
        </div>

        {error && <p className="mb-3 text-sm text-danger">{error}</p>}

        <div className="mb-5 flex flex-col gap-2">
          {linked === null && <p className="text-sm text-muted-foreground">Cargando…</p>}
          {linked !== null && linked.length === 0 && <p className="text-sm text-muted-foreground">Este recurso no está asociado a ningún paciente todavía.</p>}
          {linked !== null && linked.length > 0 && (
            <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
              {linked.map((p) => (
                <li key={p.id} className="flex items-center justify-between px-3 py-2 text-sm">
                  <span className="text-foreground">{p.preferredName || p.fullName}</span>
                  <Button type="button" variant="secondary" onClick={() => unlink(p.id)}>
                    Desasociar
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="flex flex-col gap-2">
          <TextField label="Asociar a un paciente" placeholder="Buscar por nombre…" value={search} onChange={(e) => setSearch(e.target.value)} />
          {candidates.length > 0 && (
            <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
              {candidates
                .filter((p) => !linkedIds.has(p.id))
                .map((p) => (
                  <li key={p.id} className="flex items-center justify-between px-3 py-2 text-sm">
                    <span className="text-foreground">{p.preferredName || p.fullName}</span>
                    <Button type="button" variant="secondary" onClick={() => link(p.id)}>
                      Asociar
                    </Button>
                  </li>
                ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  )
}

function ResourceRow({ resource, view, onChanged }: { resource: LibraryResourceSummary; view: ViewMode; onChanged: () => void }) {
  const [editing, setEditing] = useState(false)
  const [previewing, setPreviewing] = useState(false)
  const [managingPatients, setManagingPatients] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const withBusy = async (action: () => Promise<void>) => {
    setError(null)
    setBusy(true)
    try {
      await action()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo completar la acción.')
    } finally {
      setBusy(false)
    }
  }

  const open = () => {
    if (!resource.fileDocumentId) return
    if (isImage(resource.mimeType)) {
      setPreviewing(true)
      return
    }
    void withBusy(() => libraryApi.openExternally(resource.id))
  }

  const exportCopy = () =>
    withBusy(async () => {
      if (!resource.originalFilename) return
      const destination = await libraryApi.pickExportDestination(resource.originalFilename)
      if (!destination) return
      await libraryApi.exportTo(resource.id, destination)
    })

  /** Archivar avisa si el recurso sigue asociado a pacientes (el backend rechaza con un mensaje
   * que incluye cuántos) — nunca se archiva en silencio sin que la usuaria confirme. */
  const archiveOrRestore = () =>
    withBusy(async () => {
      if (view === 'archived') {
        await libraryApi.restore(resource.id)
        onChanged()
        return
      }
      try {
        await libraryApi.archive(resource.id, false)
        onChanged()
      } catch (err) {
        const message = typeof err === 'string' ? err : 'No se pudo archivar el recurso.'
        if (window.confirm(`${message}\n\n¿Archivar de todas formas? Las asociaciones con pacientes nunca se eliminan.`)) {
          await libraryApi.archive(resource.id, true)
          onChanged()
        }
      }
    })

  return (
    <tr className="border-b border-border align-top last:border-b-0">
      <td className="px-4 py-3">
        <div className="font-medium text-foreground">{resource.title}</div>
        {resource.author && <div className="text-xs text-muted-foreground">{resource.author}</div>}
        {resource.originalFilename && <div className="text-xs text-muted-foreground">{resource.originalFilename}</div>}
        {resource.sourceUrl && (
          <a href={resource.sourceUrl} target="_blank" rel="noreferrer" className="text-xs text-accent hover:underline">
            {resource.sourceUrl}
          </a>
        )}
        {error && <p className="mt-1 text-xs text-danger">{error}</p>}
      </td>
      <td className="px-4 py-3 text-muted-foreground">{resource.resourceType ? LIBRARY_RESOURCE_TYPE_LABELS[resource.resourceType] : '—'}</td>
      <td className="px-4 py-3 text-muted-foreground">{formatTimestampDate(resource.createdAt)}</td>
      <td className="px-4 py-3 text-muted-foreground">{resource.sizeBytes != null ? formatFileSize(resource.sizeBytes) : '—'}</td>
      <td className="px-4 py-3">
        <div className="flex flex-wrap justify-end gap-2">
          {resource.fileDocumentId && (
            <Button type="button" variant="secondary" onClick={open} disabled={busy}>
              Abrir
            </Button>
          )}
          {resource.fileDocumentId && (
            <Button type="button" variant="secondary" onClick={exportCopy} disabled={busy}>
              Exportar copia
            </Button>
          )}
          <Button type="button" variant="secondary" onClick={() => setManagingPatients(true)} disabled={busy}>
            Pacientes
          </Button>
          {view === 'active' && (
            <Button type="button" variant="secondary" onClick={() => setEditing(true)} disabled={busy}>
              Editar
            </Button>
          )}
          <Button type="button" variant="secondary" onClick={archiveOrRestore} disabled={busy}>
            {view === 'active' ? 'Archivar' : 'Restaurar'}
          </Button>
        </div>
      </td>
      {editing && (
        <EditResourceModal
          resource={resource}
          onUpdated={() => {
            setEditing(false)
            onChanged()
          }}
          onCancel={() => setEditing(false)}
        />
      )}
      {previewing && <ImagePreviewModal resource={resource} onClose={() => setPreviewing(false)} />}
      {managingPatients && <LinkedPatientsModal resource={resource} onClose={() => setManagingPatients(false)} />}
    </tr>
  )
}

/**
 * Pantalla global "Biblioteca" (fase de continuación post-Fase 19) — distinta de la pestaña
 * "Documentos" de cada paciente: un recurso se sube una sola vez y se asocia a cualquier número
 * de pacientes desde aquí o desde la ficha de cada paciente (`PatientLibrarySection`), sin
 * duplicar nunca el archivo físico. Búsqueda y orden se resuelven en cliente sobre la lista ya
 * cargada, mismo criterio que el resto de la aplicación (p. ej. Pacientes).
 */
export function LibraryScreen() {
  const [view, setView] = useState<ViewMode>('active')
  const [resources, setResources] = useState<LibraryResourceSummary[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const [search, setSearch] = useState('')
  const [sort, setSort] = useState<SortMode>('title')

  const load = () => {
    setError(null)
    const request = view === 'active' ? libraryApi.list() : libraryApi.listArchived()
    request.then(setResources).catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar la Biblioteca.'))
  }

  useEffect(load, [view])

  const visible = useMemo(() => {
    if (!resources) return []
    return sortResources(resources.filter((r) => matchesSearch(r, search)), sort)
  }, [resources, search, sort])

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-10">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-xl font-semibold text-foreground">Biblioteca</h1>
        {view === 'active' && <Button onClick={() => setAdding(true)}>Agregar recurso</Button>}
      </div>
      <p className="text-sm text-muted-foreground">
        Recursos generales (artículos, protocolos, escalas, enlaces) que se suben una sola vez y pueden asociarse a cualquier número de
        pacientes — distinta de los documentos propios de cada paciente.
      </p>

      <div className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex gap-1 border-b border-border">
          <button
            onClick={() => setView('active')}
            className={`px-3 py-2 text-sm font-medium transition-colors ${
              view === 'active' ? 'border-b-2 border-accent text-accent' : 'text-muted-foreground hover:text-foreground'
            }`}
          >
            Activos
          </button>
          <button
            onClick={() => setView('archived')}
            className={`px-3 py-2 text-sm font-medium transition-colors ${
              view === 'archived' ? 'border-b-2 border-accent text-accent' : 'text-muted-foreground hover:text-foreground'
            }`}
          >
            Archivados
          </button>
        </div>
        <div className="flex flex-wrap gap-2">
          <TextField label="Buscar" placeholder="Título, autor o archivo…" value={search} onChange={(e) => setSearch(e.target.value)} className="w-64" />
          <Select label="Ordenar por" value={sort} onChange={(e) => setSort(e.target.value as SortMode)} className="w-40">
            <option value="title">Título (A-Z)</option>
            <option value="date">Más recientes</option>
            <option value="size">Tamaño</option>
          </Select>
        </div>
      </div>

      {error && <p className="text-sm text-danger">{error}</p>}
      {resources === null && <p className="text-sm text-muted-foreground">Cargando…</p>}

      {resources !== null && visible.length === 0 && (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">
            {search
              ? 'Ningún recurso coincide con la búsqueda.'
              : view === 'archived'
                ? 'No hay recursos archivados.'
                : 'Sin recursos registrados todavía.'}
          </p>
          {view === 'active' && !search && <Button onClick={() => setAdding(true)}>Agregar recurso</Button>}
        </div>
      )}

      {resources !== null && visible.length > 0 && (
        <div className="overflow-x-auto rounded-lg border border-border">
          <table className="w-full text-left text-sm">
            <thead className="bg-surface text-xs uppercase tracking-wide text-muted-foreground">
              <tr>
                <th className="px-4 py-2.5 font-medium">Título</th>
                <th className="px-4 py-2.5 font-medium">Tipo</th>
                <th className="px-4 py-2.5 font-medium">Fecha</th>
                <th className="px-4 py-2.5 font-medium">Tamaño</th>
                <th className="px-4 py-2.5 font-medium text-right">Acciones</th>
              </tr>
            </thead>
            <tbody>
              {visible.map((r) => (
                <ResourceRow key={r.id} resource={r} view={view} onChanged={load} />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {adding && (
        <AddResourceModal
          onCreated={() => {
            setAdding(false)
            load()
          }}
          onCancel={() => setAdding(false)}
        />
      )}
    </div>
  )
}
