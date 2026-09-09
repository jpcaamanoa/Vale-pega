import { useEffect, useState } from 'react'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { Textarea } from '../../components/ui/Textarea'
import { documentsApi } from './api'
import { DOCUMENT_CATEGORIES, DOCUMENT_CATEGORY_LABELS, formatFileSize, type DocumentSummary } from './types'

type ViewMode = 'active' | 'archived'

function formatTimestampDate(iso: string): string {
  return new Date(iso).toLocaleDateString('es-CL', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

function isImage(mimeType: string): boolean {
  return mimeType.startsWith('image/')
}

/** Formulario "Agregar documento" — selecciona un archivo de origen ya existente en el equipo
 * (nunca crea ninguna copia en plano: Cuaderno Clínico lee ese archivo y escribe únicamente
 * ciphertext dentro del vault, Bloque 11 de la aprobación). */
function AddDocumentModal({ patientId, onCreated, onCancel }: { patientId: string; onCreated: () => void; onCancel: () => void }) {
  const [sourcePath, setSourcePath] = useState<string | null>(null)
  const [category, setCategory] = useState('')
  const [description, setDescription] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const pickFile = async () => {
    const selected = await documentsApi.pickSourceFile()
    if (selected) setSourcePath(selected)
  }

  const submit = async () => {
    if (!sourcePath) {
      setError('Selecciona un archivo primero.')
      return
    }
    setError(null)
    setSubmitting(true)
    try {
      await documentsApi.create({ patientId, sourcePath, category: category || null, description: description.trim() || null })
      onCreated()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo importar el documento.')
    } finally {
      setSubmitting(false)
    }
  }

  const displayName = sourcePath?.split(/[/\\]/).pop() ?? null

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="w-full max-w-lg rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-4 text-base font-semibold text-foreground">Agregar documento</h2>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <span className="text-sm font-medium text-foreground">Archivo</span>
            <div className="flex items-center gap-2">
              <Button type="button" variant="secondary" onClick={pickFile}>
                {displayName ? 'Cambiar archivo…' : 'Elegir archivo…'}
              </Button>
              {displayName && <span className="truncate text-sm text-muted-foreground">{displayName}</span>}
            </div>
          </div>
          <Select label="Categoría (opcional)" value={category} onChange={(e) => setCategory(e.target.value)}>
            <option value="">Sin categoría</option>
            {DOCUMENT_CATEGORIES.map((c) => (
              <option key={c} value={c}>
                {DOCUMENT_CATEGORY_LABELS[c]}
              </option>
            ))}
          </Select>
          <Textarea label="Descripción (opcional)" value={description} onChange={(e) => setDescription(e.target.value)} />
          {error && <p className="text-sm text-danger">{error}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onCancel} disabled={submitting}>
              Cancelar
            </Button>
            <Button type="button" onClick={submit} disabled={submitting}>
              {submitting ? 'Importando…' : 'Agregar documento'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

function EditMetadataModal({ document, onUpdated, onCancel }: { document: DocumentSummary; onUpdated: () => void; onCancel: () => void }) {
  const [category, setCategory] = useState(document.category ?? '')
  const [description, setDescription] = useState(document.description ?? '')
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const submit = async () => {
    setError(null)
    setSubmitting(true)
    try {
      await documentsApi.updateMetadata(document.id, { category: category || null, description: description.trim() || null })
      onUpdated()
    } catch (err) {
      setError(typeof err === 'string' ? err : 'No se pudo editar el documento.')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="w-full max-w-lg rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <h2 className="mb-4 text-base font-semibold text-foreground">Editar documento</h2>
        <p className="mb-4 text-sm text-muted-foreground">{document.originalFilename}</p>
        <div className="flex flex-col gap-4">
          <Select label="Categoría (opcional)" value={category} onChange={(e) => setCategory(e.target.value)}>
            <option value="">Sin categoría</option>
            {DOCUMENT_CATEGORIES.map((c) => (
              <option key={c} value={c}>
                {DOCUMENT_CATEGORY_LABELS[c]}
              </option>
            ))}
          </Select>
          <Textarea label="Descripción (opcional)" value={description} onChange={(e) => setDescription(e.target.value)} />
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

/** Vista de imagen en memoria — nunca escribe ningún archivo temporal en disco (Bloque 20 de la
 * aprobación, estrategia híbrida por MIME). */
function ImagePreviewModal({ document, onClose }: { document: DocumentSummary; onClose: () => void }) {
  const [dataUrl, setDataUrl] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    documentsApi
      .getDataUrl(document.id)
      .then(setDataUrl)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo abrir la imagen.'))
  }, [document.id])

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-foreground/40 px-4 py-8">
      <div className="flex max-h-full w-full max-w-3xl flex-col gap-4 rounded-2xl bg-surface-elevated p-6 shadow-lg">
        <div className="flex items-center justify-between">
          <h2 className="text-base font-semibold text-foreground">{document.originalFilename}</h2>
          <Button type="button" variant="secondary" onClick={onClose}>
            Cerrar
          </Button>
        </div>
        {error && <p className="text-sm text-danger">{error}</p>}
        {!error && !dataUrl && <p className="text-sm text-muted-foreground">Cargando…</p>}
        {dataUrl && <img src={dataUrl} alt={document.originalFilename} className="max-h-[70vh] w-full rounded-lg object-contain" />}
      </div>
    </div>
  )
}

/** Una fila de la tabla: nombre/categoría/fecha/tamaño y todas las acciones (Bloque 18/24/25/26
 * de la aprobación) — nunca muestra `storage_path` ni ningún detalle criptográfico. */
function DocumentRow({
  document,
  view,
  canEdit,
  onChanged,
}: {
  document: DocumentSummary
  view: ViewMode
  canEdit: boolean
  onChanged: () => void
}) {
  const [editing, setEditing] = useState(false)
  const [previewing, setPreviewing] = useState(false)
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
    if (isImage(document.mimeType)) {
      setPreviewing(true)
      return
    }
    void withBusy(() => documentsApi.openExternally(document.id))
  }

  const exportCopy = () =>
    withBusy(async () => {
      const destination = await documentsApi.pickExportDestination(document.originalFilename)
      if (!destination) return
      await documentsApi.exportTo(document.id, destination)
    })

  const archiveOrRestore = () =>
    withBusy(async () => {
      if (view === 'active') await documentsApi.archive(document.id)
      else await documentsApi.restore(document.id)
      onChanged()
    })

  return (
    <tr className="border-b border-border last:border-b-0 align-top">
      <td className="px-4 py-3">
        <div className="font-medium text-foreground">{document.originalFilename}</div>
        {document.description && <div className="text-xs text-muted-foreground">{document.description}</div>}
        {error && <p className="mt-1 text-xs text-danger">{error}</p>}
      </td>
      <td className="px-4 py-3 text-muted-foreground">{document.category ? DOCUMENT_CATEGORY_LABELS[document.category] : '—'}</td>
      <td className="px-4 py-3 text-muted-foreground">{formatTimestampDate(document.createdAt)}</td>
      <td className="px-4 py-3 text-muted-foreground">{formatFileSize(document.sizeBytes)}</td>
      <td className="px-4 py-3">
        <div className="flex flex-wrap justify-end gap-2">
          <Button type="button" variant="secondary" onClick={open} disabled={busy}>
            Abrir
          </Button>
          <Button type="button" variant="secondary" onClick={exportCopy} disabled={busy}>
            Exportar copia
          </Button>
          {canEdit && view === 'active' && (
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
        <EditMetadataModal
          document={document}
          onUpdated={() => {
            setEditing(false)
            onChanged()
          }}
          onCancel={() => setEditing(false)}
        />
      )}
      {previewing && <ImagePreviewModal document={document} onClose={() => setPreviewing(false)} />}
    </tr>
  )
}

/**
 * Pestaña "Documentos" de la ficha del paciente (Fase 16) — reemplaza el placeholder
 * "Próximamente". Nunca muestra `storage_path` ni ningún detalle criptográfico (Bloque 18 de la
 * aprobación). Un paciente archivado no puede recibir documentos nuevos, pero los existentes
 * siguen visibles/editables/exportables — autoridad real en `services::documents`, esto solo
 * oculta el botón de "Agregar" como refuerzo de UI.
 */
export function DocumentsTab({ patientId, patientArchived }: { patientId: string; patientArchived: boolean }) {
  const [view, setView] = useState<ViewMode>('active')
  const [documents, setDocuments] = useState<DocumentSummary[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)

  const load = () => {
    setError(null)
    const request = view === 'active' ? documentsApi.list(patientId) : documentsApi.listArchived(patientId)
    request.then(setDocuments).catch((err) => setError(typeof err === 'string' ? err : 'No se pudieron cargar los documentos.'))
  }

  useEffect(load, [patientId, view])

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
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
        {!patientArchived && view === 'active' && <Button onClick={() => setAdding(true)}>Agregar documento</Button>}
      </div>

      {error && <p className="text-sm text-danger">{error}</p>}
      {documents === null && <p className="text-sm text-muted-foreground">Cargando…</p>}

      {documents !== null && documents.length === 0 && (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">{view === 'archived' ? 'No hay documentos archivados.' : 'Sin documentos registrados todavía.'}</p>
          {view === 'active' && !patientArchived && <Button onClick={() => setAdding(true)}>Agregar documento</Button>}
        </div>
      )}

      {documents !== null && documents.length > 0 && (
        <div className="overflow-x-auto rounded-lg border border-border">
          <table className="w-full text-left text-sm">
            <thead className="bg-surface text-xs uppercase tracking-wide text-muted-foreground">
              <tr>
                <th className="px-4 py-2.5 font-medium">Nombre</th>
                <th className="px-4 py-2.5 font-medium">Categoría</th>
                <th className="px-4 py-2.5 font-medium">Fecha</th>
                <th className="px-4 py-2.5 font-medium">Tamaño</th>
                <th className="px-4 py-2.5 font-medium text-right">Acciones</th>
              </tr>
            </thead>
            <tbody>
              {documents.map((d) => (
                <DocumentRow key={d.id} document={d} view={view} canEdit onChanged={load} />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {adding && (
        <AddDocumentModal
          patientId={patientId}
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
