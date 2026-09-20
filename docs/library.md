# Biblioteca global de recursos (fase de continuación post-Fase 19)

Documento técnico de esta fase. Complementa `docs/documents.md` (Fase 16, cifrado de archivos) y
`docs/ARCHITECTURE.md`.

## 1. Objetivo y alcance

Una Biblioteca **global**, distinta de la pestaña "Documentos" de cada paciente: un recurso
(artículo, protocolo, escala, video, enlace) se sube **una sola vez** y puede asociarse a
cualquier número de pacientes, sin duplicar nunca el archivo físico ni su cifrado.

## 2. Reutilización de infraestructura — sin segunda implementación de cifrado

`library_resources`, `library_tags` y `library_resource_tags` ya existían desde `SCHEMA_V1`, pero
sin ningún código que las usara hasta esta fase. El diseño original ya preveía exactamente esta
reutilización: `library_resources.file_document_id` referenciaba `documents(id)` desde el primer
día.

Esta fase construye sobre esa base, sin crear ninguna tabla de archivos nueva ni ninguna
implementación de cifrado nueva:

- El archivo de un recurso, si tiene uno, es una fila de `documents` con **`patient_id = NULL`**
  (columna nullable desde `SCHEMA_V1`, `ON DELETE SET NULL` — nunca usada con `NULL` hasta ahora).
  `repositories::documents::NewDocumentRow.patient_id` pasó de `&str` a `Option<&str>` para
  reflejarlo en Rust; `services::documents::create_document` (Documentos por paciente) sigue
  pasando siempre `Some(...)`, sin ningún cambio de comportamiento para esa fase ya aprobada.
- `services::library` reutiliza directamente `services::document_crypto::*` (generar DEK,
  cifrar/descifrar, ruta física opaca) — el mismo formato de archivo `CCD1`, el mismo límite de
  tamaño, la misma verificación de integridad por hash.
- `services::library` reutiliza también las utilidades pequeñas de `services::documents`
  (`sha256_hex`, `guess_mime_from_extension`, `wrapped_key_to_columns`, marcadas `pub(crate)`)
  en vez de duplicarlas.

## 3. Modelo de datos: migración `SCHEMA_V12`

Puramente aditiva — agrega la única pieza que faltaba, la relación N:M con pacientes:

```sql
CREATE TABLE library_resource_patients (
  resource_id TEXT NOT NULL REFERENCES library_resources(id) ON DELETE CASCADE,
  patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
  linked_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (resource_id, patient_id)
);
CREATE INDEX idx_library_resource_patients_patient ON library_resource_patients(patient_id);
```

Un recurso puede asociarse a cualquier número de pacientes y un paciente puede tener cualquier
número de recursos asociados — nunca se duplica la fila de `documents` ni su ciphertext: la
relación N:M vive exclusivamente en esta tabla de enlace.

## 4. "Eliminar" un recurso: archivado reversible, nunca borrado físico

A diferencia de lo que un `ON DELETE CASCADE` real haría, "eliminar" un recurso desde la interfaz
es un **archivado reversible** (`library_resources.deleted_at`, mismo patrón que Documentos/
Pacientes/Procesos):

- Si el recurso sigue asociado a algún paciente y no se confirma explícitamente
  (`archive_resource(id, force=false)`), se rechaza con `LinkedToPatients(n)` — el frontend
  muestra ese aviso (incluye cuántos pacientes) y solo reintenta con `force=true` si la usuaria
  confirma.
- Incluso con `force=true`, las filas de `library_resource_patients` **nunca se tocan** — el
  recurso pasa a archivado, restaurable en cualquier momento con sus asociaciones intactas. Nunca
  hay una vía, con o sin confirmación, que borre una asociación en silencio.

## 5. Reglas de paciente archivado

- **Asociar** un recurso a un paciente archivado se rechaza (`PatientArchived`) — mismo criterio
  que crear cualquier otro contenido clínico nuevo en el resto de la aplicación.
- **Desasociar** un recurso de un paciente, o simplemente **consultar** sus recursos asociados,
  sigue siempre permitido sin importar el estado de archivado — mismo criterio que el resto del
  proyecto nunca bloquea deshacer una asociación ya existente.

## 6. Backup / Restore — sin ningún cambio de código

