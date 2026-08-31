# Sistema visual y design tokens (Fase 1.7)

Documento técnico de la Fase 1.7. Complementa `docs/ARCHITECTURE.md` sección 14 (identidad
visual, aprobada el 31 de agosto de 2026) y `CLAUDE.md` regla 8. Esta fase estableció la base
visual definitiva de Cuaderno Clínico — de aquí en adelante, toda pantalla nueva consume estos
tokens; ninguna pantalla nueva debe volver a escribir un color de la paleta por defecto de
Tailwind directamente.

## 1. Dónde viven los tokens

`src/index.css`, bloque `@theme` (sintaxis CSS-first de Tailwind v4 — no hay `tailwind.config.js`
en este proyecto). Cada `--color-*` ahí definido se convierte, automáticamente:

1. en una variable CSS real disponible en `:root` (`var(--color-accent)`, etc.), y
2. en las utilidades de Tailwind correspondientes (`bg-accent`, `text-accent`, `border-accent`,
   `ring-accent`, `accent-accent` para `accent-color` nativo de checkboxes/radios, etc.).

Los componentes consumen exclusivamente estas utilidades. No hay un color hexadecimal ni una
clase de la paleta por defecto de Tailwind (`slate-*`, `emerald-*`, `red-*`, `amber-*`, etc.)
en ningún archivo de `src/` a partir de esta fase.

## 2. Mapeo: concepto requerido → variable → utilidad → valor

| Concepto (pedido en la Fase 1.7) | Variable CSS | Utilidad principal | Valor |
|---|---|---|---|
| Background | `--color-background` | `bg-background` | `#F7F5F1` |
| Surface | `--color-surface` | `bg-surface` | `#FDFCFA` |
| Surface elevated | `--color-surface-elevated` | `bg-surface-elevated` | `#FFFFFF` |
| Text primary | `--color-foreground` | `text-foreground` | `#2A2622` |
| Text secondary | `--color-muted-foreground` | `text-muted-foreground` | `#6B6459` |
| Border | `--color-border` | `border-border` | `#D6D0C2` |
| Accent | `--color-accent` | `bg-accent` / `text-accent` / `border-accent` | **`#2D5128`** |
| Accent hover | `--color-accent-hover` | `hover:bg-accent-hover` | `#24401F` |
| Accent active | `--color-accent-active` | `active:bg-accent-active` | `#1C3218` |
| Success | `--color-success` | `bg-success` / `text-success` | `#2A6640` |
| Warning | `--color-warning` | `bg-warning` / `text-warning` | `#7A5313` |
| Danger | `--color-danger` | `bg-danger` / `text-danger` | `#B3403A` |
| Focus | `--color-focus` | `focus:ring-accent` + `:focus-visible` global | `#2D5128` (mismo que accent) |
| Disabled | `--color-disabled` / `--color-disabled-foreground` | `disabled:bg-disabled` / `disabled:text-disabled-foreground` | `#EDEBE6` / `#8F8879` |

Los nombres de variable (`foreground`/`muted-foreground` en vez de `text-primary`/`text-secondary`,
`border` en vez de un nombre distinto) siguen la convención ya extendida en el ecosistema
Tailwind/shadcn — se eligieron así para que la utilidad generada sea legible (`text-foreground`,
no `text-text-primary`), no para renombrar el concepto: la tabla de arriba es la fuente de verdad
de la equivalencia.

**Tokens derivados adicionales** (no pedidos explícitamente, agregados por coherencia y
documentados para que no se interpreten como una paleta de identidad paralela):

- `--color-accent-soft` (`#E8EDE6`) y `--color-accent-foreground` (`#FFFFFF`): fondo tenue para
  filas/estados seleccionados, y color de texto sobre un fondo `accent`.
- `--color-success-soft`, `--color-warning-soft`, `--color-danger-soft`: fondos tenues para
  banners/alertas (p. ej. el aviso de "paciente archivado"), con su color de texto
  correspondiente encima ya verificado para contraste (sección 4).

## 3. El verde de identidad

`--color-accent: #2D5128` es el único verde de marca. Se usa en:

- botones primarios (`Button` variante `primary`);
- pestaña/tab activa (Activos/Archivados, secciones de la ficha de paciente);
- foco de teclado (`:focus-visible` global + `focus:ring-accent` en inputs);
- el punto decorativo junto al wordmark "Cuaderno Clínico" en el header;
- el `accent-color` nativo de checkboxes (p. ej. "Ya guardé mi código de recuperación").

Deliberadamente **no** se usa como color de fondo masivo de ninguna pantalla — el fondo de la
aplicación es `background`/`surface`, neutro y cálido. Ver sección 5 (verificación visual) para
capturas que muestran esta proporción real.

## 4. Accesibilidad — contraste verificado

Se calcularon los ratios de contraste WCAG 2.1 (luminancia relativa real, no una aproximación)
para cada combinación de texto/fondo que la interfaz usa hoy:

