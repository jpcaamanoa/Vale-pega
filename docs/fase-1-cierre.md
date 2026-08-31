# Fase 1.8 — Cierre técnico de Fase 1, regresión y auditoría (31 de agosto de 2026)

Documento de cierre de toda la Fase 1 (1.1–1.8). No se implementó funcionalidad nueva en esta
fase: es exclusivamente auditoría, regresión y documentación. Complementa
`docs/ARCHITECTURE.md`, `CLAUDE.md` y los documentos técnicos de cada fase anterior
(`sqlcipher.md`, `db-schema.md`, `security.md`, `patients-vertical.md`, `design-tokens.md`).

## 0. Punto de partida verificado antes de tocar nada

- Último commit estable: `ed68c38` (Fase 1.7), working tree limpio, un solo branch
  (`claude/cuaderno-clinico-desktop-udijjq`), historial lineal sin ramas divergentes desde
  `d34b393`.
- `cargo test` (114/114), `cargo clippy --all-targets` (sin advertencias) y `npm run build`/
  `npm run lint` confirmados en verde **antes** de iniciar cualquier revisión, para tener una
  línea base innegable de "esto ya funcionaba" contra la cual comparar el cierre.
- Se comparó el plan original de `docs/ARCHITECTURE.md` sección 11 contra lo realmente
  construido (ya documentado en la nota agregada en la Fase 1.7): el contenido real de 1.5–1.7
  divergió del plan de nombres, pero no de la arquitectura. La Fase 1.8 real difiere también de
  su descripción original ("Suite de pruebas y validación cruzada mac/Windows"): **no hay
  máquina macOS ni Windows disponible en este entorno**, así que esta fase es una auditoría
  técnica completa de arquitectura/seguridad/regresión sobre Linux, no una validación cruzada
  física — eso se deja explícitamente pendiente (sección 6).

## 1. Qué se verificó (checklist completo)

| Área | Resultado |
|---|---|
| Tauri + React + TS + Tailwind arrancan y compilan | 🟢 Verificado (`npm run build`, `cargo clippy`, build real de Tauri) |
| React → IPC → Rust, sin SQL en React | 🟢 Verificado por grep (`SELECT/INSERT/UPDATE/DELETE` en `src/`: 0 resultados) |
| Separación repositories/services/commands | 🟢 Verificado leyendo los 3 módulos de `patients`; cada capa solo conoce la de abajo |
| SQLCipher: apertura/bloqueo del vault | 🟢 `open_vault` sigue verificando `PRAGMA cipher_version` + lectura real de `sqlite_master`; sin ruta a SQLite sin cifrar |
| Envelope encryption y manejo de claves | 🟢 DEK/KEK-contraseña/KEK-recuperación sin cambios desde la Fase 1.4; parámetros de Argon2id guardados por envoltorio (compatibilidad futura, ver `vault_meta.rs`) |
| Zeroización y ausencia de secretos en logs | 🟢 `VaultKey`, `Kek`, `RecoveryCode` redactan `Debug` y zeroizan en `Drop` (verificado leyendo las 3 implementaciones); `grep` de `log::`/`println!`/`dbg!` en código de negocio: 0 resultados; log real de la app inspeccionado (ver sección 4) — una sola línea técnica, sin datos |
| Soft delete y restauración | 🟢 `soft_delete`/`restore` sin `hard_delete` en ningún punto del código; verificado con prueba manual real (crear→editar→archivar→restaurar→cerrar app→reabrir) |
| Migraciones — regla de no modificar una ya publicada | 🟢 Verificado con `git log`/`git show` sobre `migrations.rs`: el único cambio posterior a la Fase 1.3 (en la Fase 1.4) fue eliminar un comentario y un atributo `#[allow(dead_code)]` — el texto de `SCHEMA_V1` no cambió un carácter |
| Constraints e integridad de la base de datos | 🟢 Sin cambios desde 1.3; 25 tablas, FKs, CHECKs e índices intactos, cubiertos por los mismos tests |
| Versionado de `session_notes` | 🟢 Esquema sin tocar: los `CHECK` de consistencia (`is_locked`↔`closed_at`, `is_current`↔`superseded_at`) y el índice único parcial siguen exactamente como en la Fase 1.3 (la funcionalidad de UI sobre esto todavía no se construye — corresponde a una fase clínica futura) |
| Design tokens y uso de `#2D5128` | 🟢 Confirmado en `src/index.css` (`@theme`) y visualmente en la app real |
| Ausencia de colores hardcodeados fuera de tokens | 🟢 `grep` de `slate-/emerald-/red-/amber-/indigo-/blue-/green-/gray-/zinc-/neutral-` en `src/`: 0 resultados |
| CSP | 🟢 Sin cambios: `default-src 'self'`, sin `connect-src` externo |
| Ausencia de `localStorage`/`sessionStorage`/persistencia insegura | 🟢 `grep` en `src/`: 0 resultados; adicionalmente se inspeccionaron en disco `CacheStorage`/`WebKitCache`/`storage` del WebView real y no contienen ningún dato clínico (ver sección 4) |
| Ausencia de SQL directo desde React | 🟢 Confirmado (mismo grep de arriba) |
| Ausencia de comandos Tauri genéricos tipo `run_sql` | 🟢 Cada comando (`create_patient`, `list_patients`, etc.) es una operación con nombre y forma fijos; no existe ningún comando que reciba una consulta arbitraria |
| Datos ficticios únicamente en tests/demo | 🟢 Ningún seed ni dato de ejemplo incorporado al binario; la verificación manual de esta fase usó un vault de prueba separado, con datos ficticios, y se restauró/limpió al terminar (ver sección 5) |
| Estado de Git y reversibilidad por fase | 🟢 10 commits, uno por fase (o su corrección puntual), todos alcanzables con `git checkout`/`git log`; ninguno reescrito |

