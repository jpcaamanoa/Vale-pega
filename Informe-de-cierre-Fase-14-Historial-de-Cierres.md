# Informe de cierre — Fase 14: Historial de cierres y hardening longitudinal

Sigue el formato obligatorio de `CLAUDE.md` sección 10, más las secciones específicas pedidas para
esta microfase.

## 1. Baseline

Verificado antes de tocar nada:

- `git rev-parse HEAD` → `5427aee` — coincide exactamente con el baseline esperado.
- `git branch --show-current` → `claude/cuaderno-clinico-desktop-udijjq` — correcto.
- `git status --porcelain=2` → un único archivo sin trackear: `Auditoria-transicion-Fase-13-siguiente-fase.md`
  (el informe de la auditoría anterior, deliberadamente dejado sin commitear por esa tarea — "0
  commits" fue una instrucción explícita de esa auditoría). No es una discrepancia: coincide
  exactamente con el estado en que quedó el repositorio al cierre de esa auditoría, ya reportado.
- `origin/claude/cuaderno-clinico-desktop-udijjq` → `5427aee` — sin divergencia.

Baseline confirmado sin necesidad de detenerse.

## 2. Regresión inicial

| Comando | Resultado |
|---|---|
| `cargo test --release` | **668/668 en verde** |
| `cargo clippy --release --all-targets` | 0 warnings |
| `cargo build --release` | limpio |
| `npm run build` | limpio, sin errores TS |
| `npm run lint` | 23 warnings, 0 errors |
| `git diff --check` | limpio |

Coincide exactamente con lo declarado en la auditoría de transición.

## 3. Auditoría del historial de cierres

Inspeccionados directamente, antes de escribir ninguna UI:

- **`repositories/episode_closures.rs`**: `EpisodeClosure { id, episode_id, closed_at, reason,
  reason_detail, outcome, summary, recommendations, reverted_at, reverted_reason, created_at,
  updated_at }`. `list_history_by_episode` devuelve **todo** el historial de un proceso — vigente y
  anulados, sin filtrar nada — ordenado por `created_at DESC` (más reciente primero). Confirmado
  con el test `list_history_includes_both_active_and_reverted_ordered_most_recent_first`.
- **`services/episode_closures.rs`**: `list_closure_history` es una capa fina sobre el repositorio
  (solo valida que el proceso exista). `reason`/`outcome` usan las taxonomías fijas ya conocidas
  (`VALID_REASONS`/`VALID_OUTCOMES`). `reverted_at`/`reverted_reason` son `Option<String>` — `None`
  para el cierre vigente, ambos presentes juntos para uno anulado (nunca uno sin el otro, reforzado
  por un `CHECK` de esquema desde `SCHEMA_V5`).
- **`commands/episode_closures.rs`**: `list_episode_closure_history` ya registrado en `lib.rs` desde
  la Fase 11 — sin cambios necesarios.
- **Frontend** (`api.ts`/`types.ts`): `episodeClosuresApi.listHistory(episodeId): Promise<EpisodeClosure[]>`
  y el tipo `EpisodeClosure` en TypeScript ya reflejan exactamente el DTO de Rust (`camelCase`) —
  sin necesidad de ningún tipo nuevo.
- **`ClosureSection.tsx`**: muestra el cierre **vigente** únicamente cuando `episode.status ===
  'cerrado'` (vía `episodeClosuresApi.getActive`). Cuando el proceso se reabre (`revert_closure`),
  este componente deja de mostrar cualquier información de cierre — confirmando el hueco real que
  esta microfase cierra: sin una vista de historial, un proceso reabierto no dejaba ningún rastro
  visible de que alguna vez estuvo cerrado.
- **Tests existentes**: 12 tests en `repositories::episode_closures` y 25 en
  `services::episode_closures`, cubriendo inserción, anulación, reapertura, y el caso crítico de
  conflicto A/B — todos verdes, ninguno modificado.

No se cambió el modelo de datos en ningún punto de esta auditoría previa.

## 4. UI implementada

`src/features/treatment-episodes/ClosureHistorySection.tsx` (archivo nuevo), integrado en
`TreatmentEpisodeDetailScreen.tsx` inmediatamente después de `ClosureSection`. Cada cierre anulado
se muestra en una tarjeta con: fecha de cierre, motivo (+ detalle si es `'otro'`), resultado,
resumen (si existe), recomendaciones (si existen), fecha de anulación, motivo de la anulación, y
una etiqueta visual "Cierre anulado". Nunca se muestran `patientId`/`episodeId` como información de
UI. Sin ningún botón de acción — es una vista de solo lectura.

## 5. Cierres anulados

Distinguidos con una etiqueta discreta ("Cierre anulado", estilo neutro — sin colores de alarma
como `danger`/`warning`, para no sugerir un problema donde solo hay una corrección administrativa
ya resuelta). Permanecen visibles indefinidamente, nunca desaparecen, no tienen ningún botón de
edición ni de eliminación — coherente con la inmutabilidad ya establecida en Fase 11
(`repositories::episode_closures` nunca expuso ni expone una función `update` de contenido).

## 6. Cierre vigente

`ClosureSection.tsx` no se modificó — sigue funcionando exactamente igual. La nueva sección nunca
repite el cierre vigente: `ClosureHistorySection` filtra explícitamente por `revertedAt !== null`,
así que el cierre activo de un proceso cerrado solo aparece una vez, en "Cierre del proceso".

## 7. Hardening `today_utc_date`

Se eligió la **opción A** (mover la consulta exacta al repositorio), la preferencia explícita para
minimizar cualquier cambio semántico: `repositories::episode_closures::today_utc_date(conn)` ahora
ejecuta la misma consulta SQL exacta (`SELECT strftime('%Y-%m-%d','now')`) que antes vivía en
`services::episode_closures`. `services::episode_closures::close_episode` la invoca vía
`episode_closures::today_utc_date(conn)?` en el único punto donde se usaba (fallback cuando
`closed_at` no viene informado). La función privada duplicada en el servicio se eliminó.

## 8. Comportamiento antes/después

**Antes**: `services::episode_closures::today_utc_date(conn)` ejecutaba SQL directamente en la
capa de servicio.
**Después**: `services::episode_closures` delega en `repositories::episode_closures::today_utc_date`,
que ejecuta la misma consulta.
**Cambio de comportamiento**: ninguno — verificado con dos tests nuevos (sección 11).

## 9. Archivos nuevos

- `src/features/treatment-episodes/ClosureHistorySection.tsx`

## 10. Archivos modificados

- `src-tauri/src/repositories/episode_closures.rs` (nueva función `today_utc_date` + 1 test nuevo)
- `src-tauri/src/services/episode_closures.rs` (eliminada la función privada duplicada, delegación
  al repositorio + 1 test nuevo)
- `src/features/treatment-episodes/TreatmentEpisodeDetailScreen.tsx` (import + render de
  `ClosureHistorySection`, comentario de cabecera actualizado)
- `docs/episode-closure.md` (limitación "sin pantalla dedicada de historial de cierres" marcada
  como resuelta en las dos secciones donde estaba documentada)

Ningún archivo prohibido (`security/*`, `calendar/*`, `backup/*`, `db/migrations.rs`,
`db/connection.rs`) fue tocado — verificado con `git status --porcelain=2` sobre esas rutas antes
del commit (sin salida).

## 11. Tests nuevos

- `repositories::episode_closures::tests::today_utc_date_matches_sqlites_own_current_date` —
  confirma que la función movida devuelve exactamente lo mismo que una consulta directa de
  `strftime('%Y-%m-%d','now')`.
- `services::episode_closures::tests::omitting_closed_at_defaults_to_todays_utc_date` — confirma
  que `close_episode` con `closedAt` omitido sigue usando la fecha de hoy en UTC, evidencia directa
  de cero cambio de comportamiento tras el hardening.

Ningún test trivial "por contar" — ambos aportan evidencia real de la separación de capas y de la
preservación de comportamiento, como se pidió explícitamente.

## 12. Total de tests

**670/670 en verde** (668 previos + 2 nuevos). Ningún test eliminado ni debilitado.

## 13. Clippy

`cargo clippy --release --all-targets` → 0 warnings, antes y después del hardening.

## 14. Builds

`cargo build --release` limpio. `npm run build` limpio, sin errores de TypeScript.

## 15. Lint

`npm run lint` → 23 warnings, 0 errores — idéntico al baseline. `ClosureHistorySection.tsx` no
introdujo ninguna advertencia nueva (su `useEffect` no dispara `set-state-in-effect`, la única
categoría presente en el resto del proyecto, porque el `setState` ocurre dentro de un `.then()`
asíncrono, no de forma síncrona en el cuerpo del efecto).

## 16. Prueba manual

**VALIDACIÓN MANUAL PENDIENTE.** Este entorno remoto no permite ejecutar una interfaz gráfica real
(el clasificador de permisos de la sesión bloqueó explícitamente, en turnos anteriores de esta
misma conversación, tanto mover el vault existente como lanzar el binario compilado en segundo
plano bajo Xvfb — el mismo bloqueo ya declarado en el cierre de Fase 13). No se inventó ninguna
prueba manual. El escenario completo pedido (crear paciente → crear proceso → cerrar → anular →
cerrar de nuevo → abrir detalle → verificar cierre vigente + historial con fechas/motivo/resultado/
motivo de anulación → lock/unlock → reinicio completo) queda agregado a la lista acumulada de
escenarios pendientes para el futuro "Pre-V1 Manual Acceptance Test" (junto con los ya pendientes
de Plan de Seguridad y Evaluaciones) — no se ejecuta aisladamente, según tu propia instrucción de
esta microfase.

## 17. Privacidad

- Ningún contenido de un cierre (resumen, recomendaciones, motivo de anulación) se escribe en
  logs, `title`, `localStorage`, `sessionStorage`, portapapeles, URL, analytics ni telemetría —
  `ClosureHistorySection.tsx` solo mantiene el historial en estado de React, dentro del componente,
  mientras la pantalla de detalle del proceso está abierta.
- El historial solo se solicita al abrir el detalle de un proceso concreto (`useEffect` con
  `episodeId` como dependencia) — nunca en un listado general de pacientes o de procesos.

## 18. Calendar

`grep` sobre `src-tauri/src/calendar/` confirma **cero referencias** a `closure_summary`,
`closure_reason`, `outcome`, `recommendations`, `reverted_reason` ni `EpisodeClosure` — el módulo
de Calendar no fue tocado y no conoce nada de este dominio, antes ni después de esta microfase.

## 19. Backup

`src-tauri/src/backup/*` no fue modificado — no era necesario, dado que no hubo ningún cambio de
esquema. Sin regresión exhaustiva de Backup (no era el objetivo de esta microfase); se confirmó
únicamente que el archivo permanece intacto (`git status --porcelain=2` sobre esa ruta, sin
salida).

## 20. Regresión final

Repetida tras todos los cambios: 670/670 tests, clippy limpio, `cargo build --release` limpio,
`npm run build` limpio, `npm run lint` 23/0 (sin categoría nueva), `git diff --check` limpio.
Ningún test previo a esta microfase fue eliminado ni debilitado.

## 21. Limitaciones

- Prueba manual GUI pendiente (sección 16), acumulada para el Pre-V1 Manual Acceptance Test.
- El diseño de "solo mostrar cierres anulados en el historial, nunca el vigente" es una decisión de
  UX tomada en esta microfase (para no duplicar información entre `ClosureSection` y
  `ClosureHistorySection`) — coherente con tu instrucción explícita de la sección 9 del encargo,
  pero es la primera vez que se implementa, así que queda sujeta a tu revisión si prefieres una
  presentación distinta.

## 22. Deuda pendiente

Ninguna deuda nueva introducida. Las deudas ya conocidas de la auditoría de transición (23 warnings
de lint, `raw_responses` sin usar, instrumentos sin archivado, Línea temporal, React Flow,
Recharts, FTS5, validación macOS/Windows) permanecen exactamente iguales — deliberadamente no
tocadas, tal como se pidió.

## 23. Commit

`c4fb227` — `Fase 14: historial de cierres y hardening longitudinal`.

## 24. Push

Realizado a `claude/cuaderno-clinico-desktop-udijjq`. Sin force push.

## 25. Git final

```
git status --porcelain=2
```
→ un único archivo sin trackear: `Auditoria-transicion-Fase-13-siguiente-fase.md` (ajeno a esta
microfase, sin cambios). Ningún archivo de código pendiente de commitear.

- HEAD final: `c4fb227` — coincide con `origin/claude/cuaderno-clinico-desktop-udijjq`.
- Commits creados: 1.
- Push: 1, sin force.
- Migraciones: 0.
- Dependencias nuevas: 0.

---

## Auditoría breve post-fase (sin implementar nada)

- **A. Formulación textual**: sigue siendo la candidata más fuerte para la próxima vertical
  clínica — esquema ya versionado (`formulation_versions.summary_text`), sin necesidad de tocar
  `formulation_nodes`/`formulation_edges`, sin ninguna dependencia nueva.
- **B. Documentos cifrados**: sigue bloqueada por la decisión criptográfica de diseño (subclave
  derivada por HKDF del DEK, nunca reutilizar el DEK directamente) — no debe empezar sin esa
  decisión aprobada explícitamente primero.
- **C. Recordatorios internos**: candidata razonable de complejidad baja, sin notificaciones del
  sistema operativo en su V1.
- **D. Línea temporal**: no necesita tabla nueva (vista agregada de lectura), pero de menor
  prioridad que Formulación.
- **E. Validación macOS/Windows**: sigue siendo la brecha más ancha hacia una "V1 operacional" —
  nunca ejecutada en hardware real en toda la historia del proyecto.

Ninguna de las cinco se implementa en este cierre — a la espera de tu aprobación de este informe,
según la regla permanente del proyecto.
