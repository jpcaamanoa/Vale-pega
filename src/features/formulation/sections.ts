import { FORMULATION_SECTIONS } from './types'

/**
 * Serializa las secciones editadas en el formulario a un único
 * `summaryText` con encabezados Markdown simples — el backend lo trata
 * como texto libre, nunca interpreta su estructura (ver `docs/formulation.md`).
 * Una sección vacía se omite por completo, para no llenar el texto de
 * encabezados sin contenido.
 */
export function serializeSections(sections: Record<string, string>): string {
  return FORMULATION_SECTIONS.map(({ key, label }) => ({ label, value: (sections[key] ?? '').trim() }))
    .filter(({ value }) => value !== '')
    .map(({ label, value }) => `## ${label}\n\n${value}`)
    .join('\n\n')
}

/**
 * Vuelve a separar un `summaryText` guardado en secciones editables. Si el
 * texto no sigue el formato de encabezados esperado (por ejemplo, contenido
 * legado o escrito a mano fuera de la app), todo el texto se coloca en la
 * sección "Síntesis del caso" sin perder ni un carácter — nunca se
 * descarta contenido por no calzar con el formato.
 */
export function parseSections(summaryText: string | null): Record<string, string> {
  const sections: Record<string, string> = Object.fromEntries(FORMULATION_SECTIONS.map((s) => [s.key, '']))
  if (!summaryText || summaryText.trim() === '') return sections

  const labelToKey = new Map(FORMULATION_SECTIONS.map((s) => [s.label, s.key]))
  const blocks = summaryText.split(/\n(?=## )/)
  for (const block of blocks) {
    const match = block.match(/^## (.+?)\n\n?([\s\S]*)$/)
    const key = match ? labelToKey.get(match[1].trim()) : undefined
    if (match && key) {
      sections[key] = match[2].trim()
    } else if (block.trim() !== '') {
      // Contenido que no calza con el formato de encabezados esperado
      // (legado, o escrito fuera de la app) — nunca se descarta.
      sections.sintesis = [sections.sintesis, block.trim()].filter(Boolean).join('\n\n')
    }
  }
  return sections
}