## 2. Multiplataforma — verificado sin implementar nada nuevo

No se tocó ningún código relacionado con plataforma en esta fase. Se confirmó que:

- No existe ningún código específico de macOS/Windows todavía (sin `keyring`, sin
  `cfg(target_os = ...)`, sin integración de Keychain/Credential Manager) — es decir, no hay nada
  que *simule* soporte para una plataforma no probada; lo que no está implementado, simplemente
  no está, de forma honesta.
- `docs/sqlcipher.md` y `docs/security.md` ya declaraban explícitamente que macOS y Windows no se
  han compilado ni probado en una máquina real — esto sigue siendo cierto hoy. **Se reafirma
  explícitamente:** todo lo verificado en esta fase (incluida toda la prueba manual) se ejecutó
  en Linux (Xvfb). Ni macOS ni Windows ni iOS/iPadOS se probaron físicamente en ningún momento de
  la Fase 1.
- Nada de lo construido depende de una API exclusiva de una plataforma: Tauri (que sí soporta
  destinos móviles sobre el mismo core), SQLCipher, Argon2id/AES-GCM (RustCrypto, puro Rust, sin
  dependencia de una API criptográfica del sistema operativo) y React son multiplataforma por
  diseño desde la Fase 1.1.
- La arquitectura de envelope encryption (DEK envuelto por múltiples KEK independientes) sigue
  siendo compatible con un futuro modelo de "dispositivos autorizados" (`docs/ARCHITECTURE.md`
  sección 15.C) sin haber cerrado esa puerta.
- No se encontró ninguna decisión de esta fase, ni de fases anteriores, que bloquee
  innecesariamente una futura sincronización local-first + E2EE.

## 3. Modo WAL

Investigado sin activar, como se pidió explícitamente. Conclusión completa y recomendación en
`docs/db-schema.md` (sección "Decisión explícitamente diferida", revisión de la Fase 1.8):
**se mantiene diferido a la Fase 7 (backup)**, sin cambiar la estrategia ni el código. El motivo
real ya no es una duda de cifrado (SQLCipher documenta soporte completo de WAL) sino que el
diseño de backup, que decide cómo lidiar con los archivos `-wal`/`-shm`, todavía no existe.

