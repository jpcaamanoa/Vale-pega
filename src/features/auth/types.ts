export type VaultStatus = 'no_vault' | 'locked' | 'pending_creation' | 'unlocked'

export type StrengthLabel = 'debil' | 'aceptable' | 'fuerte'

export interface PasswordStrength {
  score: number
  label: StrengthLabel
}
