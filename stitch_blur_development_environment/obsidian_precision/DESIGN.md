---
name: Obsidian Precision
colors:
  surface: '#111318'
  surface-dim: '#111318'
  surface-bright: '#37393f'
  surface-container-lowest: '#0c0e13'
  surface-container-low: '#1a1b21'
  surface-container: '#1e2025'
  surface-container-high: '#282a2f'
  surface-container-highest: '#33353a'
  on-surface: '#e2e2e9'
  on-surface-variant: '#bdc8d1'
  inverse-surface: '#e2e2e9'
  inverse-on-surface: '#2e3036'
  outline: '#87929a'
  outline-variant: '#3e484f'
  surface-tint: '#7bd0ff'
  primary: '#8ed5ff'
  on-primary: '#00354a'
  primary-container: '#38bdf8'
  on-primary-container: '#004965'
  inverse-primary: '#00668a'
  secondary: '#cebdff'
  on-secondary: '#381385'
  secondary-container: '#4f319c'
  on-secondary-container: '#bea8ff'
  tertiary: '#4ee6aa'
  on-tertiary: '#003825'
  tertiary-container: '#22c990'
  on-tertiary-container: '#004e35'
  error: '#ffb4ab'
  on-error: '#690005'
  error-container: '#93000a'
  on-error-container: '#ffdad6'
  primary-fixed: '#c4e7ff'
  primary-fixed-dim: '#7bd0ff'
  on-primary-fixed: '#001e2c'
  on-primary-fixed-variant: '#004c69'
  secondary-fixed: '#e8ddff'
  secondary-fixed-dim: '#cebdff'
  on-secondary-fixed: '#21005e'
  on-secondary-fixed-variant: '#4f319c'
  tertiary-fixed: '#68fcbf'
  tertiary-fixed-dim: '#45dfa4'
  on-tertiary-fixed: '#002114'
  on-tertiary-fixed-variant: '#005137'
  background: '#111318'
  on-background: '#e2e2e9'
  surface-variant: '#33353a'
typography:
  headline-lg:
    fontFamily: Geist
    fontSize: 1.75rem
    fontWeight: '600'
    lineHeight: 2.25rem
    letterSpacing: -0.025em
  headline-md:
    fontFamily: Geist
    fontSize: 1.25rem
    fontWeight: '600'
    lineHeight: 1.75rem
    letterSpacing: -0.02em
  headline-sm:
    fontFamily: Geist
    fontSize: 1rem
    fontWeight: '500'
    lineHeight: 1.5rem
    letterSpacing: -0.015em
  body-lg:
    fontFamily: Geist
    fontSize: 0.9375rem
    fontWeight: '400'
    lineHeight: 1.5rem
  body-md:
    fontFamily: Geist
    fontSize: 0.8125rem
    fontWeight: '400'
    lineHeight: 1.25rem
  body-sm:
    fontFamily: Geist
    fontSize: 0.75rem
    fontWeight: '400'
    lineHeight: 1rem
  code-lg:
    fontFamily: JetBrains Mono
    fontSize: 0.875rem
    fontWeight: '400'
    lineHeight: 1.5rem
  code-md:
    fontFamily: JetBrains Mono
    fontSize: 0.8125rem
    fontWeight: '400'
    lineHeight: 1.375rem
  code-sm:
    fontFamily: JetBrains Mono
    fontSize: 0.75rem
    fontWeight: '400'
    lineHeight: 1.125rem
  label-md:
    fontFamily: JetBrains Mono
    fontSize: 0.75rem
    fontWeight: '500'
    lineHeight: 1rem
    letterSpacing: 0.02em
  label-sm:
    fontFamily: JetBrains Mono
    fontSize: 0.6875rem
    fontWeight: '500'
    lineHeight: 0.875rem
    letterSpacing: 0.04em
  keybinding:
    fontFamily: JetBrains Mono
    fontSize: 0.625rem
    fontWeight: '600'
    lineHeight: 0.75rem
    letterSpacing: 0.05em
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  gutter: 0.5rem
  gutter-editor: 1px
  margin: 0.75rem
  margin-mobile: 0.5rem
  space-xs: 0.25rem
  space-sm: 0.375rem
  space-md: 0.5rem
  space-lg: 0.75rem
  space-xl: 1rem
  space-2xl: 1.5rem
---

## Brand & Style
The design system operates on the core thesis: "Less noise. More control." It serves high-output software engineers, systems programmers, and terminal-first developers who reject unnecessary chrome, sluggish abstractions, and visual bloat. 

