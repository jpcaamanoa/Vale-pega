import { invoke } from '@tauri-apps/api/core'
import type { GoogleCalendarListItem, GoogleConnectionStatus } from './types'

export const googleCalendarApi = {
  status: () => invoke<GoogleConnectionStatus>('google_connection_status'),

  saveCredentials: (clientId: string, clientSecret: string) =>
    invoke<void>('save_google_credentials', { clientId, clientSecret }),

  /** Abre el navegador y espera el callback OAuth — puede tardar hasta 5 minutos (ver `AUTH_TIMEOUT` en el backend). */
  beginAuth: () => invoke<void>('begin_google_auth'),

  listCalendars: () => invoke<GoogleCalendarListItem[]>('list_google_calendars'),

  selectCalendar: (calendarId: string) => invoke<void>('select_google_calendar', { calendarId }),

  disconnect: () => invoke<void>('disconnect_google_calendar'),
}
