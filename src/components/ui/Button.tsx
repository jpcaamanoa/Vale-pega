import type { ButtonHTMLAttributes } from 'react'

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost'
}

const variants: Record<NonNullable<ButtonProps['variant']>, string> = {
  primary: 'bg-slate-900 text-white hover:bg-slate-800 disabled:bg-slate-400',
  secondary: 'bg-white text-slate-900 border border-slate-300 hover:bg-slate-50',
  ghost: 'text-slate-600 hover:text-slate-900 hover:bg-slate-100',
}

export function Button({ variant = 'primary', className = '', ...props }: ButtonProps) {
  return (
    <button
      className={`inline-flex items-center justify-center gap-2 rounded-lg px-4 py-2.5 text-sm font-medium transition-colors disabled:cursor-not-allowed ${variants[variant]} ${className}`}
      {...props}
    />
  )
}
