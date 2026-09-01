/** `session_date` es AAAA-MM-DD (fecha simple, sin hora ni zona horaria — a diferencia de las citas de Agenda). */
export function formatSessionDate(dateStr: string): string {
  const [y, m, d] = dateStr.split('-')
  return `${d}-${m}-${y}`
}
