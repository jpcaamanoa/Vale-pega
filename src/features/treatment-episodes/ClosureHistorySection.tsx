import { useEffect, useState } from 'react'
import { formatSessionDate } from '../sessions/datetime'
import { episodeClosuresApi } from './api'
import { CLOSURE_OUTCOME_LABELS, CLOSURE_REASON_LABELS, type EpisodeClosure } from './types'

function formatTimestampDate(iso: string): string {
  return new Date(iso).toLocaleDateString('es-CL', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

function HistoryField({ label, value, multiline }: { label: string; value: string; multiline?: boolean }) {
  return (
    <div>
      <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{label}</h4>
      <p className={`text-sm text-foreground ${multiline ? 'whitespace-pre-wrap' : ''}`}>{value}</p>
    </div>
  )
}

/**
 * Cierres anteriores/anulados de un proceso terapéutico (microfase de
 * cierre de deuda post-Fase 13 — `episodeClosuresApi.listHistory` ya
 * existía desde la Fase 11, sin ninguna UI que lo mostrara).
 *
 * El cierre **vigente** ya se muestra en `ClosureSection` ("Cierre del
 * proceso") — esta sección deliberadamente nunca lo repite, solo lista los
 * cierres **anulados** (`revertedAt !== null`), para no duplicar la misma
 * información en dos lugares. Si nunca hubo un cierre anulado, la sección
 * completa no se renderiza (nunca un bloque vacío).
 *
 * Solo lectura: un cierre anulado permanece visible para siempre, nunca
 * puede editarse ni eliminarse — no hay ningún botón de acción aquí, ver
 * `docs/episode-closure.md` sobre la inmutabilidad del cierre.
 */
export function ClosureHistorySection({ episodeId }: { episodeId: string }) {
  const [history, setHistory] = useState<EpisodeClosure[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    episodeClosuresApi
      .listHistory(episodeId)
      .then(setHistory)
      .catch((err) => setError(typeof err === 'string' ? err : 'No se pudo cargar el historial de cierres.'))
  }, [episodeId])

  if (error) return <p className="mb-6 text-sm text-danger">{error}</p>
  if (history === null) return null

  const revertedClosures = history.filter((c) => c.revertedAt !== null)
  if (revertedClosures.length === 0) return null

  return (
    <div className="mb-6 flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
      <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Historial de cierres</h3>
      <div className="flex flex-col gap-4">
        {revertedClosures.map((c) => (
          <div key={c.id} className="flex flex-col gap-3 rounded-lg border border-border p-4">
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium text-foreground">Cerrado el {formatSessionDate(c.closedAt)}</span>
              <span className="rounded-full bg-muted-foreground/10 px-2 py-0.5 text-xs font-medium text-muted-foreground">Cierre anulado</span>
            </div>
            <HistoryField label="Motivo" value={CLOSURE_REASON_LABELS[c.reason]} />
            {c.reason === 'otro' && c.reasonDetail && <HistoryField label="Detalle" value={c.reasonDetail} />}
            <HistoryField label="Resultado" value={CLOSURE_OUTCOME_LABELS[c.outcome]} />
            {c.summary && <HistoryField label="Resumen del proceso" value={c.summary} multiline />}
            {c.recommendations && <HistoryField label="Recomendaciones" value={c.recommendations} multiline />}
            {c.revertedAt && <HistoryField label="Anulado el" value={formatTimestampDate(c.revertedAt)} />}
            {c.revertedReason && <HistoryField label="Motivo de la anulación" value={c.revertedReason} />}
          </div>
        ))}
      </div>
    </div>
  )
}
