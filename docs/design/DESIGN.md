---
version: alpha
name: Ersa
description: Design tokens and rationale for the Ersa desktop application. Follows the DESIGN.md format (google-labs-code/design.md); exported to W3C design tokens JSON for the interface build.
colors:
  # Values are first proposals, to be tuned in the component gallery. Both themes ship from day one.
  light:
    canvas: "#F6F5F2"
    surface: "#FFFFFF"
    surface-raised: "#FBFAF8"
    ink: "#1B1A17"
    ink-muted: "#6B675F"
    ink-faint: "#A39E94"
    line: "#E6E2DA"
    accent: "#2F6F8F"
    accent-ink: "#FFFFFF"
    positive: "#3B7F5C"
    caution: "#B9802B"
    danger: "#B04A3C"
  dark:
    canvas: "#15161A"
    surface: "#1C1E23"
    surface-raised: "#23262C"
    ink: "#ECEAE4"
    ink-muted: "#A6A299"
    ink-faint: "#6E6A62"
    line: "#2E3138"
    accent: "#6FA8C7"
    accent-ink: "#0F1A21"
    positive: "#6FB58F"
    caution: "#D8A251"
    danger: "#D9736A"
typography:
  ui:
    fontFamily: "Inter, 'Segoe UI Variable', system-ui, sans-serif"
    fontSize: 13px
    lineHeight: 1.45
  ui-small:
    fontFamily: "{typography.ui.fontFamily}"
    fontSize: 12px
    lineHeight: 1.4
  heading:
    fontFamily: "{typography.ui.fontFamily}"
    fontSize: 15px
    fontWeight: 600
    lineHeight: 1.3
  prose:
    fontFamily: "{typography.ui.fontFamily}"
    fontSize: 14px
    lineHeight: 1.6
  mono:
    fontFamily: "'JetBrains Mono', 'Cascadia Code', Consolas, monospace"
    fontSize: 12.5px
    lineHeight: 1.5
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

Ersa is an instrument, not a chat app. The user is running several agents at once and glancing between them; the interface's job is to make state legible at a glance and to stay out of the way while reading. The reference point is Claude Desktop's calm, document-like feel, pushed further toward a workbench: denser information in the rail and workbench, generous reading space in the transcript.

Principles that resolve trade-offs:

- **Quiet surfaces, one accent.** Almost everything is warm neutral. Colour means something: accent for the thing you can act on, semantic colours for state. Never decorative.
- **Agent output is a document, not a bubble.** The agent's words read like a page. The user's messages are visually distinct but equally plain. No speech bubbles, no avatars in the stream.
- **Status lives in tiles.** Cost, context, permissions, and progress are compact, consistent tiles with the same anatomy everywhere, so the eye learns one shape.
- **Motion explains.** Every animation shows where something came from or went to. Nothing animates for decoration. Durations come from the motion tokens; nothing is hand-timed.
- **Both themes are first-class.** Light and dark are designed together. A component is not done until it is right in both.
- **Not "AI-looking".** No gradients, no glow, no sparkle iconography, no purple. If a choice would look at home on a model vendor's landing page, choose differently.
- **One distinctive move.** The principles above describe most good developer tools of this era; followed alone they produce a competent clone. Ersa's signature is the instrument readout: every number in the interface (context ring, cost, counts, durations) is set in tabular mono figures with a consistent gauge vocabulary, so the rail reads like a well-made panel of dials rather than a list of chats. A second candidate, a UI typeface other than Inter, is to be evaluated in the gallery before M1 ends.
- **Attention is an edge, never a fill.** A tile that needs the user shows a coloured edge or glyph. Filled rows are forbidden: with ten sessions, filled attention rows become a wall of colour within a week and the principle above dies first.

## Colors

Two token sets, `light` and `dark`, with identical names so components reference roles, never values. `canvas` is the window background; `surface` is a pane or card; `surface-raised` is a hovered or focused card. `ink` levels carry text hierarchy. `line` is the only border colour. The accent is a muted blue chosen to sit quietly against warm neutrals; it marks interactive affordances and the active session. Semantic colours (`positive`, `caution`, `danger`) appear only in tiles and inline status, never as large fills.

Open: the exact accent hue is a proposal. Tune in the gallery against real transcripts in both themes before building components.

Attention marks use `accent` for "needs you" (permission, question) and `danger` for "failed"; both appear as an edge or glyph on the tile, never as a row fill.

## Typography

One UI face and one mono face. `ui` is the default for controls and the rail; `prose` is for agent and user text in the transcript, slightly larger and looser for sustained reading; `mono` is for code, paths, and anything the user may copy. Headings are rare and small: the transcript's hierarchy comes from spacing, not type size.

Every numeric readout (context, cost, counts, elapsed time) uses `mono` with tabular figures so digits align and do not jitter as values change. This is the instrument signature; do not set readouts in the UI face.

## Layout

A 4px unit. Pane padding is `lg`; inline gaps between related controls are `sm`; separation between unrelated groups is `xl`. The shell layout itself is specified in `LAY-SHELL`. Fixed sizes, such as the rail and the narrowest a tile column may get, come from the `size` tokens.

## Elevation & Depth

Flat by default. Depth is shown with `surface` steps and `line`, not shadows. The only shadows are on transient layers (menus, popovers, drag previews) and are soft and short.

## Shapes

`md` radius for cards, tiles, and inputs; `sm` for small controls and code blocks; `pill` for status chips. Panes have no radius; they meet at lines. Every line is `hairline` wide; focus and state change a line's colour, never its width.

## Components

Component style tokens are added here as component briefs reach `ready`. Each component brief (`CMP-*`) lists the tokens it consumes; this section holds the values.

## Do's and Don'ts

- Do give every state (hover, focus, pressed, disabled, loading, empty, error) a deliberate appearance in both themes.
- Do use the motion tokens for every transition.
- Do keep the rail and workbench visually quieter than the transcript so attention lands on the conversation.
- Don't introduce a new colour without adding a token and a reason.
- Don't animate on a timer; animate on a change.
- Don't use gradients, glows, or iconography that signals "AI".