Drawing heavily from Tokyo night minimalism, high-performance execution environments (Zed, Sublime Text), and structured workflow architecture (Linear), the aesthetic bridges deep obsidian darkness with razor-sharp editorial discipline. The UI evokes state-of-flow immersion, tactical command, and hyper-responsive tactile feedback. Visual architecture is achieved through razor-thin geometric containment, micro-tonal depth tiers, and disciplined hierarchy rather than heavy shadows or decorative fluff.

## Colors
The palette leverages a deep spectral hierarchy engineered specifically for low visual fatigue during prolonged sessions under low ambient light:

- **Canvas & Shell**: Deepest obsidian (`#0D0E11`) serves as the base window and gutter backdrop.
- **Surface Elevation**: Layer 1 work surfaces sit at `#14161B` (editors, panels), progressing to Layer 2 overlays and flyouts at `#1B1E26`.
- **Stroke & Separation**: Structural borders are anchored uniformly at `#2A2E39`, creating crisp single-pixel containment without harsh contrast.
- **Text & Foreground**: Primary tokens emit crisp, stark legibility at `#F1F3F9`. Secondary metadata and inactive tabs rest at `#8B949E`, while structural tokens (line numbers, gutter glyphs, guides) de-escalate to `#545D68`.
- **Strategic Chromatic Accents**:
  - **Electric Cyan (`#38BDF8`)**: Primary action, active focus ring, active cursor, and execution point.
  - **Lavender (`#A78BFA`)**: Extensions, LSP semantics, Lua runtimes, and external plugin triggers.
  - **Emerald (`#34D399`)**: Clean builds, added lines in diffs, synced Git trees.
  - **Amber (`#FBBF24`)**: LSP warnings, modified hunk indicators, pending buffers.
  - **Rose (`#F87171`)**: Syntax errors, deleted hunks, destructive states.

## Typography
The typography system uses a pairing of Geist for structural framing and contextual UI chrome, balanced against JetBrains Mono for code streams, system telemetry, file paths, and keyboard shortcuts. 

- **Geist**: Handles modal headers, settings panes, dialog prose, and primary navigation items. Its neutral geometric skeleton keeps the shell quiet and non-distracting.
- **JetBrains Mono**: Employs programmatic precision. Used strictly for status readouts, breadcrumb paths, line markers, diff stats, and keybind badges. 
- **Tabular Layouts**: All numeric displays (line numbers, Git commit hashes, latency pings, column indices) must enforce tabular figures (`font-variant-numeric: tabular-nums`) to prevent horizontal jitter during rapid editor state updates.

## Layout & Spacing
The layout relies on a fixed, multi-pane dock architecture governed by hairline boundaries. Rather than conventional responsive document layouts, the layout behaves as a rigid, tiling pane system:

- **Panels & Dividers**: Splits (editor splits, terminal panels, side rails) connect edge-to-edge separated by explicit `1px` structural borders (`#2A2E39`). Negative gutter space is set to zero across panes to maximize screen estate for code lines.
- **Shell Density**: Density is deliberately compact. Navigation headers, tab ribbons, and status bars are fixed-height micro-bands (tabs: 32px height; status bar: 24px height; breadcrumbs: 28px height).
- **Modal Centering**: Floating utility overlays (Command Palette, Quick Open, Symbol Finders) are horizontally centered, anchored 18% below the top viewport boundary, with a fixed width of `640px` and adaptive dynamic height.
- **Breakpoints & Collapsibility**:
  - Below `768px` (Compact/Tablet): Left explorer and terminal docks auto-collapse into flyover panels; the active editor pane occupies full canvas width.
  - Above `1280px` (Ultra-wide/Desktop): Enables multi-column split views with minimum pane widths of `380px`.

## Elevation & Depth
This design system enforces depth through micro-tonal surface layering and 1px ghost borders rather than standard ambient drop shadows. Floating layers utilize deep background filtration:

- **Base Layer (Canvas)**: `#0D0E11` sits beneath all components (gutters, empty pane space, window chrome).
- **Workspace Layer**: `#14161B` isolates active code files, terminal panes, and settings documents.
- **Overlay Layer**: Floating palettes, tooltips, autocompletion menus, and context menus render at `#1B1E26` with a solid `1px` stroke of `#2A2E39`.
- **Glass / Scrim Attributes**: Command Palette and Quick Open overlays invoke a subtle backdrop blur (`backdrop-filter: blur(12px)`) accompanied by a 40% opacity obsidian scrim (`#0D0E1166`).
- **Focus Elevation**: Active or focused elements are not elevated via dropshadow; they claim priority through a sharp, zero-offset inner boundary illumination using `#38BDF8` at 1px border width.

