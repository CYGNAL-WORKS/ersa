---
version: alpha
name: Ersa
description: Design tokens and rationale for the Ersa desktop application. Follows the DESIGN.md format (google-labs-code/design.md); exported to W3C design tokens JSON for the interface build.
colors:
  # Midnight, the default palette (see Colors). Generated with Leonardo from contrast targets against the canvas. Both themes ship from day one.
  light:
    canvas: "#F1F3F6"
    surface: "#FBFCFD"
    surface-raised: "#F6F7F9"
    ink: "#212429"
    ink-muted: "#616368"
    ink-faint: "#8A8D91"
    line: "#D4D7DB"
    accent: "#2F61E0"
    accent-ink: "#FFFFFF"
    positive: "#217D4C"
    caution: "#966413"
    danger: "#BE4539"
    wash-primary: "#3D6BFF0F"
    wash-secondary: "#8FA6FF0D"
  dark:
    canvas: "#0B0D10"
    surface: "#181A1F"
    surface-raised: "#222429"
    ink: "#D8DBDF"
    ink-muted: "#929499"
    ink-faint: "#686A70"
    line: "#2F3137"
    accent: "#739EFF"
    accent-ink: "#0B0D10"
    positive: "#32A166"
    caution: "#BD8326"
    danger: "#E66858"
    wash-primary: "#3D6BFF14"
    wash-secondary: "#8FA6FF12"
typography:
  ui:
    fontFamily: "'IBM Plex Sans', 'Segoe UI Variable', system-ui, sans-serif"
    fontSize: 14px
    lineHeight: 1.45
  ui-small:
    fontFamily: "{typography.ui.fontFamily}"
    fontSize: 12px
    lineHeight: 1.4
  heading:
    fontFamily: "Oxanium, {typography.ui.fontFamily}"
    fontSize: 17px
    fontWeight: 600
    lineHeight: 1.3
  wordmark:
    fontFamily: "{typography.heading.fontFamily}"
    fontSize: 40px
    fontWeight: 600
    lineHeight: 1
  prose:
    fontFamily: "{typography.ui.fontFamily}"
    fontSize: 15px
    lineHeight: 1.6
  mono:
    fontFamily: "'JetBrains Mono', 'Cascadia Code', Consolas, monospace"
    fontSize: 13px
    lineHeight: 1.55
rounded:
  sm: 4px
  md: 8px
  lg: 12px
  pill: 999px
spacing:
  unit: 4px
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  xl: 24px
  xxl: 32px
line:
  # Custom group. Stroke widths for borders and rules.
  hairline: 1px
size:
  # Custom group. Fixed layout sizes.
  rail: 224px
  tile-min: 224px
  # The transcript's reading column.
  measure: 672px
  # The height past which a tool card's output or a long permission field scrolls.
  card-max: 224px
motion:
  # Custom group. Durations in ms, easings as CSS cubic-bezier.
  instant: 80
  quick: 160
  settle: 260
  slow: 400
  ease-out: "cubic-bezier(0.2, 0.8, 0.2, 1)"
  ease-in-out: "cubic-bezier(0.4, 0, 0.2, 1)"
---

# Ersa design

## Overview

