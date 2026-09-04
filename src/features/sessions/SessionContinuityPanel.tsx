import { PrepNotesSection } from '../prep-notes/PrepNotesSection'
import { TherapyTasksSection } from '../therapy-tasks/TherapyTasksSection'

/**
 * Continuidad entre sesiones (Fase 8): agrupa "Para próxima sesión" y
 * "Tareas entre sesiones" para que la profesional no tenga que abrir
 * manualmente la nota de la sesión anterior para recordar qué quedó
 * pendiente. Se usa tanto dentro de una sesión concreta (`sessionId`
 * presente — lo que se crea aquí queda originado/asignado en esa sesión, y
 * resolver una tarea aquí registra en qué sesión se revisó) como en la
 * pestaña "Sesiones" de la ficha del paciente, fuera de cualquier sesión
 * (`sessionId` ausente).
 */
export function SessionContinuityPanel({ patientId, sessionId, patientArchived }: { patientId: string; sessionId?: string; patientArchived: boolean }) {
  return (
    <div className="flex flex-col gap-4">
      <PrepNotesSection patientId={patientId} sessionId={sessionId} patientArchived={patientArchived} />
      <TherapyTasksSection patientId={patientId} sessionId={sessionId} patientArchived={patientArchived} />
    </div>
  )
}
