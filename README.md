# Cuaderno Clínico

Cuaderno de trabajo clínico personal para psicología clínica independiente. Aplicación de
escritorio local-first (Tauri + React + TypeScript + Rust). Uso exclusivamente personal — no es
una plataforma para pacientes.

Ver `docs/ARCHITECTURE.md` para la arquitectura completa, el modelo de datos, el diseño de
seguridad y el plan de implementación por fases.

## Estado actual

**Fase 1.1 — Scaffold** completada: proyecto Tauri + React + TypeScript + Tailwind funcionando
de extremo a extremo (ventana nativa, WebView, comando Rust vía IPC, estilos Tailwind).

## Requisitos

- Node.js 20+
- Rust estable (`rustup`)
- Linux (desarrollo/CI): `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`,
  `librsvg2-dev`, `libssl-dev`, `libxdo-dev`, `build-essential` — ver
  [prerrequisitos de Tauri](https://v2.tauri.app/start/prerequisites/) para macOS/Windows.

## Desarrollo

```bash
npm install
npm run tauri dev     # levanta la app de escritorio en modo desarrollo
```

## Pruebas y verificación

```bash
npm run build                     # typecheck (tsc) + build de producción del frontend
npm run lint                      # lint del frontend (oxlint)

cd src-tauri
cargo check                       # compila el backend Rust
cargo test                        # tests unitarios de Rust
cargo clippy --all-targets        # lint de Rust
```

Build completo de escritorio (sin empaquetar instaladores):

```bash
npx tauri build --no-bundle
```
