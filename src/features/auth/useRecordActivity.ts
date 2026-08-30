import { useEffect, useRef } from 'react'
import { authApi } from './api'

const THROTTLE_MS = 5000

/** Avisa al backend que hubo actividad de la usuaria, para el bloqueo automático por inactividad. */
export function useRecordActivity(enabled: boolean) {
  const lastSentAt = useRef(0)

  useEffect(() => {
    if (!enabled) return

    const handleActivity = () => {
      const now = Date.now()
      if (now - lastSentAt.current < THROTTLE_MS) return
      lastSentAt.current = now
      void authApi.recordVaultActivity()
    }

    const events: (keyof WindowEventMap)[] = ['mousemove', 'keydown', 'mousedown', 'wheel']
    events.forEach((event) => window.addEventListener(event, handleActivity, { passive: true }))
    handleActivity()

    return () => {
      events.forEach((event) => window.removeEventListener(event, handleActivity))
    }
  }, [enabled])
}
