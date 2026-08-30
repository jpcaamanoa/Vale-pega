import { invoke } from '@tauri-apps/api/core'
import type { PasswordStrength, VaultStatus } from './types'

export const authApi = {
  vaultStatus: () => invoke<VaultStatus>('vault_status'),

  evaluatePasswordStrength: (password: string) =>
    invoke<PasswordStrength>('evaluate_password_strength', { password }),

  beginVaultCreation: (password: string) => invoke<string>('begin_vault_creation', { password }),

  confirmVaultCreation: () => invoke<void>('confirm_vault_creation'),

  cancelVaultCreation: () => invoke<void>('cancel_vault_creation'),

  unlockVault: (password: string) => invoke<void>('unlock_vault', { password }),

  recoverVaultAccess: (recoveryCode: string, newPassword: string) =>
    invoke<void>('recover_vault_access', { recoveryCode, newPassword }),

  changeVaultPassword: (currentPassword: string, newPassword: string) =>
    invoke<void>('change_vault_password', { currentPassword, newPassword }),

  lockVault: () => invoke<void>('lock_vault'),

  recordVaultActivity: () => invoke<void>('record_vault_activity'),
}

/** Los comandos de seguridad devuelven `Result<_, String>`; Tauri los rechaza como string plano. */
export function errorMessage(err: unknown): string {
  return typeof err === 'string' ? err : 'Ocurrió un error inesperado.'
}
