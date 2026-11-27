---
name: Quiet Precision
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
  on-surface-variant: '#c1c7ce'
  inverse-surface: '#e2e2e9'
  inverse-on-surface: '#2e3036'
  outline: '#8b9198'
  outline-variant: '#41474d'
  surface-tint: '#9eccf0'
  primary: '#9eccf0'
  on-primary: '#00344e'
  primary-container: '#6c99bb'
  on-primary-container: '#003049'
  inverse-primary: '#346382'
  secondary: '#9ed0c5'
  on-secondary: '#003730'
  secondary-container: '#1d4f46'
  on-secondary-container: '#8dbfb4'
  tertiary: '#e9c176'
  on-tertiary: '#412d00'
  tertiary-container: '#b28f4a'
  on-tertiary-container: '#3c2a00'
  error: '#ffb4ab'
  on-error: '#690005'
  error-container: '#93000a'
  on-error-container: '#ffdad6'
  primary-fixed: '#c9e6ff'
  primary-fixed-dim: '#9eccf0'
  on-primary-fixed: '#001e2f'
  on-primary-fixed-variant: '#184b69'
  secondary-fixed: '#baede1'
  secondary-fixed-dim: '#9ed0c5'
  on-secondary-fixed: '#00201b'
  on-secondary-fixed-variant: '#1d4f46'
  tertiary-fixed: '#ffdea5'
  tertiary-fixed-dim: '#e9c176'
  on-tertiary-fixed: '#261900'
  on-tertiary-fixed-variant: '#5d4201'
  background: '#111318'
  on-background: '#e2e2e9'
  surface-variant: '#33353a'
typography:
  headline-lg:
    fontFamily: Geist
    fontSize: 30px
    fontWeight: '400'
    lineHeight: 40px
    letterSpacing: -0.02em
  headline-md:
    fontFamily: Geist
    fontSize: 22px
    fontWeight: '400'
    lineHeight: 32px
    letterSpacing: -0.015em
  headline-sm:
    fontFamily: Geist
    fontSize: 18px
    fontWeight: '500'
    lineHeight: 26px
    letterSpacing: -0.01em
  body-lg:
    fontFamily: JetBrains Mono
    fontSize: 15px
    fontWeight: '400'
    lineHeight: 26px
    letterSpacing: -0.005em
  body-md:
    fontFamily: JetBrains Mono
    fontSize: 13.5px
    fontWeight: '400'
    lineHeight: 23px
    letterSpacing: 0em
  body-sm:
    fontFamily: JetBrains Mono
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 19px
    letterSpacing: 0.01em
  label-md:
    fontFamily: Geist
    fontSize: 12px
    fontWeight: '450'
    lineHeight: 16px
    letterSpacing: 0.02em
  label-sm:
    fontFamily: Geist
    fontSize: 11px
    fontWeight: '400'
    lineHeight: 14px
    letterSpacing: 0.03em
  code-inline:
    fontFamily: JetBrains Mono
    fontSize: 13px
    fontWeight: '400'
    lineHeight: 20px
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  gutter: 1rem
  gutter-mobile: 0.75rem
  margin: 2rem
  margin-mobile: 1rem
  space-xs: 0.25rem
  space-sm: 0.5rem
  space-md: 0.875rem
  space-lg: 1.5rem
  space-xl: 2.5rem
---

## Brand & Style

This design system embodies the ethos of "Less noise. More control." It draws from Japanese minimalist digital craftsmanship, austere text-centric tools (such as Acme, iA Writer, and Zed), and the disciplined calm of classic Unix terminal culture. The interface treats text and code as artifacts worthy of quiet reverence.

### Brand Personality & Audience
- **Target Audience:** Writers, systems engineers, minimalist developers, and knowledge architects who spend hours immersed in text and require an environment free of cognitive friction, flashing telemetry, and visual noise.
- **Emotional Response:** Quiet confidence, spatial relief, mental restoration, and deep focus. The interface never clamors for attention; it yields entirely to the user's thought stream.

