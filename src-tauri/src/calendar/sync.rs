//! Punto único de orquestación entre una cita local y su evento espejo en
//! Google Calendar. Se llama después de cada mutación local de una cita
//! (crear/editar/cancelar/archivar/restaurar) y también desde el reintento
//! manual — siempre con la misma secuencia de tres pasos:
//!
//! 1. [`prepare_reconcile`] (síncrono, con `&Connection`): lee del vault todo
//!    lo que hace falta para decidir qué hacer — credenciales, calendario
//!    seleccionado, y una foto de la cita.
//! 2. [`reconcile`] (asíncrono, **sin** `&Connection`): con esa foto ya en
//!    memoria, habla con Google.
//! 3. [`apply_reconcile_effect`] (síncrono, con `&Connection`): persiste lo
//!    que haya que persistir (normalmente, el `google_event_id` nuevo).
//!
//! Esta separación en tres pasos no es estética: `VaultSession::with_connection`
//! solo acepta un closure síncrono (`FnOnce(&Connection) -> T`) y mantiene un
//! `std::sync::Mutex` tomado durante exactamente ese closure — no puede
//! envolver un `.await`, y aunque pudiera, mantener ese mutex tomado durante
//! una llamada de red bloquearía cualquier otra operación del vault mientras
//! se espera la respuesta de Google. Por eso ninguna función de este archivo
//! que haga `.await` recibe jamás una `&Connection`; quien orquesta las tres
//! llamadas (`commands::appointments`) hace dos `with_connection` cortos
//! alrededor de un único `.await` intermedio.
//!
//! Principio no negociable de esta fase, aplicado en cada rama de este
//! archivo: la cita local **ya se guardó** antes de que se llame a
//! `prepare_reconcile` — nada de lo que pase aquí puede deshacer, cancelar,
//! archivar ni modificar esa fila. Como mucho, limpia el vínculo técnico
//! (`google_event_id`) cuando Google confirma que el evento ya no existe.

use rusqlite::Connection;

use crate::repositories::appointments::Appointment;
use crate::repositories::{app_settings, appointments};

use super::client::{self, GoogleApiError};
use super::tokens;

const KEY_CLIENT_ID: &str = "google_client_id";
const KEY_CLIENT_SECRET: &str = "google_client_secret";
const KEY_CALENDAR_ID: &str = "google_calendar_id";

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SyncOutcome {
    /// No hay credenciales de Google configuradas o no hay un calendario
    /// seleccionado — no es un error, simplemente no hay nada que
    /// sincronizar todavía.
    NotConnected,
    /// El estado local ya coincide con lo que debería reflejarse en
    /// Google (p. ej. una cita cancelada que nunca llegó a sincronizarse).
    Skipped,
    Synced,
    /// El `refresh_token` fue rechazado por Google (revocado/expirado). Ya
    /// se limpió del keychain — la próxima consulta de estado de conexión
    /// reportará "desconectado".
    Disconnected,
    /// Variante struct (no tupla) a propósito: un `tag = "kind"` internamente
    /// etiquetado solo puede llevar contenido adicional que serialice como
    /// mapa — una tupla `Failed(String)` panickea en tiempo de ejecución al
    /// serializar (`serde_json` no puede fusionar un string suelto con el
    /// tag). `Failed { message }` sí serializa como
    /// `{"kind":"failed","message":"..."}`.
    Failed { message: String },
}

/// Lee el Client ID/Client Secret configurados por la usuaria en Ajustes.
/// `pub(crate)` (no privado) a propósito: `commands::calendar` también
/// necesita leerlos, tanto para iniciar el flujo OAuth como para reportar el
/// estado de conexión.
pub(crate) fn get_credentials(conn: &Connection) -> Option<(String, String)> {
    let id = app_settings::get(conn, KEY_CLIENT_ID).ok().flatten()?;
    let secret = app_settings::get(conn, KEY_CLIENT_SECRET).ok().flatten()?;
    Some((id, secret))
}

pub fn get_selected_calendar_id(conn: &Connection) -> Option<String> {
    app_settings::get(conn, KEY_CALENDAR_ID).ok().flatten()
}

pub fn set_selected_calendar_id(conn: &Connection, calendar_id: &str) -> rusqlite::Result<()> {
    app_settings::set(conn, KEY_CALENDAR_ID, calendar_id)
}

pub fn clear_selected_calendar_id(conn: &Connection) -> rusqlite::Result<()> {
    app_settings::delete(conn, KEY_CALENDAR_ID)
}

