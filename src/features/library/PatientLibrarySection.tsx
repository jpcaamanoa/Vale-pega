import { useEffect, useMemo, useState } from 'react'
import { Button } from '../../components/ui/Button'
import { TextField } from '../../components/ui/TextField'
import { libraryApi } from './api'
import { formatFileSize, type LibraryResourceSummary } from './types'

function isImage(mimeType: string | null): boolean {
  return mimeType?.startsWith('image/') ?? false
}

function LinkedResourceRow({ resource, patientId, onChanged }: { resource: LibraryResourceSummary; patientId: string; onChanged: () => void }) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [dataUrl, setDataUrl] = useState<string | null>(null)

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
      libraryApi
        .getDataUrl(resource.id)
        .then(setDataUrl)
        .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo abrir la imagen.'))
      return
    }
    void withBusy(() => libraryApi.openExternally(resource.id))
  }

  const unlink = () => withBusy(async () => {
    await libraryApi.unlinkFromPatient(resource.id, patientId)
    onChanged()
  })

  return (
    <li className="flex flex-col gap-2 border-b border-border py-3 last:border-b-0">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="font-medium text-foreground">{resource.title}</div>
          {resource.author && <div className="text-xs text-muted-foreground">{resource.author}</div>}
          {resource.originalFilename && (
            <div className="text-xs text-muted-foreground">
              {resource.originalFilename} · {resource.sizeBytes != null ? formatFileSize(resource.sizeBytes) : ''}
            </div>
          )}
          {resource.sourceUrl && (
            <a href={resource.sourceUrl} target="_blank" rel="noreferrer" className="text-xs text-accent hover:underline">
              {resource.sourceUrl}
            </a>
          )}
          {error && <p className="mt-1 text-xs text-danger">{error}</p>}
        </div>
        <div className="flex shrink-0 gap-2">
          {resource.fileDocumentId && (
            <Button type="button" variant="secondary" onClick={open} disabled={busy}>
              Abrir
            </Button>
          )}
          <Button type="button" variant="secondary" onClick={unlink} disabled={busy}>
            Desasociar
          </Button>
        </div>
      </div>
      {dataUrl && <img src={dataUrl} alt={resource.title} className="max-h-64 rounded-lg object-contain" />}
    </li>
  )
}

/**
 * Sección "Biblioteca" de la ficha del paciente — a diferencia de "Documentos" (archivos propios
 * de este paciente), aquí solo se listan/asocian/desasocian recursos globales ya existentes:
 * nunca se sube un archivo nuevo desde aquí (eso vive únicamente en la pantalla global
 * `LibraryScreen`, para no duplicar el flujo de importación). Asociar un recurso nuevo requiere
 * que el paciente no esté archivado; desasociar y consultar siguen siempre disponibles.
 */
export function PatientLibrarySection({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const [linked, setLinked] = useState<LibraryResourceSummary[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [allActive, setAllActive] = useState<LibraryResourceSummary[] | null>(null)
  const [linkError, setLinkError] = useState<string | null>(null)

  const loadLinked = () => {
    setError(null)
    libraryApi
      .listResourcesForPatient(patientId)
      .then((all) => {
        setLinked(all)
        setError(null)
      })
      .catch((err) => {
        setLinked(null)
        setError(typeof err === 'string' ? err : 'No se pudo cargar esta sección. Intenta nuevamente.')
      })
  }

  useEffect(loadLinked, [patientId])

  useEffect(() => {
    if (patientArchived) return
    libraryApi.list().then(setAllActive).catch(() => setAllActive([]))
  }, [patientArchived])

  const linkedIds = useMemo(() => new Set((linked ?? []).map((r) => r.id)), [linked])
  const candidates = useMemo(() => {
    if (!allActive || !search.trim()) return []
    const q = search.trim().toLowerCase()
    return allActive.filter((r) => !linkedIds.has(r.id) && (r.title.toLowerCase().includes(q) || r.author?.toLowerCase().includes(q)))
  }, [allActive, search, linkedIds])

  const link = async (resourceId: string) => {
    setLinkError(null)
    try {
      await libraryApi.linkToPatient(resourceId, patientId)
      setSearch('')
      loadLinked()
    } catch (err) {
      setLinkError(typeof err === 'string' ? err : 'No se pudo asociar el recurso.')
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <p className="rounded-lg border border-border bg-surface px-4 py-3 text-xs text-muted-foreground">
        Recursos de la Biblioteca global asociados a este paciente. Un recurso se sube una sola vez desde la pantalla «Biblioteca» y puede
        asociarse a cualquier número de pacientes — nunca se duplica el archivo.
      </p>

      {error && (
        <div className="flex flex-col gap-2">
          <p className="text-sm text-danger">{error}</p>
          <div>
            <Button type="button" variant="secondary" onClick={loadLinked}>
              Reintentar
            </Button>
          </div>
        </div>
      )}
      {linked === null && !error && <p className="text-sm text-muted-foreground">Cargando…</p>}
      {linked !== null && linked.length === 0 && <p className="text-sm text-muted-foreground">Sin recursos asociados todavía.</p>}
      {linked !== null && linked.length > 0 && (
        <ul className="rounded-lg border border-border px-4">
          {linked.map((r) => (
            <LinkedResourceRow key={r.id} resource={r} patientId={patientId} onChanged={loadLinked} />
          ))}
        </ul>
      )}

      {!patientArchived && (
        <div className="flex flex-col gap-2 rounded-lg border border-border p-4">
          <h3 className="text-sm font-medium text-foreground">Asociar un recurso existente</h3>
          <TextField label="Buscar en la Biblioteca" placeholder="Título o autor…" value={search} onChange={(e) => setSearch(e.target.value)} />
          {linkError && <p className="text-sm text-danger">{linkError}</p>}
          {candidates.length > 0 && (
            <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
              {candidates.map((r) => (
                <li key={r.id} className="flex items-center justify-between px-3 py-2 text-sm">
                  <span className="text-foreground">{r.title}</span>
                  <Button type="button" variant="secondary" onClick={() => link(r.id)}>
                    Asociar
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  )
}
