/**
 * El backend guarda `startsAt`/`endsAt` en UTC (ISO-8601, formato
 * `AAAA-MM-DDTHH:MM:SS(.mmm)Z`). El navegador solo sabe mostrar/editar
 * `<input type="datetime-local">` en la hora local del sistema — estas dos
 * funciones son la única conversión entre ambos mundos, para que el resto
 * de la feature nunca tenga que pensar en zonas horarias.
 */

function pad(n: number): string {
  return String(n).padStart(2, '0')
}

/** ISO-8601 UTC → valor local para un `<input type="datetime-local">`. */
export function isoToLocalInput(iso: string): string {
  const d = new Date(iso)
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** Valor local de un `<input type="datetime-local">` → ISO-8601 UTC. */
export function localInputToIso(local: string): string {
  return new Date(local).toISOString()
}

/** Para mostrar un horario en texto (listas, advertencias de solapamiento). */
export function formatLocalDateTime(iso: string): string {
  return new Date(iso).toLocaleString('es-CL', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function formatLocalTime(iso: string): string {
  return new Date(iso).toLocaleTimeString('es-CL', { hour: '2-digit', minute: '2-digit' })
}

export function formatLocalDate(iso: string): string {
  return new Date(iso).toLocaleDateString('es-CL', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

/** Medianoche local de hoy, como ISO-8601 UTC — límite inferior para "Hoy". */
export function startOfTodayIso(): string {
  const now = new Date()
  return new Date(now.getFullYear(), now.getMonth(), now.getDate(), 0, 0, 0).toISOString()
}

/** Medianoche local `daysAhead` días después de hoy, como ISO-8601 UTC. */
export function startOfDayIsoDaysFromNow(daysAhead: number): string {
  const now = new Date()
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + daysAhead, 0, 0, 0).toISOString()
}
