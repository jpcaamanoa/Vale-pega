import type { FormEvent } from 'react'
import { useEffect, useState } from 'react'
import { Button } from '../../components/ui/Button'
import { Select } from '../../components/ui/Select'
import { TextField } from '../../components/ui/TextField'
import { googleCalendarApi } from './api'
import type { GoogleCalendarListItem, GoogleConnectionStatus } from './types'

function StatusBadge({ label, ok }: { label: string; ok: boolean }) {
  return (
    <span
      className={`rounded-full px-2.5 py-1 text-xs font-medium ${
        ok ? 'bg-success-soft text-success' : 'bg-disabled text-disabled-foreground'
      }`}
    >
      {label}: {ok ? 'Sí' : 'No'}
    </span>
  )
}

export function SettingsScreen() {
  const [status, setStatus] = useState<GoogleConnectionStatus | null>(null)
  const [statusError, setStatusError] = useState<string | null>(null)

  const [clientId, setClientId] = useState('')
  const [clientSecret, setClientSecret] = useState('')
  const [savingCredentials, setSavingCredentials] = useState(false)
  const [credentialsError, setCredentialsError] = useState<string | null>(null)
  const [credentialsSaved, setCredentialsSaved] = useState(false)

  const [connecting, setConnecting] = useState(false)
  const [connectError, setConnectError] = useState<string | null>(null)

  const [calendars, setCalendars] = useState<GoogleCalendarListItem[] | null>(null)
  const [loadingCalendars, setLoadingCalendars] = useState(false)
  const [calendarsError, setCalendarsError] = useState<string | null>(null)
  const [selectingCalendar, setSelectingCalendar] = useState(false)

  const [confirmingDisconnect, setConfirmingDisconnect] = useState(false)
  const [disconnecting, setDisconnecting] = useState(false)

  const loadStatus = () => {
    googleCalendarApi
      .status()
      .then((s) => {
        setStatus(s)
        setStatusError(null)
      })
      .catch((err) => setStatusError(typeof err === 'string' ? err : 'No se pudo leer el estado de la conexión.'))
  }

  useEffect(loadStatus, [])

  useEffect(() => {
    if (!status?.connected) {
      setCalendars(null)
      return
    }
    let cancelled = false
    setLoadingCalendars(true)
    googleCalendarApi
      .listCalendars()
      .then((results) => {
        if (!cancelled) {
          setCalendars(results)
          setCalendarsError(null)
        }
      })
      .catch((err) => {
        if (!cancelled) setCalendarsError(typeof err === 'string' ? err : 'No se pudieron listar los calendarios.')
      })
      .finally(() => {
        if (!cancelled) setLoadingCalendars(false)
      })
    return () => {
      cancelled = true
    }
  }, [status?.connected])

  const handleSaveCredentials = async (e: FormEvent) => {
    e.preventDefault()
    setSavingCredentials(true)
    setCredentialsError(null)
    setCredentialsSaved(false)
    try {
      await googleCalendarApi.saveCredentials(clientId.trim(), clientSecret.trim())
      setCredentialsSaved(true)
      setClientId('')
      setClientSecret('')
      loadStatus()
    } catch (err) {
      setCredentialsError(typeof err === 'string' ? err : 'No se pudieron guardar las credenciales.')
    } finally {
      setSavingCredentials(false)
    }
  }

  const handleConnect = async () => {
    setConnecting(true)
    setConnectError(null)
    try {
      await googleCalendarApi.beginAuth()
      loadStatus()
    } catch (err) {
      setConnectError(typeof err === 'string' ? err : 'No se pudo completar la conexión con Google.')
    } finally {
      setConnecting(false)
    }
  }

  const handleSelectCalendar = async (calendarId: string) => {
    if (!calendarId) return
    setSelectingCalendar(true)
    setCalendarsError(null)
    try {
      await googleCalendarApi.selectCalendar(calendarId)
      loadStatus()
    } catch (err) {
      setCalendarsError(typeof err === 'string' ? err : 'No se pudo seleccionar el calendario.')
    } finally {
      setSelectingCalendar(false)
    }
  }

  const handleDisconnect = async () => {
    setDisconnecting(true)
    try {
      await googleCalendarApi.disconnect()
      setConfirmingDisconnect(false)
      loadStatus()
    } catch (err) {
      setConnectError(typeof err === 'string' ? err : 'No se pudo desconectar.')
    } finally {
      setDisconnecting(false)
    }
  }

  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-8 px-6 py-10">
      <h1 className="text-xl font-semibold text-foreground">Ajustes</h1>

      <section className="flex flex-col gap-4 rounded-lg border border-border bg-surface p-6">
        <div>
          <h2 className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">Google Calendar</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            Sincronización unidireccional: las citas creadas en Cuaderno Clínico se reflejan en un calendario de
            Google ya existente. Solo se envía el horario — nunca el nombre del paciente, diagnóstico, ni ningún
            otro dato clínico.
          </p>
        </div>

        {statusError && <p className="text-sm text-danger">{statusError}</p>}

        {status && (
          <div className="flex flex-col gap-4">
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <StatusBadge label="Credenciales" ok={status.credentialsConfigured} />
              <StatusBadge label="Conexión" ok={status.connected} />
              {status.connected && <StatusBadge label="Calendario seleccionado" ok={status.calendarId !== null} />}
            </div>

            <div className="border-t border-border pt-4">
              <h3 className="mb-2 text-sm font-medium text-foreground">1. Cliente OAuth de Google Cloud Console</h3>
              <p className="mb-3 text-xs text-muted-foreground">
                Crea un cliente OAuth de tipo "Aplicación de escritorio" en Google Cloud Console y pega aquí su
                Client ID y Client Secret. Se guardan cifrados dentro del vault — nunca se vuelven a mostrar en
                pantalla.
              </p>
              <form onSubmit={handleSaveCredentials} className="flex flex-col gap-3 sm:flex-row sm:items-end">
                <div className="flex-1">
                  <TextField
                    label="Client ID"
                    value={clientId}
                    onChange={(e) => setClientId(e.target.value)}
                    placeholder={status.credentialsConfigured ? 'Ya configurado' : 'xxxxx.apps.googleusercontent.com'}
                  />
                </div>
                <div className="flex-1">
                  <TextField
                    label="Client Secret"
                    type="password"
                    value={clientSecret}
                    onChange={(e) => setClientSecret(e.target.value)}
                    placeholder={status.credentialsConfigured ? 'Ya configurado' : 'GOCSPX-…'}
                  />
                </div>
                <Button type="submit" variant="secondary" disabled={savingCredentials || !clientId || !clientSecret}>
                  {savingCredentials ? 'Guardando…' : 'Guardar'}
                </Button>
              </form>
              {credentialsError && <p className="mt-2 text-sm text-danger">{credentialsError}</p>}
              {credentialsSaved && <p className="mt-2 text-sm text-success">Credenciales guardadas.</p>}
            </div>

            <div className="border-t border-border pt-4">
              <h3 className="mb-2 text-sm font-medium text-foreground">2. Conexión</h3>
              {status.connected ? (
                <div className="flex items-center justify-between gap-3">
                  <p className="text-sm text-muted-foreground">Conectado a tu cuenta de Google.</p>
                  <Button variant="secondary" onClick={() => setConfirmingDisconnect(true)}>
                    Desconectar
                  </Button>
                </div>
              ) : (
                <div className="flex flex-col gap-2">
                  <p className="text-sm text-muted-foreground">
                    Se abrirá tu navegador para iniciar sesión en Google y autorizar el acceso.
                  </p>
                  <Button onClick={handleConnect} disabled={connecting || !status.credentialsConfigured}>
                    {connecting ? 'Esperando autorización…' : 'Conectar con Google'}
                  </Button>
                  {!status.credentialsConfigured && (
                    <p className="text-xs text-muted-foreground">Primero guarda el Client ID y Client Secret.</p>
                  )}
                </div>
              )}
              {connectError && <p className="mt-2 text-sm text-danger">{connectError}</p>}
            </div>

            {status.connected && (
              <div className="border-t border-border pt-4">
                <h3 className="mb-2 text-sm font-medium text-foreground">3. Calendario</h3>
                <p className="mb-3 text-xs text-muted-foreground">
                  Elige un calendario ya existente en tu cuenta de Google. Cuaderno Clínico nunca crea un calendario
                  nuevo.
                </p>
                {loadingCalendars && <p className="text-sm text-muted-foreground">Cargando calendarios…</p>}
                {calendarsError && <p className="text-sm text-danger">{calendarsError}</p>}
                {calendars && (
                  <Select
                    label="Calendario"
                    value={status.calendarId ?? ''}
                    disabled={selectingCalendar}
                    onChange={(e) => handleSelectCalendar(e.target.value)}
                  >
                    <option value="" disabled>
                      Selecciona un calendario…
                    </option>
                    {calendars.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.summary}
                        {c.primary ? ' (principal)' : ''}
                      </option>
                    ))}
                  </Select>
                )}
              </div>
            )}
          </div>
        )}
      </section>

      {confirmingDisconnect && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/40 px-4">
          <div className="w-full max-w-sm rounded-2xl bg-surface-elevated p-6 shadow-lg">
            <h2 className="mb-2 text-base font-semibold text-foreground">Desconectar Google Calendar</h2>
            <p className="mb-4 text-sm text-muted-foreground">
              Se revocará el acceso y se dejará de sincronizar. Las citas locales no se modifican. El Client ID y
              Client Secret guardados no se borran, para poder reconectar más adelante sin volver a configurarlos.
            </p>
            <div className="flex justify-end gap-2">
              <Button variant="secondary" onClick={() => setConfirmingDisconnect(false)}>
                Volver
              </Button>
              <Button onClick={handleDisconnect} disabled={disconnecting}>
                {disconnecting ? 'Desconectando…' : 'Desconectar'}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