`backup::service::create_backup` enumera los `storage_path` a respaldar vía
`repositories::documents::list_all_storage_paths`, que **nunca filtró por `patient_id`** — así que
el archivo de un recurso de Biblioteca (fila de `documents` con `patient_id = NULL`) queda incluido
automáticamente en cada backup, sin ningún cambio en `backup::service`. La tabla
`library_resource_patients` (las asociaciones) vive dentro del propio `vault.db`, incluida
íntegramente por el mismo `VACUUM INTO` que ya respaldaba el resto del esquema.

Verificado con dos tests dedicados: inclusión del archivo en el manifiesto
(`backup_includes_a_library_resources_file_automatically`) y restauración completa de un recurso,
su archivo, y su asociación con un paciente
(`restore_recovers_a_library_resource_its_file_and_its_patient_association`).

## 7. Comandos Tauri

`create_library_resource`, `get_library_resource`, `list_library_resources`,
`list_archived_library_resources`, `update_library_resource_metadata`, `archive_library_resource`
(con `force`), `restore_library_resource`, `get_library_resource_data_url` (imágenes, en memoria),
`open_library_resource_externally` (resto de formatos, temporal opaco), `export_library_resource`,
`link_library_resource_to_patient`, `unlink_library_resource_from_patient`,
`list_patients_for_library_resource`, `list_library_resources_for_patient`.

## 8. Frontend

- **Pantalla global "Biblioteca"** (`src/features/library/LibraryScreen.tsx`, ruta `/library`,
  enlace en la barra de navegación principal junto a Estadísticas): listar/buscar (cliente, sobre
  la lista ya cargada)/ordenar (título/fecha/tamaño)/agregar (archivo opcional — un recurso puede
  ser solo un enlace)/editar/archivar-restaurar/abrir-previsualizar/exportar copia, y gestionar qué
  pacientes están asociados desde el propio recurso.
- **Sección "Biblioteca" de la ficha del paciente** (`PatientLibrarySection.tsx`): lista los
  recursos ya asociados a ese paciente, con abrir y desasociar; permite asociar un recurso
  existente buscándolo por título/autor. Nunca sube un archivo nuevo desde aquí — ese flujo vive
  únicamente en la pantalla global, para no duplicar el punto de importación.

## 9. Privacidad

Igual criterio que Documentos: nada de este dominio se registra en logs,
`localStorage`/`sessionStorage`, `document.title` ni telemetría. El contenido descifrado nunca se
cachea ni se escribe en plano dentro del vault — solo temporales opacos fuera de él al "abrir"
(limpiados por el mismo `DocumentTempRegistry` de Documentos), o el destino explícito elegido por
la usuaria al "exportar copia".

## 10. Tests

73 tests nuevos de backend: 6 de migración (`db::migrations`, tabla nueva + cascadas + FK check +
idempotencia), 12 de repositorio (`repositories::library`), 13 de servicio (`services::library`,
incluye archivado con/sin `force`, paciente archivado, asociación/desasociación), y 2 de
backup/restore dedicados a Biblioteca. Suite completa: 841/841, `cargo clippy` limpio.

## 11. Limitaciones conocidas

- No hay tags (`library_tags`/`library_resource_tags` siguen sin usarse — quedan disponibles para
  una fase futura si se decide exponerlos).
- Búsqueda y orden son en cliente sobre la lista ya cargada — sin paginación en el backend.
  Razonable para el volumen esperado de una biblioteca clínica; se reevaluaría si el volumen real
  lo justifica.
- No hay versión de "subir un archivo nuevo" para un recurso ya existente — el archivo, igual que
  en Documentos, es inmutable una vez creado el recurso.

## 12. BUG WIN-DB-02 (validación real Windows) — causa raíz y corrección

La validación manual real en Windows sobre un vault **preexistente** (creado antes de `SCHEMA_V12`)
encontró que la Biblioteca global funcionaba parcialmente (listar, buscar, agregar, adjuntar un
archivo ficticio) pero la sección "Biblioteca" de la ficha de un paciente mostraba "error interno al
acceder a la base de datos" y luego quedaba en "Cargando…"; "Asociar un recurso existente" aceptaba
texto pero nunca completaba la asociación.

