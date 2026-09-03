import { useEffect, useState } from 'react'
import { statisticsApi } from './api'
import type { GeoDistributionItem, GeographicStatistics } from './types'

type Filter = 'active' | 'all'

/**
 * Paleta cualitativa derivada del único acento de marca (`--color-accent`)
 * con `color-mix`, en vez de colores nuevos escritos a mano — nunca se
 * introduce un hexadecimal fuera del sistema de tokens (ver
 * `docs/ARCHITECTURE.md` sección 14). "Otras" siempre usa un tono neutro
 * aparte, para que la categoría agrupada por privacidad se distinga del
 * resto a simple vista.
 */
const CHART_PALETTE = [
  'var(--color-accent)',
  'color-mix(in srgb, var(--color-accent) 75%, white)',
  'color-mix(in srgb, var(--color-accent) 50%, white)',
  'color-mix(in srgb, var(--color-accent) 85%, black)',
  'color-mix(in srgb, var(--color-accent) 60%, black)',
  'color-mix(in srgb, var(--color-accent) 30%, white)',
]
const OTHER_COLOR = 'color-mix(in srgb, var(--color-muted-foreground) 35%, white)'
const OTHER_LABEL = 'Otras'

function colorFor(item: GeoDistributionItem, index: number): string {
  if (item.label === OTHER_LABEL) return OTHER_COLOR
  return CHART_PALETTE[index % CHART_PALETTE.length]
}

function formatPercent(count: number, total: number): string {
  if (total === 0) return '0%'
  return `${Math.round((count / total) * 100)}%`
}

/** Donut nativo en SVG (sin librería de gráficos): un círculo de fondo más
 * un arco por categoría, dibujado con `strokeDasharray`/`strokeDashoffset`
 * sobre la circunferencia. Nunca hay click-through hacia un listado de
 * pacientes — solo lectura. */
