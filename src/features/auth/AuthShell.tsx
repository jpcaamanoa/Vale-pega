import type { ReactNode } from 'react'

export function AuthShell({ title, subtitle, children }: { title: string; subtitle?: string; children: ReactNode }) {
  return (
    <main className="flex min-h-screen items-center justify-center bg-background px-4">
      <div className="w-full max-w-sm rounded-2xl border border-border bg-surface-elevated p-8 shadow-sm">
        <h1 className="mb-1 text-center text-lg font-semibold text-foreground">Cuaderno Clínico</h1>
        <h2 className="mb-1 text-center text-base font-medium text-foreground">{title}</h2>
        {subtitle && <p className="mb-6 text-center text-sm text-muted-foreground">{subtitle}</p>}
        <div className={subtitle ? '' : 'mt-6'}>{children}</div>
      </div>
    </main>
  )
}
