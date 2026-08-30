import { forwardRef, useId, useState } from 'react'
import type { InputHTMLAttributes } from 'react'

interface PasswordFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> {
  label: string
  error?: string
}

export const PasswordField = forwardRef<HTMLInputElement, PasswordFieldProps>(function PasswordField(
  { label, error, id, className = '', ...props },
  ref,
) {
  const [visible, setVisible] = useState(false)
  const autoId = useId()
  const inputId = id ?? autoId

  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={inputId} className="text-sm font-medium text-slate-700">
        {label}
      </label>
      <div className="relative">
        <input
          id={inputId}
          ref={ref}
          type={visible ? 'text' : 'password'}
          className={`w-full rounded-lg border px-3 py-2.5 pr-16 text-sm outline-none transition-colors focus:border-slate-500 focus:ring-1 focus:ring-slate-500 ${
            error ? 'border-red-400' : 'border-slate-300'
          } ${className}`}
          {...props}
        />
        <button
          type="button"
          onClick={() => setVisible((v) => !v)}
          className="absolute inset-y-0 right-0 px-3 text-xs font-medium text-slate-500 hover:text-slate-800"
          tabIndex={-1}
        >
          {visible ? 'Ocultar' : 'Mostrar'}
        </button>
      </div>
      {error && <p className="text-sm text-red-600">{error}</p>}
    </div>
  )
})