| Primer plano | Fondo | Ratio | Cumple AA texto normal (4.5:1) |
|---|---|---|---|
| `foreground` | `background` | 13.79:1 | Sí (AAA) |
| `foreground` | `surface` | 14.64:1 | Sí (AAA) |
| `muted-foreground` | `background` | 5.37:1 | Sí |
| `muted-foreground` | `surface` | 5.70:1 | Sí |
| `accent` | `background` | 8.32:1 | Sí (AAA) |
| `accent` | `surface` | 8.83:1 | Sí (AAA) |
| `accent` | blanco | 9.05:1 | Sí (AAA) |
| blanco | `accent` (texto de botón) | 9.05:1 | Sí (AAA) |
| blanco | `accent-hover` | 11.49:1 | Sí (AAA) |
| blanco | `accent-active` | 13.83:1 | Sí (AAA) |
| `success` | `background` / `surface` | 6.28 / 6.66 | Sí |
| blanco | `success` | 6.83:1 | Sí |
| `warning` | `background` / `surface` | 6.27 / 6.66 | Sí |
| `warning` | `warning-soft` (banner) | 6.19:1 | Sí |
| `danger` | `background` / `surface` | 5.20 / 5.52 | Sí |
| blanco | `danger` | 5.66:1 | Sí |
| `success` | `success-soft` | 5.87:1 | Sí |
| `danger` | `danger-soft` | 4.86:1 | Sí |

Ninguna combinación de texto usada en la aplicación cae por debajo de 4.5:1. En particular, se
verificó explícitamente el requisito de la Fase 1.7 de que **`#2D5128` nunca se use como texto
sobre un fondo de contraste insuficiente** — el peor caso (`accent` sobre `background`) da 8.32:1,
muy por encima del mínimo.

`border` (`#D6D0C2`) sobre `background`/`surface` da un contraste bajo (~1.3:1) **a propósito**:
es un divisor decorativo, no el único medio para percibir un límite (la diferencia de fondo entre
`surface` y `background`, más el radio/sombra de las tarjetas, ya comunican el límite). Los
elementos interactivos (inputs, botones) no dependen de ese borde de reposo para su affordance:
tienen además el anillo de foco de `accent`, que si cumple los 3:1 exigidos para indicadores de
foco (8.3:1 medido).

`disabled-text` sobre `disabled-bg` (2.95:1) es intencionalmente bajo — WCAG exime explícitamente
a los elementos deshabilitados del contraste mínimo de texto, porque no son interactivos; el
valor elegido es igualmente el mínimo necesario para que el texto siga siendo perceptible, no
invisible.

## 5. Foco de teclado

Además del anillo de foco por componente (`focus:border-accent focus:ring-1 focus:ring-accent`
en inputs/selects), se agregó una regla global en `src/index.css`:

```css
:focus-visible {
  outline: 2px solid var(--color-focus);
  outline-offset: 2px;
}
```

Esto garantiza un indicador de foco visible en **cualquier** elemento enfocable por teclado
(botones, links, checkbox) aunque un componente nuevo no reimplemente el anillo explícitamente —
es una red de seguridad de accesibilidad a nivel de aplicación, no por componente.

## 6. Preparado para tema oscuro (no implementado)

Como todos los colores son variables CSS reales en `:root` (no valores fijos repartidos en cada
componente), un futuro tema oscuro es, en principio, **redefinir el mismo bloque `@theme` bajo un
selector `@media (prefers-color-scheme: dark)` o una clase `.dark`** — ningún componente necesita
tocarse, porque todos consumen `var(--color-*)` indirectamente a través de las utilidades. Esta
fase no implementa esa segunda paleta (no se pidió, y hacerlo sin necesidad real sería
sobre-ingeniería); solo se dejó la arquitectura en una forma que no bloquea agregarla después.

## 7. Archivos modificados

Ver el informe de la Fase 1.7 en el mensaje de cierre de la fase (git log) para la lista completa
de archivos de `src/` actualizados. En resumen: `src/index.css` (tokens), los 4 primitivos de
`src/components/ui/`, las 6 pantallas/componentes de `src/features/auth/`, las 5
pantallas/componentes de `src/features/patients/`, y `src/App.tsx` / `src/app/Layout.tsx`. Cero
archivos de `src-tauri/` (Rust) tocados — esta fase es exclusivamente frontend.

## 8. Verificación manual realizada

Compilado con `npx tauri build --no-bundle --debug`, ejecutado bajo Xvfb con interacción real de
mouse/teclado, capturas de pantalla en cada paso. Se revisó específicamente:

- Pantalla de bloqueo/desbloqueo (`UnlockScreen`).
- Pantalla de creación de vault (`CreateVaultScreen`) y código de recuperación
  (`RecoveryCodeScreen`), incluyendo el estado `disabled` del botón "Continuar" antes de marcar
  el checkbox.
- Medidor de fortaleza de contraseña con los tres estados (débil/aceptable/fuerte →
  danger/warning/success).
- Listado de pacientes, pestañas Activos/Archivados con el indicador de pestaña activa en
  `accent`.
- Ficha de paciente, con y sin el banner de "archivado" (`warning-soft`).
- Diálogos de confirmación de archivar/restaurar.
- Formulario de paciente con un campo enfocado, confirmando visualmente el anillo de foco en
  `accent`.
- Ciclo completo archivar → ver en "Archivados" → restaurar → bloquear → desbloquear, confirmando
  que ninguna funcionalidad de la Fase 1.5/1.6 se rompió con el cambio visual.

El vault de prueba usado ya existía de la verificación manual de la Fase 1.6 (dato ficticio
"Paciente de Prueba Uno"); se protegió con un respaldo temporal (movido, no borrado) mientras se
verificaba la pantalla de creación de vault con un vault nuevo separado, y se restauró
exactamente a su estado original al terminar — sin pérdida de datos.
