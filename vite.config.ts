import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// Configuración alineada con los requisitos de Tauri:
// https://v2.tauri.app/start/frontend/vite/
const host = process.env.TAURI_DEV_HOST

export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],

  // Evita que Vite oculte los logs de Rust en la consola de `tauri dev`.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    watch: {
      // No recompilar el frontend por cambios en el backend Rust.
      ignored: ['**/src-tauri/**'],
    },
  },
}))