pub fn save_credentials(conn: &Connection, client_id: &str, client_secret: &str) -> rusqlite::Result<()> {
    app_settings::set(conn, KEY_CLIENT_ID, client_id)?;
    app_settings::set(conn, KEY_CLIENT_SECRET, client_secret)
}

/// Todo lo que [`reconcile`] necesita para actuar, ya leído del vault — de
/// aquí en adelante no se vuelve a tocar la base de datos hasta el paso 3
/// ([`apply_reconcile_effect`]).
pub struct ReconcileInput {
    appointment: Appointment,
    calendar_id: String,
    client_id: String,
    client_secret: String,
}

/// Paso 1: síncrono, con `&Connection`. Devuelve `Err(SyncOutcome)` cuando ya
/// se puede resolver el resultado sin tocar la red (nada configurado, o la
/// cita ya no existe) — en ese caso no hay paso 2 ni paso 3 que ejecutar.
pub fn prepare_reconcile(conn: &Connection, appointment_id: &str) -> Result<ReconcileInput, SyncOutcome> {
    let calendar_id = get_selected_calendar_id(conn).ok_or(SyncOutcome::NotConnected)?;
    let (client_id, client_secret) = get_credentials(conn).ok_or(SyncOutcome::NotConnected)?;
    let appointment = appointments::find_by_id(conn, appointment_id)
        .ok()
        .flatten()
        .ok_or_else(|| SyncOutcome::Failed { message: "cita no encontrada".to_string() })?;
    Ok(ReconcileInput { appointment, calendar_id, client_id, client_secret })
}

/// Qué persistir en el paso 3, decidido durante el paso 2 sin acceso a la
/// base de datos.
pub enum ReconcileEffect {
    None,
    SetLink { event_id: Option<String>, calendar_id: Option<String>, synced_at: Option<String> },
}

pub struct ReconcileResult {
    pub outcome: SyncOutcome,
    pub effect: ReconcileEffect,
}

/// `Ok(access_token)`, o `Err(true)` si Google rechazó explícitamente el
/// refresh token (hay que desconectar), o `Err(false)` para cualquier otro
/// fallo (red, servidor de Google caído, no hay token guardado, etc. — no se
/// toca nada local). No recibe `&Connection`: el `refresh_token` vive en el
/// keychain del sistema operativo, no en SQLite.
///
/// `pub(crate)` porque `commands::calendar::list_google_calendars` también
/// necesita un access token vigente fuera del flujo de reconciliación de una
/// cita puntual.
pub(crate) async fn get_valid_access_token(client_id: &str, client_secret: &str) -> Result<String, bool> {
    let refresh_token = tokens::load().ok().flatten().ok_or(false)?;

    match client::refresh_access_token(client_id, client_secret, refresh_token.expose_secret()).await {
        Ok(response) => {
            if let Some(new_refresh) = response.refresh_token {
                let _ = tokens::save(&tokens::RefreshToken::new(new_refresh));
            }
            Ok(response.access_token)
        }
        Err(GoogleApiError::TokenRevoked) => {
            let _ = tokens::clear();
            Err(true)
        }
        Err(_) => Err(false),
    }
}