function RegionDonut({ items }: { items: GeoDistributionItem[] }) {
  const total = items.reduce((sum, item) => sum + item.count, 0)
  if (total === 0) {
    return <p className="text-sm text-muted-foreground">Sin datos de región para mostrar.</p>
  }

  const radius = 70
  const strokeWidth = 28
  const circumference = 2 * Math.PI * radius
  // Fracción acumulada de todos los segmentos *antes* de cada uno, para
  // ubicar su punto de inicio sobre la circunferencia sin una variable
  // mutable reasignada durante el render.
  const cumulativeFractionsBefore = items.reduce<{ before: number[]; running: number }>(
    (acc, item) => {
      acc.before.push(acc.running)
      acc.running += item.count / total
      return acc
    },
    { before: [], running: 0 },
  ).before

  return (
    <div className="flex flex-col items-center gap-6 sm:flex-row">
      <svg width="180" height="180" viewBox="0 0 180 180" role="img" aria-label="Distribución de pacientes por región">
        <g transform="rotate(-90 90 90)">
          <circle cx="90" cy="90" r={radius} fill="none" stroke="var(--color-border)" strokeWidth={strokeWidth} />
          {items.map((item, index) => {
            const fraction = item.count / total
            const dash = fraction * circumference
            const offset = -(cumulativeFractionsBefore[index] * circumference)
            return (
              <circle
                key={item.label}
                cx="90"
                cy="90"
                r={radius}
                fill="none"
                stroke={colorFor(item, index)}
                strokeWidth={strokeWidth}
                strokeDasharray={`${dash} ${circumference - dash}`}
                strokeDashoffset={offset}
              />
            )
          })}
        </g>
        <text x="90" y="86" textAnchor="middle" className="fill-foreground" style={{ fontSize: '24px', fontWeight: 600 }}>
          {total}
        </text>
        <text x="90" y="104" textAnchor="middle" className="fill-muted-foreground" style={{ fontSize: '11px' }}>
          pacientes
        </text>
      </svg>

      <ul className="flex flex-1 flex-col gap-2">
        {items.map((item, index) => (
          <li key={item.label} className="flex items-center justify-between gap-3 text-sm">
            <span className="flex items-center gap-2">
              <span
                className="h-3 w-3 shrink-0 rounded-full"
                style={{ backgroundColor: colorFor(item, index) }}
                aria-hidden="true"
              />
              <span className="text-foreground">{item.label}</span>
            </span>
            <span className="shrink-0 text-muted-foreground">
              {item.count} · {formatPercent(item.count, total)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}

/** Barras horizontales nativas (div + width%), sin librería de gráficos. */
function CommuneBars({ items }: { items: GeoDistributionItem[] }) {
  if (items.length === 0) {
    return <p className="text-sm text-muted-foreground">Sin datos de comuna para mostrar.</p>
  }
  const total = items.reduce((sum, item) => sum + item.count, 0)
  const max = Math.max(...items.map((item) => item.count))

  return (
    <ul className="flex flex-col gap-3">
      {items.map((item, index) => (
        <li key={item.label} className="flex flex-col gap-1">
          <div className="flex items-center justify-between text-sm">
            <span className="text-foreground">{item.label}</span>
            <span className="text-muted-foreground">
              {item.count} · {formatPercent(item.count, total)}
            </span>
          </div>
          <div className="h-2.5 w-full overflow-hidden rounded-full bg-accent-soft">
            <div
              className="h-full rounded-full"
              style={{ width: `${(item.count / max) * 100}%`, backgroundColor: colorFor(item, index) }}
            />
          </div>
        </li>
      ))}
    </ul>
  )
}

/**
 * Pantalla "Estadísticas" (Fase 6.1): agregados geográficos de pacientes,
 * siempre calculados en el backend (`GROUP BY`) — nunca se trae aquí una
 * lista de pacientes para contarla en el cliente. Sin click-through desde
 * ningún gráfico hacia una ficha o listado de pacientes: es de solo
 * lectura, a propósito.
 */
export function StatisticsScreen() {
  const [filter, setFilter] = useState<Filter>('active')
  const [stats, setStats] = useState<GeographicStatistics | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setStats(null)
    setError(null)
    statisticsApi
      .geographic(filter === 'all')
      .then((result) => {
        if (!cancelled) setStats(result)
      })
      .catch((err) => {
        if (!cancelled) setError(typeof err === 'string' ? err : 'No se pudieron cargar las estadísticas.')
      })
    return () => {
      cancelled = true
    }
  }, [filter])

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-10">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-xl font-semibold text-foreground">Estadísticas</h1>
        <div className="flex gap-1 rounded-lg border border-border p-1" role="group" aria-label="Filtro de pacientes">
          {(['active', 'all'] as const).map((value) => (
            <button
              key={value}
              onClick={() => setFilter(value)}
              aria-pressed={filter === value}
              className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                filter === value ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              {value === 'active' ? 'Activos' : 'Todos'}
            </button>
          ))}
        </div>
      </div>

      {error && <p className="text-sm text-danger">{error}</p>}
      {!stats && !error && <p className="text-sm text-muted-foreground">Cargando…</p>}

      {stats && (
        <>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div className="rounded-lg border border-border bg-surface p-6">
              <p className="text-sm text-muted-foreground">Con ubicación registrada</p>
              <p className="text-2xl font-semibold text-accent">{stats.withLocation}</p>
            </div>
            <div className="rounded-lg border border-border bg-surface p-6">
              <p className="text-sm text-muted-foreground">Sin ubicación registrada</p>
              <p className="text-2xl font-semibold text-foreground">{stats.withoutLocation}</p>
            </div>
          </div>

          <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
            <h2 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
              Distribución por región
            </h2>
            <RegionDonut items={stats.byRegion} />
          </section>

          <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
            <h2 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
              Distribución por comuna
            </h2>
            <CommuneBars items={stats.byCommune} />
          </section>

          <p className="text-xs text-muted-foreground">
            Las categorías con menos de 3 pacientes se agrupan en «{OTHER_LABEL}» para que ninguna persona sea
            identificable a partir de un gráfico.
          </p>
        </>
      )}
    </div>
  )
}