## 4. Seguridad — hallazgos específicos

Se buscó explícitamente cada uno de los puntos pedidos. Ningún hallazgo de severidad 🔴 o 🟠.

- **Secretos en logs**: inspeccionado el archivo de log real de una ejecución completa de la
  aplicación (creación de vault, pacientes, archivar/restaurar, cierre/reapertura). Contenido
  completo: una sola línea, `[rusqlite_migration][INFO] Database migrated to version 1`. Ningún
  nombre, contraseña, clave ni dato clínico.
- **Contraseñas o claves almacenadas**: `vault.meta.json` inspeccionado — solo sales, parámetros
  de Argon2id y los dos DEK envueltos (cifrados). Ninguna clave en texto plano en disco, en
  ningún archivo del `app_data_dir`.
- **Claves fuera de los mecanismos previstos**: no existe ningún otro lugar donde el código
  escriba una clave — no hay integración de keychain/Credential Manager todavía (diferido a Fase
  7, según lo aprobado).
- **Datos clínicos enviados al frontend innecesariamente**: el listado (`PatientListItem`) sigue
  sin campo `rut` a nivel de tipo (no es una omisión de la UI, el campo no existe en el struct);
  la ficha completa (`Patient`) solo se pide para un paciente específico que la usuaria ya está
  viendo — no hay ningún comando que traiga todos los pacientes con todos sus campos a la vez.
- **Datos clínicos a servicios externos / telemetría / analytics**: `grep` de palabras clave
  (`analytics`, `telemetry`, `sentry`, `fetch(`, `axios`, `reqwest`, etc.) en todo `src/` y
  `src-tauri/src/`: 0 resultados. No hay cliente HTTP en el árbol de dependencias de Rust.
- **Archivos clínicos en texto plano dentro del vault**: no aplica todavía — el módulo de
  documentos (`files/`, AES-256-GCM por archivo) no existe como código; es una funcionalidad de
  una fase futura, correctamente no adelantada.
- **Nombres clínicos expuestos en rutas internas**: no aplica todavía por el mismo motivo (no
  hay almacenamiento de archivos aún); cuando se implemente, el esquema ya definido en
  `documents.storage_path` usa nombres basados en UUID, no en el nombre del paciente.
- **Persistencia accidental del estado clínico en el navegador/webview**: además del `grep` de
  `localStorage`/`sessionStorage` en el código fuente, se inspeccionaron directamente en disco
  las carpetas que WebKitGTK usa como caché (`CacheStorage`, `WebKitCache`, `storage`,
  `mediakeys`) dentro del `app_data_dir` real de una sesión con un paciente ficticio creado
  ("Paciente de Prueba Uno"/"Paciente Regresion Fase 18"). Se buscó el nombre completo del
  paciente en el contenido de esas carpetas: **no aparece en ningún archivo**. El motor del
  WebView no está cacheando contenido dinámico de la aplicación.

## 5. Tests y resultados exactos

**Automatizados:**

```
cargo test           → 114 passed; 0 failed; 0 ignored
cargo clippy --all-targets → sin advertencias
npm run build         → tsc + vite build sin errores
npm run lint           → 5 advertencias preexistentes (React Compiler / set-state-in-effect),
                         las mismas desde la Fase 1.5/1.6, ninguna nueva
```

**Manuales** (aplicación real compilada con `npx tauri build --no-bundle --debug`, ejecutada bajo
Xvfb con interacción real de mouse/teclado, sobre un **vault de prueba separado** — no se
reutilizó ni se tocó el vault existente, que se protegió moviéndolo temporalmente y se restauró
exactamente a su estado original al terminar):

1. Arranque de la aplicación con vault inexistente → pantalla "Crear tu cuaderno clínico".
2. Crear vault con contraseña ficticia → código de recuperación mostrado → confirmación →
   aplicación desbloqueada.
