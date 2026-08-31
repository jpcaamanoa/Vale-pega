# Cuaderno Clínico — Reglas permanentes del proyecto

Estas reglas son **obligatorias para todo el proyecto**, desde el 31 de agosto de 2026 hasta el
producto final. Se aplican a toda fase futura (1.6 en adelante) sin excepción. Fueron dictadas
explícitamente por la usuaria y no se reinterpretan ni se relajan sin su aprobación expresa.

La arquitectura técnica completa vive en `docs/ARCHITECTURE.md` (léelo antes de tocar código si
no está ya en contexto). Este archivo es el complemento de **proceso**: cómo se trabaja, no qué se
construye.

## 1. Estabilidad y no retroceso

- El proyecto avanza de forma acumulativa y recuperable. Cada fase aprobada es un estado
  funcional recuperable vía Git.
- Antes de empezar una fase nueva: verificar que compila, correr los tests existentes, verificar
  que lo ya implementado sigue funcionando, y confirmar que existe un commit estable del estado
  anterior.
- No eliminar ni sobrescribir funcionalidad ya aprobada solo para facilitar una implementación
  nueva.
- Si una decisión anterior necesita cambiar: **detenerse antes de tocar el código** y explicar
  cuál es el problema, por qué la arquitectura actual no alcanza, qué se propone, qué
  archivos/tablas/componentes se verían afectados, qué riesgos introduce, y cómo se preservarían
  los datos y funcionalidades existentes. Ningún cambio arquitectónico importante sin aprobación.

## 2. Base de datos y migraciones

- La base de datos es crítica: nunca borrar, resetear, recrear ni reemplazar una base existente
  como forma de resolver un problema.
- Todo cambio de esquema futuro va por migraciones versionadas (`rusqlite_migration`), que
  preservan los datos existentes siempre que sea técnicamente posible.
- Ninguna migración destructiva sobre datos reales sin advertencia explícita y aprobación previa.
- Las pruebas cubren tanto creación de una base nueva como actualización de una base con datos ya
  existentes.
- "Borrar la base y crearla de nuevo" nunca es una solución de desarrollo aceptable si existe la
  posibilidad de migrar correctamente.

## 3. Datos clínicos reales

- Todo dato usado en desarrollo, testing, screenshots, fixtures, seeds, documentación y
  demostraciones es ficticio/sintético.
- Nunca nombres reales, RUT reales, diagnósticos reales, notas clínicas reales, documentos reales
  ni cualquier otro dato identificable de un paciente real.
- La aplicación queda preparada para datos reales, pero los datos de desarrollo nunca lo son.

## 4. Seguridad y privacidad — principios no negociables

- Local-first para todo dato clínico.
- SQLCipher para la base de datos; cifrado de documentos individuales.
- La contraseña maestra nunca se almacena en ningún formato ni lugar.
- Zeroización de secretos en memoria cuando corresponda (`zeroize`).
- Ningún dato clínico se envía a servicios externos.
- Sin analytics ni telemetría que pueda exponer información.
- Sin contenido clínico en logs.
- Sin servicios de IA sobre datos clínicos.
- Sin criptografía propia: solo librerías consolidadas (RustCrypto, etc.).
- Cualquier funcionalidad futura que envíe, sincronice, procese externamente o almacene fuera del
  perímetro local un dato clínico debe identificarse explícitamente antes de implementarse.

Ver también `docs/ARCHITECTURE.md` secciones 5, 10 y 16 (principios de privacidad consolidados).

## 5. Google Calendar

- Minimización estricta ya definida en `docs/ARCHITECTURE.md` sección 6: Google Calendar nunca
  recibe nombre, RUT, diagnóstico, motivo de consulta, notas, evaluaciones, formulación,
  documentos ni ninguna información clínica.
- Es un sistema separado del futuro sistema de sincronización de la aplicación (ver regla 6).

## 6. Backup ≠ Sync ≠ Export

