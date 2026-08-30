import { useEffect } from 'react'

const isMac = typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('mac')

/**
 * Atajo ⌘/Ctrl + `key`. Pensado para acciones frecuentes de uso diario
 * (⌘/Ctrl+N → nuevo paciente hoy; ⌘/Ctrl+K → búsqueda global se agrega
 * más adelante reutilizando este mismo hook, no una implementación nueva).
 */
export function useGlobalShortcut(key: string, handler: () => void, enabled = true) {
  useEffect(() => {
    if (!enabled) return
    const onKeyDown = (event: KeyboardEvent) => {
      const modifierPressed = isMac ? event.metaKey : event.ctrlKey
      if (modifierPressed && event.key.toLowerCase() === key.toLowerCase()) {
        event.preventDefault()
        handler()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [key, handler, enabled])
}