3. Crear paciente ficticio ("Paciente Regresion Fase 18") → persistido con datos reales.
4. Editar el paciente (teléfono) → guardar → ficha refleja el cambio.
5. Archivar → desaparece de "Activos" → aparece en "Archivados".
6. Restaurar → vuelve a "Activos" con el teléfono editado intacto.
7. **Cierre completo del proceso de la aplicación** (no solo bloquear) y **reapertura real** del
   binario.
8. La app arranca en estado `Locked` (vault persistido en disco).
9. Desbloquear con la misma contraseña → paciente presente, con todos los cambios (edición,
   archivar, restaurar) sobrevividos al ciclo completo.

Ningún paso de este flujo mostró comportamiento distinto al esperado.

## 6. Problemas encontrados, clasificados

**🔴 Críticos:** ninguno.

**🟠 Importantes:** ninguno nuevo. (El único ítem 🟠 de la auditoría preventiva previa a la Fase
1.7 — la desincronización entre el plan de la sección 11 y la ejecución real — ya se corrigió en
esa misma fase.)

**🟡 Menores / deuda técnica documentada, para fases futuras:**

1. Modo WAL sin activar (sección 3) — diferido a Fase 7 con justificación, no es un defecto.
2. Ninguna plataforma salvo Linux ha sido probada físicamente — no es un defecto de esta fase,
   es una limitación real del entorno de desarrollo que debe recordarse antes de prometer
   soporte Mac/Windows/iOS como "ya probado".
3. Zustand sigue declarado como decisión técnica (sección 2 de `ARCHITECTURE.md`) pero no
   instalado ni usado — correcto por ahora (no hay estado de UI efímero que lo justifique), pero
   vale la pena revisar en la fase donde aparezca la primera necesidad real de estado compartido
   entre componentes de un mismo feature.

**🟢 Sin problema:** todo lo demás de la lista de la sección 1.

## 7. Confirmación de no retroceso

**Ninguna decisión arquitectónica aprobada se modificó en esta fase.** No se tocó:

- Ninguna migración ya publicada (`migrations.rs` no se modificó en absoluto en esta fase).
- El modelo de seguridad (envelope encryption, Argon2id, AES-GCM, zeroización) — sin cambios.
- El modelo de base de datos — sin cambios de esquema.
- Los design tokens de la Fase 1.7 — sin cambios; se verificó que siguen siendo la única fuente
  de verdad visual.
- Ninguna dependencia se agregó, quitó ni actualizó.
- Ningún dato real ni estado existente se borró — el único vault tocado en esta fase fue uno de
  prueba separado, creado y eliminado dentro de la misma fase, con datos exclusivamente
  ficticios.

Esta fase fue exclusivamente lectura, verificación, regresión y documentación.

## 8. Decisiones pendientes antes de Fase 2

Ninguna decisión bloqueante. Para que quede explícito y no se pierda:

- Modo WAL: pendiente de activar en Fase 7, con sus propios tests (sección 3).
- Validación física en macOS/Windows: pendiente de la primera vez que exista acceso a esas
  máquinas — no es una tarea de "Fase 2", pero debe quedar en el radar y no asumirse resuelta.
- Ningún otro punto requiere tu aprobación antes de continuar.

## 9. Documentación actualizada en esta fase

- `docs/ARCHITECTURE.md`: sección 17 ("Estado de avance") con la fila de la Fase 1.8 y la
  confirmación de que la Fase 1 completa (1.1–1.8) está cerrada; nota adicional en la sección 11
  aclarando que el contenido real de 1.8 difirió del plan original (auditoría técnica en vez de
  validación cruzada física, por no haber máquinas Mac/Windows disponibles).
- `docs/db-schema.md`: investigación y recomendación sobre el modo WAL (sección 3 de este
  documento).
- Este documento (`docs/fase-1-cierre.md`), como registro permanente del cierre de Fase 1.