Tres sistemas conceptualmente distintos (ver `docs/ARCHITECTURE.md` sección 15.E), que nunca se
combinan ni se sustituyen entre sí:

- **Backup**: recuperación ante pérdida o fallo.
- **Sync**: mantener varios dispositivos con el mismo estado de la aplicación.
- **Export**: sacar información en un formato abierto.

Una solución de backup nunca se reutiliza automáticamente como solución de sincronización. Cuando
llegue el momento de diseñar Sync, será una fase específica dedicada a: modelo de sincronización,
E2EE, gestión de dispositivos autorizados, revocación, resolución de conflictos, recuperación,
pérdida de dispositivos, privacidad, seguridad del servidor, y compatibilidad
Mac/Windows/iPhone/iPad. **Prohibido** usar last-write-wins silencioso para datos clínicos en
cualquier diseño de sincronización.

## 7. Multiplataforma

- Objetivo final: experiencia coherente en macOS, Windows, iPhone e iPad — pero esto **no está
  implementado hoy**. Tauri + React es la arquitectura actual de escritorio.
- iOS/iPadOS y la sincronización entre dispositivos requieren una fase específica posterior; no se
  implementan de forma improvisada dentro de las fases actuales.
- Un dispositivo Apple no implica iCloud automáticamente. Cualquier servicio de sincronización
  futuro se justifica y diseña específicamente desde privacidad y cifrado antes de adoptarse.

## 8. Diseño visual

- Color de acento aprobado: **`#2D5128`**, como parte de un sistema de design tokens — no como
  colores escritos arbitrariamente por todo el código.
- La migración progresiva de componentes a tokens centralizados ocurre a partir de la Fase 1.7,
  no antes (ver contradicción ya documentada en `docs/ARCHITECTURE.md` sección 14.D).
- El diseño debe sentirse profesional, sobrio, cálido, minimalista, cómodo para uso prolongado,
  clínico sin sentirse frío, y visualmente limpio.
- El verde de acento se usa en botones principales, elementos activos, estados seleccionados y
  detalles — nunca sobrecargando toda la interfaz de verde. Nada de apariencia "software médico
  antiguo" ni interfaces excesivamente coloridas.

## 9. Regresión obligatoria

Al terminar cada fase, comprobar: tests de fases anteriores, tests nuevos de la fase actual,
build frontend, build Tauri/Rust, lint, `cargo clippy` cuando corresponda, y cualquier smoke test
relevante. Nunca eliminar ni debilitar un test para que el proyecto "pase". Si un test falla
porque revela un problema real, corregir el problema o detenerse y explicar la situación —
nunca lo segundo en silencio.

## 10. Informe obligatorio al final de cada fase

Todo informe de cierre de fase debe incluir, en este orden:

1. Qué se implementó.
2. Qué archivos se modificaron.
3. Qué archivos nuevos se crearon.
4. Qué tablas/migraciones se modificaron, si corresponde.
5. Qué funcionalidades anteriores fueron afectadas.
6. Qué tests se ejecutaron y sus resultados.
7. Qué pruebas manuales se realizaron.
8. Qué riesgos o limitaciones permanecen.
9. Qué decisiones nuevas requieren aprobación.
10. El commit que representa el estado estable de la fase.

**No se avanza a la fase siguiente hasta recibir aprobación explícita de este informe.**

## 11. Regla de detenerse

Detenerse y explicar antes de actuar (nunca improvisar) si se detecta cualquiera de estos casos:

- Riesgo de pérdida de datos.
- Cambio de arquitectura.
- Cambio del modelo de seguridad.
- Cambio del modelo de base de datos.
- Introducción de una dependencia importante.
- Envío de información fuera del dispositivo.
- Modificación de una decisión previamente aprobada.
- Una funcionalidad que no puede implementarse de forma segura con la arquitectura actual.

En cualquiera de estos casos: explicar el problema y esperar aprobación antes de continuar.
