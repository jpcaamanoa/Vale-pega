import { forwardRef, useId } from 'react'
import type { SelectHTMLAttributes } from 'react'

interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  label: string
  error?: string
}

export const Select = forwardRef<HTMLSelectElement, SelectProps>(function Select(
  { label, error, id, className = '', children, ...props },
  ref,
) {
  const autoId = useId()
  const inputId = id ?? autoId

  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={inputId} className="text-sm font-medium text-slate-700">
        {label}
      </label>
      <select
        id={inputId}
        ref={ref}
        className={`w-full rounded-lg border bg-white px-3 py-2 text-sm outline-none transition-colors focus:border-slate-500 focus:ring-1 focus:ring-slate-500 ${
          error ? 'border-red-400' : 'border-slate-300'
        } ${className}`}
        {...props}
      >
        {children}
      </select>
      {error && <p className="text-sm text-red-600">{error}</p>}
    </div>
  )
})
