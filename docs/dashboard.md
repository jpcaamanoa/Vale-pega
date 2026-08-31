# Dashboard y routing (Fase 2)

Documento técnico de la Fase 2. Complementa `docs/ARCHITECTURE.md` sección 13.B (Dashboard) y
13.C (ficha de paciente como centro del sistema). Implementa la pantalla de inicio real de la
aplicación, sin adelantar ninguna funcionalidad de una fase posterior.

## Qué se implementó — y qué es real vs. placeholder

El Dashboard tiene los tres bloques descritos en la sección 13.B, pero **solo uno tiene datos
reales hoy**. Esta tabla es la fuente de verdad de qué está realmente implementado:

| Bloque | Contenido en pantalla | ¿Real o placeholder? |
|---|---|---|
| **Hoy** | Explicación sobria de que se habilitará cuando exista Agenda | Placeholder — sin backend, sin datos |
| **Pendientes** | Explicación sobria de que se habilitará cuando existan Sesiones/Pagos/Documentos | Placeholder — sin backend, sin datos |
| **Resumen → Pacientes activos** | Conteo real vía `patientsApi.list()` | **Real** — mismo comando `list_patients` de la Fase 1.5, sin cambios |
| **Resumen → Sesiones del mes** | Placeholder | Placeholder — sin backend, sin datos |
| **Resumen → Ingresos del mes** | Placeholder | Placeholder — sin backend, sin datos |

Ningún bloque placeholder muestra un número, una fecha ni un estado inventado. El componente
`ComingSoonCard` (y el tratamiento equivalente inline para las dos filas de "Resumen") usa
exclusivamente el token `disabled`/`disabled-foreground` para la etiqueta "Próximamente" — el
mismo tratamiento visual de "no disponible/no interactivo" que ya se usaba en la Fase 1.7 para
estados deshabilitados, reutilizado aquí con el mismo significado semántico.

## Routing — antes y después

| Ruta | Fase 1 (hasta 1.8) | Fase 2 |
|---|---|---|
| `/` | Listado de pacientes | **Dashboard** |
| `/patients` | (no existía) | Listado de pacientes |
| `/patients/new` | Crear paciente | Sin cambios |
| `/patients/:id` | Ficha de paciente | Sin cambios |
| `/patients/:id/edit` | Editar paciente | Sin cambios |

Dos lugares del código asumían que "volver al listado" era `navigate('/')`
(`PatientCreateScreen.tsx`, botón "Cancelar"; `PatientDetailScreen.tsx`, después de archivar) —
se actualizaron a `navigate('/patients')` para preservar exactamente el comportamiento anterior
bajo el nuevo routing. Es el único cambio funcional fuera de las pantallas nuevas del Dashboard.

## Navegación

`Layout.tsx` gana dos enlaces (`Inicio` → `/`, `Pacientes` → `/patients`) en el header, con el
estado activo resaltado en `accent` (mismo patrón visual que las pestañas Activos/Archivados de
la Fase 1.6). No se agregó ningún enlace a una funcionalidad que no exista todavía.

## Archivos nuevos

- `src/features/dashboard/DashboardScreen.tsx`
- `src/features/dashboard/ComingSoonCard.tsx`

## Archivos modificados

- `src/App.tsx` — routing (`/` → Dashboard, `/patients` → listado).
- `src/app/Layout.tsx` — nav "Inicio"/"Pacientes".
- `src/features/patients/PatientCreateScreen.tsx` — `navigate('/')` → `navigate('/patients')` en
  "Cancelar".
- `src/features/patients/PatientDetailScreen.tsx` — mismo cambio, después de archivar.

Ningún archivo de `src-tauri/` (Rust) se tocó. Ningún comando Tauri nuevo. Ninguna migración
nueva. Ninguna dependencia nueva.

## Qué se dejó deliberadamente fuera (pertenece a fases posteriores)

Agenda/citas locales y Google Calendar (Fase 3), Sesiones/`session_notes` como funcionalidad de
UI, Pagos, Documentos, Formulación, Objetivos, Evaluaciones, Biblioteca, Herramientas,
Recordatorios, sincronización, IA. La existencia de las tablas correspondientes en el esquema
(desde la Fase 1.3) no autorizó construir ninguna de esas verticales en esta fase — el Dashboard
solo consume la vertical de Pacientes, que ya existía.

## Tests y verificación

`cargo test` (114/114, sin cambios — no se tocó Rust), `cargo clippy --all-targets` (sin
advertencias), `npm run build`/`npm run lint` (limpios, mismas advertencias preexistentes).

Verificación manual sobre la aplicación real compilada, con un **vault de prueba separado** (no
se tocó el vault existente): Dashboard sin pacientes → crear paciente ficticio → conteo real
actualizado a 1 → click en "Pacientes activos" navega a `/patients` → archivar (redirige
correctamente a `/patients`, no al Dashboard) → conteo del Dashboard vuelve a 0 → ver en
"Archivados" → restaurar → **cierre completo del proceso** → reapertura → arranca `Locked` →
desbloquear → aterriza en el Dashboard con el conteo real (1) persistido. Capturas de pantalla de
cada paso como evidencia.