fn now_iso8601() -> String {
    // Mismo formato que usan las columnas `created_at`/`updated_at` de
    // toda la aplicación — no se agrega una dependencia de fechas solo
    // para esto.
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let (h, m, s) = (time_of_day / 3600, (time_of_day % 3600) / 60, time_of_day % 60);
    // Cálculo de fecha civil a partir de días desde época (algoritmo de
    // Howard Hinnant, de dominio público) — evita agregar `chrono` solo
    // para formatear una marca de tiempo.
    let z = days_since_epoch as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_ = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m_ <= 2 { y + 1 } else { y };
    format!("{y:04}-{m_:02}-{d:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

/// Paso 2: asíncrono, **sin** `&Connection` — solo habla con Google a partir
/// de lo que ya se leyó en el paso 1.
pub async fn reconcile(input: ReconcileInput) -> ReconcileResult {
    let ReconcileInput { appointment, calendar_id, client_id, client_secret } = input;

    let access_token = match get_valid_access_token(&client_id, &client_secret).await {
        Ok(token) => token,
        Err(true) => return ReconcileResult { outcome: SyncOutcome::Disconnected, effect: ReconcileEffect::None },
        Err(false) => {
            return ReconcileResult {
                outcome: SyncOutcome::Failed { message: "no se pudo renovar el acceso a Google".to_string() },
                effect: ReconcileEffect::None,
            }
        }
    };

    let should_have_event = appointment.deleted_at.is_none() && appointment.status != "cancelada";

    if should_have_event {
        match &appointment.google_event_id {
            None => match client::create_event(&access_token, &calendar_id, &appointment.starts_at, &appointment.ends_at).await {
                Ok(event_id) => ReconcileResult {
                    outcome: SyncOutcome::Synced,
                    effect: ReconcileEffect::SetLink {
                        event_id: Some(event_id),
                        calendar_id: Some(calendar_id),
                        synced_at: Some(now_iso8601()),
                    },
                },
                Err(e) => ReconcileResult { outcome: SyncOutcome::Failed { message: e.to_string() }, effect: ReconcileEffect::None },
            },
            Some(event_id) => {
                match client::update_event(&access_token, &calendar_id, event_id, &appointment.starts_at, &appointment.ends_at).await {
                    Ok(()) => ReconcileResult {
                        outcome: SyncOutcome::Synced,
                        effect: ReconcileEffect::SetLink {
                            event_id: Some(event_id.clone()),
                            calendar_id: Some(calendar_id),
                            synced_at: Some(now_iso8601()),
                        },
                    },
                    // El evento desapareció directamente en Google: se
                    // limpia el vínculo técnico, pero la cita local no se
                    // toca — puede volver a sincronizarse manualmente. Se
                    // conserva `last_synced_at` como señal de "estuvo
                    // vinculada, ya no" para la UI.
                    Err(GoogleApiError::ApiError { status: 404 | 410, .. }) => ReconcileResult {
                        outcome: SyncOutcome::Failed {
                            message: "el evento ya no existe en Google — vínculo limpiado, puedes volver a sincronizar".to_string(),
                        },
                        effect: ReconcileEffect::SetLink {
                            event_id: None,
                            calendar_id: None,
                            synced_at: appointment.last_synced_at.clone(),
                        },
                    },
                    Err(e) => ReconcileResult { outcome: SyncOutcome::Failed { message: e.to_string() }, effect: ReconcileEffect::None },
                }
            }
        }
    } else {
        match &appointment.google_event_id {
            None => ReconcileResult { outcome: SyncOutcome::Skipped, effect: ReconcileEffect::None },
            Some(event_id) => match client::delete_event(&access_token, &calendar_id, event_id).await {
                Ok(()) => ReconcileResult {
                    outcome: SyncOutcome::Synced,
                    effect: ReconcileEffect::SetLink { event_id: None, calendar_id: None, synced_at: None },
                },
                // Si Google falla al borrar, se deja el vínculo intacto a
                // propósito — el reintento manual necesita el mismo
                // `google_event_id` para volver a intentar el borrado.
                Err(e) => ReconcileResult { outcome: SyncOutcome::Failed { message: e.to_string() }, effect: ReconcileEffect::None },
            },
        }
    }
}

/// Paso 3: síncrono, con `&Connection`. Un fallo al persistir no se propaga
/// como error duro — el estado en Google ya quedó como corresponde, y la
/// próxima sincronización (automática o manual) puede volver a intentarlo.
pub fn apply_reconcile_effect(conn: &Connection, appointment_id: &str, effect: ReconcileEffect) {
    if let ReconcileEffect::SetLink { event_id, calendar_id, synced_at } = effect {
        let _ = appointments::set_google_link(conn, appointment_id, event_id.as_deref(), calendar_id.as_deref(), synced_at.as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_migrations;
    use crate::db::{open_vault, VaultKey, VAULT_KEY_LEN};

    fn test_conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("cc-calendar-sync-test-{}-{}", std::process::id(), name));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let key = VaultKey::new([0x62u8; VAULT_KEY_LEN]);
        let mut conn = open_vault(&dir.join("vault.db"), &key).unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn create_test_appointment(conn: &Connection) -> String {
        crate::services::appointments::create_appointment(
            conn,
            crate::services::appointments::AppointmentInput {
                patient_id: None,
                starts_at: "2026-09-01T15:00:00Z".to_string(),
                ends_at: "2026-09-01T16:00:00Z".to_string(),
                modality: None,
            },
        )
        .unwrap();
        crate::repositories::appointments::list_active(conn, None, None).unwrap()[0].id.clone()
    }

    #[test]
    fn now_iso8601_produces_a_plausible_looking_timestamp() {
        let ts = now_iso8601();
        assert_eq!(ts.len(), 24, "AAAA-MM-DDTHH:MM:SS.mmmZ debería medir 24 caracteres, salió: {ts}");
        assert!(ts.starts_with("20"), "se espera un año de cuatro dígitos empezando en 20xx: {ts}");
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn selected_calendar_id_roundtrips_through_app_settings() {
        let conn = test_conn("selected-calendar-roundtrip");
        assert_eq!(get_selected_calendar_id(&conn), None);
        set_selected_calendar_id(&conn, "primary").unwrap();
        assert_eq!(get_selected_calendar_id(&conn), Some("primary".to_string()));
        clear_selected_calendar_id(&conn).unwrap();
        assert_eq!(get_selected_calendar_id(&conn), None);
    }

    /// `prepare_reconcile` es síncrono: este caso no necesita tocar la red
    /// en absoluto, porque no hay nada configurado — se resuelve por
    /// completo dentro del paso 1.
    #[test]
    fn prepare_reconcile_reports_not_connected_without_credentials() {
        let conn = test_conn("reconcile-not-connected");
        let id = create_test_appointment(&conn);

        let result = prepare_reconcile(&conn, &id);
        assert!(matches!(result, Err(SyncOutcome::NotConnected)));
    }

    #[test]
    fn prepare_reconcile_reports_failed_for_a_nonexistent_appointment() {
        let conn = test_conn("reconcile-nonexistent-appointment");
        set_selected_calendar_id(&conn, "primary").unwrap();
        save_credentials(&conn, "client-id", "client-secret").unwrap();

        let result = prepare_reconcile(&conn, "does-not-exist");
        assert!(matches!(result, Err(SyncOutcome::Failed { .. })));
    }

    #[test]
    fn prepare_reconcile_succeeds_once_everything_is_configured() {
        let conn = test_conn("reconcile-prepared-ok");
        set_selected_calendar_id(&conn, "primary").unwrap();
        save_credentials(&conn, "client-id", "client-secret").unwrap();
        let id = create_test_appointment(&conn);

        let input = prepare_reconcile(&conn, &id).expect("todo lo necesario está configurado");
        assert_eq!(input.appointment.id, id);
        assert_eq!(input.calendar_id, "primary");
        assert_eq!(input.client_id, "client-id");
    }

    /// Regresión: `SyncOutcome` es un enum internamente etiquetado
    /// (`tag = "kind"`), y ese estilo de etiquetado solo admite contenido
    /// adicional que serialice como mapa — una variante tupla como
    /// `Failed(String)` compila pero **panickea en tiempo de ejecución** al
    /// serializar (justo lo que devolvería IPC de Tauri en cualquier fallo
    /// de red real). Este test fija el contrato: todas las variantes,
    /// incluida `Failed`, deben serializar sin error y con la forma
    /// esperada.
    #[test]
    fn every_sync_outcome_variant_serializes_without_panicking() {
        let cases: Vec<(SyncOutcome, &str)> = vec![
            (SyncOutcome::NotConnected, r#"{"kind":"not_connected"}"#),
            (SyncOutcome::Skipped, r#"{"kind":"skipped"}"#),
            (SyncOutcome::Synced, r#"{"kind":"synced"}"#),
            (SyncOutcome::Disconnected, r#"{"kind":"disconnected"}"#),
            (SyncOutcome::Failed { message: "boom".to_string() }, r#"{"kind":"failed","message":"boom"}"#),
        ];
        for (outcome, expected_json) in cases {
            let json = serde_json::to_string(&outcome).expect("SyncOutcome debe serializar sin panickear");
            assert_eq!(json, expected_json);
        }
    }

    #[test]
    fn apply_reconcile_effect_persists_a_set_link_result() {
        let conn = test_conn("apply-effect-set-link");
        let id = create_test_appointment(&conn);

        apply_reconcile_effect(
            &conn,
            &id,
            ReconcileEffect::SetLink {
                event_id: Some("evt-123".to_string()),
                calendar_id: Some("primary".to_string()),
                synced_at: Some("2026-09-01T15:05:00.000Z".to_string()),
            },
        );

        let appointment = appointments::find_by_id(&conn, &id).unwrap().unwrap();
        assert_eq!(appointment.google_event_id.as_deref(), Some("evt-123"));
        assert_eq!(appointment.google_calendar_id.as_deref(), Some("primary"));
        assert!(appointment.last_synced_at.is_some());
    }

    #[test]
    fn apply_reconcile_effect_none_leaves_the_row_untouched() {
        let conn = test_conn("apply-effect-none");
        let id = create_test_appointment(&conn);

        apply_reconcile_effect(&conn, &id, ReconcileEffect::None);

        let appointment = appointments::find_by_id(&conn, &id).unwrap().unwrap();
        assert!(appointment.google_event_id.is_none());
    }
}
