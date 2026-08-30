/**
 * Réplica en TypeScript del algoritmo módulo 11 usado en Rust
 * (src-tauri/src/services/rut.rs) — solo para retroalimentación inmediata
 * en el formulario. La validación autoritativa es la de Rust.
 */
function computeCheckDigit(bodyDigits: number[]): string {
  let sum = 0
  let weight = 2
  for (let i = bodyDigits.length - 1; i >= 0; i--) {
    sum += bodyDigits[i] * weight
    weight = weight === 7 ? 2 : weight + 1
  }
  const remainder = 11 - (sum % 11)
  if (remainder === 11) return '0'
  if (remainder === 10) return 'K'
  return String(remainder)
}

export function isValidChileanRut(input: string): boolean {
  const cleaned = input
    .replace(/[\s.-]/g, '')
    .toUpperCase()
  if (cleaned.length < 2) return false

  const checkChar = cleaned.slice(-1)
  const bodyStr = cleaned.slice(0, -1)
  if (!/^\d+$/.test(bodyStr)) return false
  if (!/^[0-9K]$/.test(checkChar)) return false

  const bodyDigits = bodyStr.split('').map(Number)
  return computeCheckDigit(bodyDigits) === checkChar
}

export function normalizeChileanRut(input: string): string {
  const cleaned = input.replace(/[\s.-]/g, '').toUpperCase()
  return `${cleaned.slice(0, -1)}-${cleaned.slice(-1)}`
}