Ersa is an agentic development harness for multi-agent work. The user runs several agents at once and moves between them; the interface makes state legible at a glance and stays out of the way while they read. It looks and behaves like the agentic harnesses people already use (Claude Desktop, Claude in VS Code, VS Code's agents view), so nothing about the conversation has to be learned, and it is more beautiful than they are.

The colors in the front matter are Midnight, the default palette, and the faces and type scale are chosen; the other values are placeholders while the spacing scale and icons are chosen. They are explored and reviewed in the Ersa design system (https://claude.ai/code/artifact/9be37025-1765-426d-89fa-0a47c8fa0f37); a choice agreed there is written back here before code uses it. ADR 0019 records why the direction changed.

Principles that resolve trade-offs:

- **Quiet surfaces, one accent.** Almost everything is neutral. Colour means something: the accent for what you can act on and what is waiting for you, semantic colours for state.
- **Familiar conversation, readable output.** The user's messages sit in chat bubbles. The agent's output has no bubble and reads as a document in `prose` at the `measure` column. No avatars.
- **Status lives in tiles.** Cost, context, permissions and progress are compact tiles with the same anatomy everywhere, so the eye learns one shape.
- **Calm and luminous.** One thing draws the eye at a time. Light and depth carry the design: soft tonal gradients give surfaces and primary controls a sense of material, a faint wash keeps the canvas from being flat, and a gentle accent glow marks the one thing that is focused or waiting for you. A screen has one focal glow; everything else rests.
- **Motion is part of the feel.** Transitions show where something came from or went to, and motion may also be there for its own sake: a message settling in, a tile easing into place, a mark that breathes once. Keep it calm, never delay input, use the motion tokens, and honour reduced motion.
- **Both themes are first-class.** Light and dark are designed together. A component is not done until it is right in both.
- **Light on dew.** Ersa is the Greek goddess of dew, a daughter of Selene, the moon. Dew forms under moonlight and catches the first light of morning: a cool, clear, quiet base with small points of brightness. That image is where the palette, the wash and the glow take their character from, and it is what keeps Ersa from looking like every other developer tool.
- **Attention is an edge or a glyph, never a fill.** A session that needs the user is told by its status glyph; a failed one by the danger glyph. Filled attention rows are forbidden: with ten sessions they become a wall of colour within a week. The open session is visibly distinct from the others, quietly.

## Colors

Two token sets, `light` and `dark`, with identical names, so components name a role and never a value. `canvas` is the window background; `surface` is a pane or card; `surface-raised` is a hovered or focused card. `ink` levels carry text hierarchy. `line` is the only border colour. `accent` marks what can be acted on and what is waiting for you. Semantic colours (`positive`, `caution`, `danger`) appear on glyphs and inline status, never as large fills.

Text a person must read meets 4.5:1 against its background; a control's edge and a state glyph meet 3:1. Generate the palette from these targets (the Leonardo method: name each role's contrast against the canvas and derive both themes from it) rather than picking hex values by eye.

`wash-primary` and `wash-secondary` are the two colours of the canvas wash, carrying their own low alpha, so a palette sets how faint its wash is.

**Palettes.** A palette is one complete set of these tokens for both themes, and the user chooses theirs. Midnight is the default and its values are the front matter above: near-black neutrals with a deep, saturated blue. Moonlit (blue-black night with a moonlight blue), Nocturne (deep indigo with periwinkle-lavender), Moonrise (blue-black with a soft champagne gold) and Selene (silver greys with the same gold) are offered too; all five come from "light on dew" and are compared in the design system's Palette card. Their values are written here when choosing a palette is built. Every palette meets the contrast targets above in both themes, and because components name roles, a palette changes no component. A gold accent sits close to `caution`, so Moonrise and Selene need their caution separated before they ship.

## Typography

Type is assigned by role, never by face: `ui` for controls, the rail and tiles; `ui-small` for captions and labels; `prose` for words in the transcript, slightly larger and looser for sustained reading; `heading` for titles; `wordmark` for the Ersa wordmark; `mono` for code, paths and command output. Headings are rare and small; the transcript's hierarchy comes from spacing.

Numeric readouts (context, cost, counts, durations) are set in the UI face with tabular figures, so digits do not jitter as values change. `mono` is for code and what the user may copy, not for numbers on their own.

The faces: Oxanium for the wordmark and titles (`wordmark`, `heading`), IBM Plex Sans for everything scanned and read (`ui`, `ui-small`, `prose`), and JetBrains Mono for code (`mono`). Code is set with ligatures off, so it shows the characters that were typed. All three are bundled with the interface, so every machine renders them rather than a fallback.

The wordmark has its own size, much larger than `heading`, and is drawn in `ink`, not `ink-muted`: it is the one large mark on the new-session screen.

## Layout

Every gap, padding and size is a named step on one scale; space shows grouping, and the gap grows with the distance between ideas. The transcript keeps a reading column of comfortable width however wide the window gets. The shell layout is specified in `LAY-SHELL`. The scale, grid and fixed sizes in the front matter are placeholders.

## Elevation & Depth

Depth comes from tone and light, not from drop shadows:

- **Material.** Surfaces, cards and primary controls carry a soft top-lit tonal gradient between neighbouring surface steps. It should be felt more than seen.
- **Wash.** The canvas carries a very faint wash of `wash-primary` from the top right and `wash-secondary` from the bottom left, so it is never purely flat. It is barely there; if it is noticed first, it is too strong.
- **Focal glow.** The one focused or waiting element (an open permission request, the focused composer) glows in the accent and may breathe once as it arrives. In dark it is a soft blur; in light a wide blur reads as a shadow, so it is a tight tinted ring instead.
- **Not a halo.** No luminous backdrop or halo around the window: tried, and too strong.

Shadows are kept for transient layers (menus, popovers, drag previews), soft and short.

## Shapes

Corners come from a small set of radii assigned by role (control, card, floating layer, pill), so things of one kind share one shape; the user's message bubble takes the floating-layer radius. Panes have no radius; they meet at lines. Every line is `hairline` wide; focus and state change a line's colour, never its width.

## Iconography

One library at one stroke weight, sized to the type beside it; the candidates are Fluent UI System Icons, Lucide and Phosphor. An icon sits beside a label unless it is universally understood (close, copy, search), and then it has a tooltip and an accessible name. Icons take ink colours; state colour stays on the glyphs.

## Components

Component style tokens are added here as component briefs reach `ready`. Each component brief (`CMP-*`) lists the tokens it consumes; this section holds the values.

## Do's and Don'ts

- Do give every state (hover, focus, pressed, disabled, loading, empty, error) a deliberate appearance in both themes.
- Do use the motion tokens for every transition, and keep motion calm.
- Do keep the rail and tiles visually quieter than the transcript, so attention lands on the conversation.
- Do add a token and a reason for every new colour, gradient or glow.
- Do keep one focal glow per screen.
