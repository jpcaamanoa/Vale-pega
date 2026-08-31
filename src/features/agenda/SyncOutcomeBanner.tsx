import type { SyncOutcome } from './types'

/**
 * Feedback visual de qué pasó al reconciliar con Google Calendar después de
 * una mutación. `not_connected` y `skipped` no son errores — no hay nada que
 * mostrarle a la usuaria en esos casos (Google simplemente no está
 * configurado, o no había nada que sincronizar), así que no se renderiza
 * nada. Un fallo real de Google **nunca** implica que la cita no se haya
 * guardado — el guardado local ya ocurrió antes de intentar sincronizar.
 */
export function SyncOutcomeBanner({ outcome }: { outcome: SyncOutcome | null }) {
  if (!outcome) return null

  switch (outcome.kind) {
    case 'not_connected':
    case 'skipped':
      return null
    case 'synced':
      return (
        <p className="rounded-lg border border-success/40 bg-success-soft px-4 py-2.5 text-sm text-success">
          Sincronizado con Google Calendar.
        </p>
      )
    case 'disconnected':
      return (
        <p className="rounded-lg border border-danger/40 bg-danger-soft px-4 py-2.5 text-sm text-danger">
          La conexión con Google Calendar expiró o fue revocada. La cita se guardó igual — ve a Ajustes para
          reconectar.
        </p>
      )
    case 'failed':
      return (
        <p className="rounded-lg border border-danger/40 bg-danger-soft px-4 py-2.5 text-sm text-danger">
          No se pudo sincronizar con Google Calendar ({outcome.message}). La cita se guardó igual — puedes
          reintentar la sincronización más tarde.
        </p>
      )
  }
}
