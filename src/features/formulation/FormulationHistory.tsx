import { useEffect, useState } from 'react'
import { Button } from '../../components/ui/Button'
import { formulationsApi } from './api'
import { parseSections } from './sections'
import { FORMULATION_SECTIONS, type FormulationVersion } from './types'

function formatTimestampDate(iso: string): string {
  return new Date(iso).toLocaleDateString('es-CL', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

/** Vista de solo lectura de una versión — nunca editable, nunca se sobrescribe. */
function VersionContent({ version }: { version: FormulationVersion }) {
  const sections = parseSections(version.summaryText)
  const hasAnyContent = FORMULATION_SECTIONS.some((s) => sections[s.key]?.trim())

  if (!hasAnyContent) {
    return <p className="text-sm text-muted-foreground">Esta versión no tiene contenido registrado.</p>
  }

  return (
    <div className="flex flex-col gap-4">
      {FORMULATION_SECTIONS.filter((s) => sections[s.key]?.trim()).map((s) => (
        <div key={s.key}>
          <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{s.label}</h4>
          <p className="whitespace-pre-wrap text-sm text-foreground">{sections[s.key]}</p>
        </div>
      ))}
    </div>
  )
}

/**
 * Historial completo de versiones de una formulación (Fase 15) — no se
 * repite la deuda de Historial de cierres (Fase 11/14): esta UI existe
 * desde el primer momento. Todas las versiones son de solo lectura; nunca
 * hay un botón de edición ni de eliminación aquí.
 */
export function FormulationHistory({ formulationId, onBack }: { formulationId: string; onBack: () => void }) {
  const [versions, setVersions] = useState<FormulationVersion[] | null>(null)
  const [opened, setOpened] = useState<FormulationVersion | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    formulationsApi
      .listVersions(formulationId)
      .then(setVersions)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el historial de la formulación.'))
  }, [formulationId])

  if (error) return <p className="text-sm text-danger">{error}</p>
  if (versions === null) return <p className="text-sm text-muted-foreground">Cargando…</p>

  if (opened) {
    return (
      <div className="flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-foreground">Versión {opened.versionNumber} — {formatTimestampDate(opened.createdAt)}</h3>
          <Button variant="secondary" onClick={() => setOpened(null)}>
            Volver al historial
          </Button>
        </div>
        <VersionContent version={opened} />
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-foreground">Historial de versiones</h3>
        <Button variant="secondary" onClick={onBack}>
          Volver
        </Button>
      </div>
      {versions.length === 0 ? (
        <p className="text-sm text-muted-foreground">Esta formulación todavía no tiene versiones.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
          {versions.map((v) => (
            <li key={v.id} className="flex items-center justify-between px-4 py-3 text-sm">
              <span className="text-foreground">Versión {v.versionNumber}</span>
              <span className="text-xs text-muted-foreground">{formatTimestampDate(v.createdAt)}</span>
              <Button variant="secondary" onClick={() => setOpened(v)}>
                Abrir
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
