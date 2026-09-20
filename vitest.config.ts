import { defineConfig } from 'vitest/config'

// Config de test aislada, sin tocar `vite.config.ts` (usado por `tauri dev`/`vite build`): los
// tests unitarios de funciones puras del frontend no necesitan el plugin de React ni Tailwind ni
// la config de servidor de Tauri, así que viven en su propio archivo para no introducir ningún
// riesgo en el build de producción.
export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
})
