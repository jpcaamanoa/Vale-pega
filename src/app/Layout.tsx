import { useState } from 'react'
import { Link, Outlet } from 'react-router-dom'
import { authApi } from '../features/auth/api'
import { ChangePasswordModal } from '../features/auth/ChangePasswordModal'

export function Layout({ onLocked }: { onLocked: () => void }) {
  const [showChangePassword, setShowChangePassword] = useState(false)

  const handleLock = async () => {
    await authApi.lockVault()
    onLocked()
  }

  return (
    <div className="min-h-screen bg-slate-50">
      <header className="flex items-center justify-between border-b border-slate-200 bg-white px-6 py-3">
        <Link to="/" className="text-sm font-semibold text-slate-900">
          Cuaderno Clínico
        </Link>
        <div className="flex gap-4 text-xs text-slate-500">
          <button onClick={() => setShowChangePassword(true)} className="hover:text-slate-800">
            Cambiar contraseña
          </button>
          <button onClick={handleLock} className="hover:text-slate-800">
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
