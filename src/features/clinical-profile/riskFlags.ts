/**
 * "Factores de riesgo" se guarda internamente como el mismo texto JSON que
 * ya validaba `services::patient_clinical_profile::validate_risk_flags` en
 * el backend (ver `schema.ts`) — eso no cambia. Lo que cambia es que la
 * usuaria nunca vuelve a escribir ni ver ese JSON: la UI trabaja siempre con
 * `string[]` y estas dos funciones son el único punto de conversión.
 *
 * Compatibilidad con datos ya guardados: si el valor existente no es un
 * array JSON de strings (dato legado de antes de este cambio, o cualquier
 * otro JSON válido según la validación original, que nunca exigió una forma
 * específica), se conserva igual como un único "tag" con el texto tal cual
 * — nunca se descarta ni se corrompe silenciosamente.
 */
export function parseRiskFlags(value: string | null | undefined): string[] {
  if (!value || !value.trim()) return []
  try {
    const parsed = JSON.parse(value)
    if (Array.isArray(parsed) && parsed.every((item) => typeof item === 'string')) {
      return parsed
    }
  } catch {
    // No era JSON válido (dato legado anterior a esta validación) — cae al fallback de abajo.
  }
  return [value]
}

export function serializeRiskFlags(tags: string[]): string | null {
  if (tags.length === 0) return null
  return JSON.stringify(tags)
}
