/**
 * Formato nativo `Intl.NumberFormat` — sin librerías de dinero. El
 * almacenamiento sigue siendo numérico (`amount: number`); esto es
 * exclusivamente de presentación, nunca se guarda un string formateado.
 * Esta fase solo admite CLP (ver `services::payments::SUPPORTED_CURRENCY`
 * en el backend), así que el formateador está fijo a CLP a propósito.
 */
const clpFormatter = new Intl.NumberFormat('es-CL', { style: 'currency', currency: 'CLP', maximumFractionDigits: 0 })

export function formatClp(amount: number): string {
  return clpFormatter.format(amount)
}
