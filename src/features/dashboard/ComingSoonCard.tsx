/**
 * Bloque "Próximamente" para partes del Dashboard cuya funcionalidad de base
 * (Agenda, Sesiones, Pagos, Documentos) todavía no existe. Nunca debe
 * mostrar un número ni un dato como si fuera real — solo el título del
 * bloque, una explicación breve de qué mostrará cuando exista, y la
 * etiqueta "Próximamente" con el tratamiento neutro/deshabilitado de los
 * design tokens (no es un error ni una advertencia, así que no usa los
 * tokens semánticos de warning/danger).
 */
export function ComingSoonCard({ title, description }: { title: string; description: string }) {
  return (
    <section className="flex flex-col gap-3 rounded-lg border border-border bg-surface p-6">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">{title}</h3>
        <span className="shrink-0 rounded-full bg-disabled px-2 py-0.5 text-xs font-medium text-disabled-foreground">
          Próximamente
        </span>
      </div>
      <p className="text-sm text-muted-foreground">{description}</p>
    </section>
  )
}