### Aesthetic Direction: Modern Austere Minimalist
The aesthetic eliminates decorative ornamentation in favor of micro-scale structural discipline:
- **Whisper-Quiet Chrome:** Non-intrusive container edges, ultra-subtle dividers, and generous spatial padding that respects focal hierarchy.
- **Contextual Presence:** Toolbars, actions, and telemetry remain dormant or completely hidden until intentionally summoned or hovered over.
- **Restrained Tonal Discipline:** Elimination of high-saturation alarms, pulsating beacons, or rainbow badge clusters in favor of desaturated, natural earth-and-mineral tones.

## Colors

The palette is tuned around matte obsidian and parchment ink tones, inspired by Japanese mineral pigments (*iwa-enogu*) and aged ink on washi paper. High-contrast harsh blacks and searing neon accents are strictly avoided to eliminate eye fatigue during protracted focus sessions.

### Functional Roles
- **Canvas Base (`#14161b`):** Deep warm obsidian charcoal. Soft matte ground that absorbs ambient light.
- **Surface Elevation (`#1a1d24`):** Modest tonal offset for floating command palettes, popovers, and status strips.
- **Primary Ink (`#6c99bb`):** Muted slate indigo. Used selectively for focused cursors, active navigation anchors, and primary selection brackets.
- **Secondary / Success / Additions (`#7daea3`):** Desaturated bamboo sage. Signals git additions, positive completion, and verified states without alarming the visual field.
- **Tertiary / Warning (`#c5a059`):** Weathered sand/amber. Reserved for pending states, non-blocking advisories, and cautious metadata.
- **Destructive / Deletion (`#b26a6a`):** Muted earth clay. Denotes line deletions, destructive confirmation prompts, and error notices.
- **Typography Base (`#dcd7ba`):** Parchment-tinted off-white. High legibility with reduced luminance strain against dark matte backdrops.
- **Typography Muted (`#71788a`):** Weathered slate for line numbers, secondary properties, keystroke guides, and passive metadata.
- **Dividers & Strokes (`#20242e`):** Hairline demarcation with near-zero visual resistance.

## Typography

The typographic hierarchy establishes clear separation between content (editor canvas) and system structure (commands, menus, metadata).

### Type Pairing
- **JetBrains Mono:** The primary voice for prose editing, code structure, line counts, and technical metrics. It maintains strict rhythmic cadences, open apertures, and balanced character widths that allow long reading sessions without strain.
- **Geist:** A neutral, highly engineered grotesque employed exclusively for command palettes, file trees, metadata indicators, keyboard shortcuts, and structural chrome.

### Rhythm & Vertical Cadence
- In the primary reading and drafting pane, line heights operate strictly within a relaxed range (1.65 - 1.7) to allow visual air between lines.
- System chrome and command palettes drop down to tighter line heights (1.3 - 1.4) to maintain density in contextual floating windows without introducing clutter.

## Layout & Spacing

The layout philosophy centers on expansive textual breathability flanked by collapsible, quiet operational margins.

### Layout Philosophy
- **Editor Canvas:** Adopts a restrained maximum column width (between 68ch and 80ch) when in prose mode to preserve optimal eye-tracking. In code split mode, panels scale fluidly with explicit minimal bounds (380px per pane).
- **Collapsible Perimeter:** Sidebars (file trees, symbol outlines) and auxiliary drawers auto-collapse into margin rails. When opened, they preserve layout calm by sliding into the gutter rather than triggering jittery reflows.

### Breakpoints & Adaptability
- **Desktop (>= 1280px):** Full multi-pane view allowed; editor margins expand to generous `space-xl` (40px) buffers around the text core.
- **Tablet (768px - 1279px):** Single-pane active editing with auxiliary panels moving to modal overlays.
- **Mobile (< 768px):** Zero peripheral chrome. All peripheral status items and file trees collapse into a single slide-over sheet. Gutters compress to `gutter-mobile` (12px).

## Elevation & Depth

In keeping with minimalist principles, elevation is conveyed through tonal surface layering and low-contrast perimeter hairlines, completely avoiding high-intensity drop shadows.

