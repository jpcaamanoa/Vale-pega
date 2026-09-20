import { useId, useState } from 'react'
import type { KeyboardEvent } from 'react'

interface TagListFieldProps {
  label: string
  helperText?: string
  tags: string[]
  onChange: (tags: string[]) => void
  error?: string
  placeholder?: string
}

/**
 * Editor de una lista corta de textos como "chips" agregables/eliminables —
 * pensado para reemplazar campos que antes exigían escribir JSON a mano
 * (p. ej. "Factores de riesgo"). El estado siempre es `string[]`; quien use
 * este componente decide cómo serializarlo (JSON, texto separado por líneas,
 * etc.) — el componente nunca expone esa representación interna a la
 * usuaria.
 */
export function TagListField({ label, helperText, tags, onChange, error, placeholder }: TagListFieldProps) {
  const [draft, setDraft] = useState('')
  const inputId = useId()

  const commitDraft = () => {
    const value = draft.trim()
    if (!value) return
    if (!tags.includes(value)) {
      onChange([...tags, value])
    }
    setDraft('')
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      commitDraft()
    } else if (e.key === 'Backspace' && draft === '' && tags.length > 0) {
      onChange(tags.slice(0, -1))
    }
  }

  const removeAt = (index: number) => {
    onChange(tags.filter((_, i) => i !== index))
  }

  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={inputId} className="text-sm font-medium text-foreground">
        {label}
      </label>
      {helperText && <p className="text-xs text-muted-foreground">{helperText}</p>}
      <div className={`flex min-h-[2.75rem] flex-wrap items-center gap-2 rounded-lg border bg-surface px-3 py-2 focus-within:border-accent focus-within:ring-1 focus-within:ring-accent ${error ? 'border-danger' : 'border-border'}`}>
        {tags.map((tag, i) => (
          <span key={`${tag}-${i}`} className="inline-flex items-center gap-1.5 rounded-full bg-accent-soft px-2.5 py-1 text-xs font-medium text-accent">
            {tag}
            <button
              type="button"
              onClick={() => removeAt(i)}
              aria-label={`Quitar "${tag}"`}
              className="rounded-full text-accent/70 hover:text-accent"
            >
              ×
            </button>
          </span>
        ))}
        <input
          id={inputId}
          type="text"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={handleKeyDown}
          onBlur={commitDraft}
          placeholder={tags.length === 0 ? (placeholder ?? 'Escribe y presiona Enter…') : 'Agregar otro…'}
          className="min-w-[8rem] flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
        />
      </div>
      {error && <p className="text-sm text-danger">{error}</p>}
    </div>
  )
}