## Shapes
Geometry is clipped, precise, and intentionally razor-sharp. Roundedness level `1` (Soft: 0.25rem / 4px base radius) is selectively utilized:

- **Editor Panes, Tabs, and Dividers**: Strict 0px radius (`rounded-none`). These elements align flush against window borders and partition lines.
- **Micro UI (Badges, Buttons, Tooltips, Inputs)**: 4px radius (`rounded-sm`). Softens tactile elements without introducing soft or playful aesthetics.
- **Keybinding Badges (`<kbd>`)**: 3px radius with an inner 1px bounding stroke to simulate precision mechanical keycaps.
- **Command Palette & Popovers**: 6px radius (`rounded-md`), bounded by `#2A2E39`.

## Components

### Razor-Sharp Tabs
- **Container**: 32px height, horizontal row bounded at bottom by a 1px `#2A2E39` line.
- **Inactive Tab**: Background transparent, label in `body-sm` (`#8B949E`), hover shifts background to `#14161B` and text to `#F1F3F9`.
- **Active Tab**: Background `#14161B`, label in `#F1F3F9`, top border accent of 2px solid `#38BDF8`, clean 1px right border `#2A2E39`.
- **Modified State**: A 6px solid circular dot in Amber (`#FBBF24`) replaces the close button until saved.

### Command Palette & Quick Open
- **Modal Window**: 640px width, max-height 420px. Background `#1B1E26`, 6px border radius, 1px perimeter border of `#2A2E39`.
- **Input Area**: Flush 44px height header with JetBrains Mono glyph indicator (`>`), input text in `Geist 15px`, zero-outline, zero-background.
- **Item Row**: 32px height, flex row. Active item highlights with `#2A2E3966` background and a 2px left border strip of Electric Cyan (`#38BDF8`).
- **Shortcut Trailing Hint**: Keyboard shortcuts displayed via monospace micro-badges.

### Keybinding Badges (`<kbd>`)
- **Structure**: Rendered in `keybinding` typography (`0.625rem`), uppercase.
- **Styling**: Background `#0D0E11`, border 1px solid `#2A2E39`, text `#8B949E`, padding 1px 4px, border-radius 3px.

### Status Bar
- **Container**: 24px height, full-width at viewport bottom. Background `#0D0E11`, 1px top border `#2A2E39`.
- **Items**: Monospace `label-sm` (`0.6875rem`). Flex child alignment, spacing `space-lg`.
- **Indicators**:
  - Git Branch: `#8B949E` with branch icon; uncommitted changes in `#FBBF24`.
  - Diagnostics: Counter chips displaying red circle dot + error count (`#F87171`), yellow triangle dot + warning count (`#FBBF24`).
  - Encoding / Position: Tabular numbers (`Ln 142, Col 38`), text in `#545D68`.

### Diff Views & Gutters
- **Gutter Column**: 48px base width, background `#0D0E11`, right border 1px solid `#1B1E26`. Line numbers in `code-sm`, right-aligned, colored `#545D68`.
- **Hunk Additions**: Gutter strip 2px wide `#34D399`, code row background `#34D3990D`.
- **Hunk Deletions**: Gutter strip 2px wide `#F87171`, code row background `#F871710D`.
- **Inline Diffs**: Removed segments highlighted `#F8717126`, added segments `#34D39926`.

### Minimalist Git Graph
- **Node Geometry**: 6px solid dots linked by 1.5px vertical and diagonal bezier strokes.
- **Branch Palette**: Dynamic allocation cycling through `#38BDF8`, `#A78BFA`, `#34D399`, and `#FBBF24`.
- **Commit Line**: Monospace 7-character hash in `#8B949E`, commit message in `body-sm` `#F1F3F9`, author timestamp right-aligned in `#545D68`.

### Buttons & Input Fields
- **Primary Button**: Solid Electric Cyan (`#38BDF8`), text `#0D0E11` font-weight 600, 28px height, 4px radius, hover brightness 1.1.
- **Ghost/Tertiary Button**: Background transparent, border 1px solid `#2A2E39`, text `#F1F3F9`, hover background `#1B1E26`.
- **Text Inputs**: Background `#0D0E11`, border 1px solid `#2A2E39`, text `#F1F3F9`, placeholder `#545D68`. On focus: border color transitions to `#38BDF8`.