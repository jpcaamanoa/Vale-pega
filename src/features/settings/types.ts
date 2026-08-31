export interface GoogleConnectionStatus {
  /** Hay Client ID/Client Secret guardados (en `app_settings`, dentro del vault cifrado). */
  credentialsConfigured: boolean
  /** Hay un refresh token válido en el keychain del sistema operativo. */
  connected: boolean
  calendarId: string | null
}

export interface GoogleCalendarListItem {
  id: string
  summary: string
  primary: boolean
}
