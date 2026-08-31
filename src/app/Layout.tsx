import { useState } from 'react'
import { Link, NavLink, Outlet } from 'react-router-dom'
import { authApi } from '../features/auth/api'
import { ChangePasswordModal } from '../features/auth/ChangePasswordModal'

const navLinkClassName = ({ isActive }: { isActive: boolean }) =>
  `text-sm font-medium transition-colors ${isActive ? 'text-accent' : 'text-muted-foreground hover:text-foreground'}`

export function Layout({ onLocked }: { onLocked: () => void }) {
  const [showChangePassword, setShowChangePassword] = useState(false)

  const handleLock = async () => {
    await authApi.lockVault()
    onLocked()
  }

  return (
    <div className="min-h-screen bg-background">
      <header className="flex items-center justify-between border-b border-border bg-surface-elevated px-6 py-3">
        <div className="flex items-center gap-6">
          <Link to="/" className="flex items-center gap-2 text-sm font-semibold text-foreground">
            <span className="h-2 w-2 rounded-full bg-accent" aria-hidden="true" />
            Cuaderno Clínico
          </Link>
          <nav className="flex gap-4">
            <NavLink to="/" end className={navLinkClassName}>
              Inicio
            </NavLink>
            <NavLink to="/patients" className={navLinkClassName}>
              Pacientes
            </NavLink>
          </nav>
        </div>
        <div className="flex gap-4 text-xs text-muted-foreground">
          <button onClick={() => setShowChangePassword(true)} className="hover:text-accent">
            Cambiar contraseña
          </button>
          <button onClick={handleLock} className="hover:text-accent">
            Bloquear
          </button>
        </div>
      </header>
      <Outlet />

      {showChangePassword && (
        <ChangePasswordModal onClose={() => setShowChangePassword(false)} onChanged={() => setShowChangePassword(false)} />
      )}
    </div>
  )
}