**Causa raíz demostrada — la misma que BUG WIN-DB-01** (ver `docs/safety-plan.md` §21): el flujo
real de desbloqueo de un vault existente (`security::vault_manager::unlock_vault`/`recover_access`)
nunca ejecutaba `db::run_migrations`, así que un vault creado antes de `SCHEMA_V12` nunca llegaba a
tener la tabla `library_resource_patients`. La Biblioteca **global** seguía funcionando porque
`library_resources` existe desde `SCHEMA_V1` — nunca dependió de la migración faltante. La
asociación **por paciente** sí depende exclusivamente de `library_resource_patients` (`SCHEMA_V12`),
de ahí que fuera precisamente esa parte, y solo esa parte, la que fallaba con "no such table:
library_resource_patients" detrás del mensaje genérico.

**Corrección**: la misma de WIN-DB-01 — `unlock_vault`/`recover_access` ahora corren
`db::run_migrations` antes de devolver la conexión. No hay ningún cambio de esquema ni ninguna
migración nueva específica de Biblioteca: `SCHEMA_V12` ya era correcta, simplemente nunca llegaba a
ejecutarse sobre un vault reabierto.

**Test de regresión**: `security::vault_manager::tests::unlock_vault_upgrades_an_existing_v10_vault_and_makes_its_data_reachable`
reproduce el caso real completo (vault `V10` con un paciente y un recurso de Biblioteca ya
existentes, cerrado y reabierto vía la función real `unlock_vault`) y verifica que
`services::library::link_resource_to_patient`/`list_resources_for_patient` funcionan sobre esa misma
conexión — ver el detalle completo en `docs/safety-plan.md` §21 (mismo test cubre ambos bugs, porque
es la misma causa raíz).

**Riesgo residual**: ninguno sobre integridad de datos. Queda pendiente la validación manual real en
Windows sobre el vault preexistente que originó el reporte.

## 13. Borrado permanente de recursos (FASE 2A, autorizada explícitamente)

Hasta esta fase, "Eliminar" un recurso era siempre `archive_resource` (reversible). Esta fase
agrega un borrado físico e irreversible, explícitamente separado del archivado y accesible
únicamente desde "Archivados" (nunca un botón destructivo junto a "Abrir" en un recurso activo).

**Requisitos, ambos validados en el servidor antes de tocar nada**:

1. **El recurso debe estar archivado.** `services::library::hard_delete_resource` rechaza con
   `LibraryError::MustBeArchivedFirst` si `deleted_at IS NULL` — reforzado también a nivel de SQL
   (`repositories::library::hard_delete` incluye `WHERE deleted_at IS NOT NULL`).
2. **Cero asociaciones con pacientes.** A diferencia de `archive_resource` (que acepta un `force`
   tras avisar), aquí **no existe** un `force`: un borrado físico nunca puede dejar una fila de
   `library_resource_patients` apuntando a un recurso que ya no existe. Si `count_patients_for_resource
   > 0`, se rechaza con `LibraryError::LinkedToPatientsBlocksHardDelete(n)` — el frontend muestra el
   conteo y un enlace a "Ver pacientes asociados" para desvincular primero.

**Atomicidad**: si el recurso tiene un archivo (`file_document_id`), su fila de `documents` y la
fila de `library_resources` se borran dentro de una única transacción SQL manual (`BEGIN IMMEDIATE`
… `COMMIT`, `ROLLBACK` ante error). El ciphertext se borra del disco **después** de que la
transacción confirma, en modo best-effort — mismo modelo que el borrado permanente de pacientes
(ver `docs/patients-vertical.md`): en el peor caso queda un ciphertext huérfano detectable por
`services::documents::find_orphan_storage_paths`, nunca una fila de base apuntando a un archivo
inexistente ni un estado a medias.

**Nunca afecta otros recursos** ni ningún documento privado de un paciente — el `document_id`
borrado siempre pertenece exclusivamente a ese recurso (`patient_id = NULL`, ver §1).

**Tests** (`services::library::tests`): rechazo sobre recurso activo; bloqueo mientras sigue
asociado (con el conteo exacto); éxito tras desvincular; borrado de un recurso sin archivo; borrado
de un recurso con archivo (verifica que el ciphertext desaparece del disco y que no queda ninguna
fila huérfana en `documents`); y que borrar un recurso nunca afecta a otro.

## 14. Exportar/Imprimir (nota de alcance)

La Biblioteca no tiene una función de exportación propia — "Exportar copia" (§ existente) ya
entrega el archivo original descifrado a un destino elegido por la usuaria. La función de
exportar/imprimir con generación de PDF vía el diálogo nativo del sistema operativo (FASE 3) es
exclusiva del Plan de Seguridad — ver `docs/safety-plan.md` §22.
