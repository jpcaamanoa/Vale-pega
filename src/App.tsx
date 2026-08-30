import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'

interface AppInfo {
  name: string
  version: string
}

/**
 * Fase 1.1: shell mínimo que confirma que la cadena completa
 * (Tauri → comando Rust → IPC → React → Tailwind) funciona de
 * extremo a extremo antes de construir funcionalidad clínica real.
 */
function App() {
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    invoke<AppInfo>('app_info')
      .then(setAppInfo)
      .catch((err) => setError(String(err)))
  }, [])

  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-3 bg-slate-50 text-slate-800">
      <h1 className="text-2xl font-semibold tracking-tight text-slate-900">Cuaderno Clínico</h1>
      <p className="text-sm text-slate-500">
        {error && <span className="text-red-600">Error al conectar con el backend: {error}</span>}
        {!error && !appInfo && 'Conectando con el backend…'}
        {appInfo && (
          <>
            Backend conectado — {appInfo.name} v{appInfo.version}
          </>
        )}
      </p>
    </main>
  )
}

export default App