### Surface Tiers
- **Tier 0 (Root Workspace):** `#14161b` (Base obsidian canvas).
- **Tier 1 (Panels & Sidebars):** `#171920` with a right hairline border of `1px solid rgba(255, 255, 255, 0.04)`.
- **Tier 2 (Floating Overlays, Command Palette, Modals):** `#1c1f27` with a subtle outline of `1px solid rgba(255, 255, 255, 0.08)`.

### Shadow Treatment
Floating command palettes and tooltips use an ultra-diffused, ambient boundary shadow:
- `box-shadow: 0 12px 32px -8px rgba(0, 0, 0, 0.45), 0 0 0 1px rgba(255, 255, 255, 0.06);`
- No colored shadows or radiant glows are permitted around active items. Focus is indicated solely through hairline border shifts to `#6c99bb` at 40% opacity.

## Shapes

The geometry of this design system is architectural, grounded, and precise. Curvature is kept low (`roundedness: 1`, translating to base radii of `4px` with larger overlays maxing out at `8px`), evoking the machined precision of high-end hardware and classic terminal windows.

- **Buttons & Input Fields:** 4px radius (`rounded-sm`).
- **Command Palette & Modal Enclosures:** 6px to 8px radius (`rounded-md` to `rounded-lg`).
- **Tags, Tokens, & Line Highlighting:** 2px radius or rectilinear box caps. Circular or pill-shaped geometries are strictly prohibited.

## Components

### Buttons & Interactive Triggers
- **Ghost/Tertiary (Default):** Transparent background, text rendered in muted slate (`#71788a`), transitions to `#dcd7ba` on hover. No background fill change until pressed.
- **Subtle Surface (Secondary):** `#1a1d24` surface with a 1px border of `rgba(255, 255, 255, 0.05)`. Text in `#dcd7ba`. Hover slightly lightens the fill to `#20242e`.
- **Primary Action:** Muted indigo fill (`#6c99bb`) with dark charcoal text (`#14161b`) for immediate legible contrast, used only for critical confirmation interactions.

### Input Fields & Command Palette
- **Palette Input:** Borderless input surface seated atop `#1c1f27`. Font size `15px` (`body-lg`), zero inner shadow, placeholder text in `#71788a` at 60% opacity.
- **Search & Filter Inputs:** Inset baseline with `1px solid rgba(255, 255, 255, 0.08)`. Focus shifts the hairline to `#6c99bb` without expanding box outlines.

### Chips, Tags & Status Indicators
- **Style:** Compact rectangular tags with `2px` corners, zero background fill, and a delicate `1px` tinted border matching their respective functional role.
- **Palette:** 
  - Additions: `#7daea3` (sage)
  - Advisories: `#c5a059` (sand)
  - Deletions: `#b26a6a` (clay)
- **Status Dots:** Avoided. System state is conveyed via quiet monospaced typographic badges (e.g., `+12`, `mod`, `ro`) rather than saturated spheres.

### Lists & Navigation Trees
- **File & Outline Trees:** 24px item height, `JetBrains Mono` at `12.5px`. Active item denoted by a subtle left-edge 2px pip in `#6c99bb` and a tonal wash of `rgba(255, 255, 255, 0.03)`. No heavy full-row highlights.
- **Hover Transitions:** 80ms fast linear opacity cross-fades to ensure the interface feels instantaneous and weightless.

### Cards & Grouping Containers
- **Content Panes:** Transparent cards bounded only by spacing or an optional hairline rule (`#20242e`). Never enclose blocks of text in raised, high-elevation white or gray cards.

### Editor-Specific Components
- **Breadcrumb Navigation:** Rendered in `label-sm` using `/` separators in `#71788a` with 30% opacity. Appears dimmed at the top margin and gains full contrast only on mouse movement.
- **Line Number Gutter:** Monospaced `12px` in `#71788a` at 40% opacity; active line number shifts to `#dcd7ba` with a hairline vertical cursor indicator.