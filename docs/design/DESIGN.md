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
    ink-muted: "#505258"
    ink-faint: "#6A6C72"
    line: "#D4D7DB"
    accent: "#2F61E0"
    accent-ink: "#FFFFFF"
    positive: "#217D4C"
    caution: "#7A5E00"
    danger: "#BE4539"
    wash-primary: "#3D6BFF0F"
    wash-secondary: "#8FA6FF0D"
  dark:
    canvas: "#0B0D10"
    surface: "#181A1F"
    surface-raised: "#222429"
    ink: "#D8DBDF"
    ink-muted: "#A6A8AD"
    ink-faint: "#8A8C91"
    line: "#2F3137"
    accent: "#739EFF"
    accent-ink: "#0B0D10"
    positive: "#32A166"
    caution: "#E6B93A"
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
    lineHeight: 1.5
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
  xxxl: 48px
line:
  # Custom group. Stroke widths for borders and rules.
  hairline: 1px
size:
  # Custom group. Fixed layout sizes.
  # The rail's width; a tile fills it.
  rail: 288px
  # The transcript's reading column.
  measure: 720px
  # The widest a chip's list grows; longer text in it is cut with an ellipsis.
  menu: 360px
  # The widest a user's message bubble grows, as a share of the reading column.
  bubble-max: 80%
  # The height past which a tool card's output or a long permission field scrolls.
  card-max: 224px
  # The state glyph a tile carries, and the dot before an inline problem.
  glyph: 8px
  # The accent mark beside a chip's chosen option.
  option-mark: 4px
  # The context ring's dial: in the session controls, and smaller on a tile.
  ring: 16px
  ring-sm: 12px
  # A control's height: a row in the rail, an icon button, an option.
  control: 28px
  # A small control or mark inside a tile.
  control-sm: 20px
  # A large control: the composer's attach, send and stop buttons.
  control-lg: 32px
  # Icons, by the type beside them: `ui`, `ui-small`, and a mark inside a tile.
  icon: 16px
  icon-sm: 14px
  icon-xs: 12px
  # The widest a dialog grows, such as the delete question.
  dialog: 360px
stack:
  # Custom group. Stacking orders, lowest first: a menu sits above the page it opens over; a dialog above everything.
  menu: 10
  dialog: 30
material:
  # Custom group. Gradients and fills that give surfaces and controls their material (see Elevation & Depth).
  # `{colors.<role>}` names a color role, so each value follows the theme and the palette.
  # The canvas: the faint wash from the top right and the bottom left over `canvas`.
  wash: "radial-gradient(90% 70% at 100% 0%, {colors.wash-primary}, transparent 75%), radial-gradient(80% 70% at 20% 100%, {colors.wash-secondary}, transparent 75%), {colors.canvas}"
  # Tiles and cards, lit from the top.
  tile: "linear-gradient(to bottom, {colors.surface-raised}, {colors.surface})"
  # The rail, settling toward the canvas at its foot.
  rail: "linear-gradient(to bottom, {colors.surface}, color-mix(in oklab, {colors.surface} 55%, {colors.canvas}))"
  # The primary button.
  primary: "linear-gradient(to bottom, color-mix(in oklab, {colors.accent} 84%, white), {colors.accent})"
  # The composer, lit from below, toward the user's hands.
  composer: "linear-gradient(to bottom, {colors.surface}, {colors.surface-raised})"
  # The open session's tile in the rail: a quiet neutral highlight, no accent.
  open-session: "color-mix(in oklab, {colors.ink} 6%, {colors.surface})"
  # Under the pointer, and while pressed: a veil of ink over whatever is beneath.
  hover: "color-mix(in oklab, {colors.ink} 4%, transparent)"
  pressed: "color-mix(in oklab, {colors.ink} 9%, transparent)"
glow:
  # Custom group. Accent glows as box shadows, built from color roles like `material`.
  # In dark a glow is a soft blur; in light a wide blur reads as a shadow, so light draws a tight tinted ring.
  light:
    # The one focused or waiting element.
    focal: "0 0 0 1px {colors.accent}, 0 0 0 4px color-mix(in oklab, {colors.accent} 16%, transparent), 0 0 12px 2px color-mix(in oklab, {colors.accent} 20%, transparent)"
  dark:
    focal: "0 0 0 1px {colors.accent}, 0 0 16px -4px color-mix(in oklab, {colors.accent} 32%, transparent)"
shadow:
  # Custom group. The soft, short shadow of a transient layer (menus, popovers, drag previews). Light builds it from the ink role, as `glow` builds from accent.
  # Dark uses a fixed black on purpose: its canvas is already near black, so a shadow drawn from a role would not show, and it need not follow the palette.
  light:
    transient: "0 4px 12px -4px color-mix(in oklab, {colors.ink} 14%, transparent)"
  dark:
    transient: "0 4px 12px -4px color-mix(in oklab, black 60%, transparent)"
motion:
  # Custom group. Durations in ms, easings as CSS cubic-bezier.
  instant: 80
  quick: 160
  settle: 260
  slow: 400
  # A chunk of streaming prose fading in to full ink.
  stream: 1000
  # One turn of the ring for work of unknown length.
  loop-ring: 1400
  # The single swell and fade of the chevron of a session that comes to need the user.
  breathe-once: 2400
  # One breath of a working session's glyph in the rail.
  breath: 3000
  ease-out: "cubic-bezier(0.2, 0.8, 0.2, 1)"
  ease-in-out: "cubic-bezier(0.4, 0, 0.2, 1)"
  # Constant speed, for a loop that turns.
  linear: "linear"
---

# Ersa design

## Overview

Ersa is an agentic development harness for multi-agent work. The user runs several agents at once and moves between them; the interface makes state legible at a glance and stays out of the way while they read. It looks and behaves like the agentic harnesses people already use (Claude Desktop, Claude in VS Code, VS Code's agents view), so nothing about the conversation has to be learned, and it is more beautiful than they are.

The colors in the front matter are Midnight, the default palette; the faces, type scale, spacing scale, sizes, radii and icons are chosen too. They are explored and reviewed in the Ersa design system (https://claude.ai/code/artifact/9be37025-1765-426d-89fa-0a47c8fa0f37); a choice agreed there is written back here before code uses it. ADR 0019 records why the direction changed.

Principles that resolve trade-offs:

- **Quiet surfaces, one accent.** Almost everything is neutral. Color means something: the accent for what you can act on, an agent at work and a question waiting for you; caution for a permission request; danger for a failure.
- **Familiar conversation, readable output.** The user's messages sit in chat bubbles. The agent's output has no bubble and reads as a document in `prose` at the `measure` column. No avatars.
- **Status lives in tiles.** Cost, context, permissions and progress are compact tiles with the same anatomy everywhere, so the eye learns one shape.
- **Calm and luminous.** One thing draws the eye at a time. Light and depth carry the design: soft tonal gradients give surfaces and primary controls a sense of material, a faint wash keeps the canvas from being flat, and a gentle accent glow marks the one thing that is focused or waiting for you. A screen has one focal glow; everything else rests.
- **Nothing pops in.** Things fade, slide, or glow into place, and leave the same way. Motion shows where something came from or went to, stays calm, never delays input, and uses the motion tokens. With reduced motion on, everything appears at once and nothing loops: the ring and a working glyph hold still at full tone. Elevation & Depth gives each moving thing its motion.
- **Both themes are first-class.** Light and dark are designed together. A component is not done until it is right in both.
- **Light on dew.** Ersa is the Greek goddess of dew, a daughter of Selene, the moon. Dew forms under moonlight and catches the first light of morning: a cool, clear, quiet base with small points of brightness. That image is where the palette, the wash and the glow take their character from, and it is what keeps Ersa from looking like every other developer tool.
- **Attention is an edge or a glyph, never a fill.** A session that needs the user shows a chevron where its state glyph would be, in the tone of what it wants. A failed one shows the danger glyph. Filled attention rows are forbidden: with ten sessions they become a wall of color within a week. The open session is visibly distinct from the others, quietly.

## Colors

Two token sets, `light` and `dark`, with identical names, so components name a role and never a value. `canvas` is the window background; `surface` is a pane or card; `surface-raised` is a hovered or focused card. `ink` levels carry text hierarchy. `line` is the only border color. `accent` marks what can be acted on, an agent at work, and a question waiting for the user. `caution` is yellow and marks a permission request and a gauge near full; `danger` marks a failure; `positive` marks success. Semantic colors appear on glyphs, icons and inline status, never as large fills.

Text meets 4.5:1 against its background; a control's edge and a state glyph meet 3:1. Every `ink` level, `ink-faint` included, meets 4.5:1 against `canvas`, `surface`, and `surface-raised`, so `ink-faint` is safe for placeholders and disabled text, and `ink-muted` stays a clear step above it. Generate the palette from these targets (the Leonardo method: name each role's contrast against the canvas and derive both themes from it) rather than picking hex values by eye. `caution` is the one role set by eye: the owner chose yellow, and its light value is as light as keeps 4.5:1 on every fill a tile can take (`docs/design/RAIL.md`).

`wash-primary` and `wash-secondary` are the two colors of the canvas wash, carrying their own low alpha, so a palette sets how faint its wash is.

**Palettes.** A palette is one complete set of these tokens for both themes, and the user chooses theirs in Ersa's settings, as Slack users choose their colors. That choice is far off and needs its own brief before it is built. Midnight is the default and its values are the front matter above: near-black neutrals with a deep, saturated blue. Moonlit (blue-black night with a moonlight blue), Nocturne (deep indigo with periwinkle-lavender), Moonrise (blue-black with a soft champagne gold) and Selene (silver greys with the same gold) are offered too; all five come from "light on dew" and are compared in the design system's Palette card. Their values are written here when choosing a palette is built. Every palette meets the contrast targets above in both themes, and because components name roles, a palette changes no component. A gold accent sits close to `caution`, so Moonrise and Selene need their caution separated before they ship.

## Typography

Type is assigned by role, never by face: `ui` for controls, the rail and tiles; `ui-small` for captions and labels; `prose` for words in the transcript, slightly larger and looser for sustained reading; `heading` for titles; `wordmark` for the Ersa wordmark; `mono` for code, paths and command output. Headings are rare and small; the transcript's hierarchy comes from spacing.

Numeric readouts (context, cost, counts, durations) are set in the UI face with tabular figures, so digits do not jitter as values change. `mono` is for code and what the user may copy, not for numbers on their own.

The faces: Oxanium for the wordmark and titles (`wordmark`, `heading`), IBM Plex Sans for everything scanned and read (`ui`, `ui-small`, `prose`), and JetBrains Mono for code (`mono`). Code is set with ligatures off, so it shows the characters that were typed. All three are bundled with the interface, so every machine renders them rather than a fallback.

The agent's text stays in IBM Plex Sans: a serif for `prose` was tried and rejected as harder to read.

The wordmark has its own size, much larger than `heading`, and is drawn in `ink`, not `ink-muted`: it is the one large mark on the new-session screen.

## Layout

Every gap, padding and size is a named step on one scale, on a 4px grid; space shows grouping, and the gap grows with the distance between ideas. The transcript keeps a reading column of comfortable width (`measure`) however wide the window gets. The rail is `rail` wide (288px, as Claude Desktop's session list; VS Code's and Zed's default to 300px), and a tile fills it. Small marks take a `size` token, not a spacing step: `size-glyph` for a tile's state glyph and the dot before an inline problem, `size-option-mark` for the mark beside a chip's chosen option, `size-ring` for the context ring in the session controls and `size-ring-sm` for it on a tile. A control's height is `size-control`, or `size-control-sm` inside a tile, and an icon is `size-icon`, `size-icon-sm` or `size-icon-xs`, by the type beside it. The shell layout is specified in `LAY-SHELL`.

**Turns are grouped by space.** A turn is a message to the agent, usually the user's, and everything the agent does in answer. Inside a turn, blocks (messages, tool cards, permission requests) sit `space-md` apart, so a turn reads dense and more of the conversation fits on screen. Between turns, before each user message except the first, the gap is `space-xxxl`, so each turn reads as one group without a divider. Claude Desktop uses about 11px inside a reply and 50 to 70px between turns.

The user's message bubble sits at the right of the reading column, as wide as its words up to `bubble-max`, filled `surface-raised` with a `line` edge. It is padded `space-md` above and below and `space-lg` at the sides. It shows no label: its place says who wrote it, and assistive technology still hears "You".

## Elevation & Depth

Depth comes from tone and light, not from drop shadows:

- **Material.** Surfaces, cards and primary controls carry a soft top-lit tonal gradient between neighbouring surface steps. It should be felt more than seen.
- **Wash.** The canvas carries a very faint wash of `wash-primary` from the top right and `wash-secondary` from the bottom left, so it is never purely flat. It is barely there; if it is noticed first, it is too strong.
- **Focal glow.** The one focused or waiting element (an open permission request, a question the user answers before going on such as whether to trust a folder, or, when neither is open, the focused composer) glows in the accent and fades in as it arrives, over `motion-settle` with `motion-ease-out`. In dark it is a soft blur; in light a wide blur reads as a shadow, so it is a tight tinted ring instead.
- **Not a halo.** No luminous backdrop or halo around the window: tried, and too strong.

Each of these is a token, built from color roles so it follows the theme and the palette. `material-wash` is the canvas. `material-tile` lights tiles and cards from the top, `material-rail` settles the rail toward the canvas at its foot, `material-primary` is the primary button, and `material-composer` is the composer. The open session's tile is `material-open-session`, a quiet neutral highlight with no accent outline. `material-hover` and `material-pressed` are a veil of ink over whatever is beneath. `glow-focal` is the focal glow. Light and dark set their own glows; the materials are the same in both. While a permission request is open it holds the focal glow, and the focused composer shows its focus by its edge alone, so the screen keeps one focal glow.

**Pointer and press.** A control with an edge (a button) shows the pointer by its edge turning `ink-faint` and its fill stepping up a surface, and sinks to `canvas` when pressed. A control without an edge (a tile, a project heading, a quiet row, an icon button, a chip, an option in a chip's list) takes `material-hover` under the pointer and `material-pressed` when pressed, inside `radius-md` corners (a chip's are `radius-pill`), and with keyboard focus its edge turns `ink`.

Shadows are kept for transient layers (menus, popovers, drag previews), soft and short: `shadow-transient`, with `radius-lg` corners. A transient layer stacks above the page it opens over by a `stack` token (`stack-menu`), never a bare number.

**Motion.** Each moving thing has one motion, reviewed live in the design system's Motion card:

- **A sent message** fades in at the foot of the transcript: alpha only, over `motion-slow` with `motion-ease-out`, and no movement. A message flying in was tried and rejected.
- **Streaming prose** fades in chunk by chunk, as Claude Desktop's does: each piece of the agent's text fades from transparent to full ink over `motion-stream` with `motion-ease-out` as it arrives, with no movement. Text read from the session record when a session opens appears at once; only text arriving live fades.
- **Work in progress** loops only while it runs. Work of unknown length, such as a running tool call, shows a ring that turns once per `motion-loop-ring` at constant speed (`motion-linear`), a duration within Material's, MUI's, and Fluent's. Work whose progress can be measured shows a bar that fills to its value, easing each change over `motion-settle` with `motion-ease-out`; it doesn't loop. A text shimmer was tried and is not used: it is a skeleton-loading pattern, not a sign of work under way.
- **A working session's glyph** in the rail breathes: its tone swells and softens on a `motion-breath` loop with `motion-ease-in-out` for as long as the session works. The breath is the glyph's own tone, not a glow, so the screen's one focal glow stays where it is. It never blinks or ripples outward; shimmering titles and pulse rings were rejected.
- **Status never moves a tile.** Nothing reorders the rail on its own: a session that comes to need the user shows its chevron, which breathes once where it is, one swell and fade of its tone over `motion-breathe-once` with `motion-ease-in-out`. When the user regroups or re-sorts the rail, pins a session or moves it to a section, or a turn ends while the rail is sorted by last activity, each tile slides to its new position over `motion-settle` with `motion-ease-in-out`, so the eye can follow it. A new session's tile, a fork's included, slides in at its place in the rail over `motion-slow` with `motion-ease-out`, while the others make room over the same duration and easing.
- **The shelf** slides out from the tile it belongs to over `motion-settle` with `motion-ease-out`, and slides back the same way when answered.
- **The working mark**, the animated mark at the foot of a turn beside the elapsed time and tokens, is open: it is to be designed as a family of marks.

## Shapes

Corners come from a small set of radii assigned by role, so things of one kind share one shape: `radius-sm` for small things inside a card (an output block), `radius-md` for controls and cards (buttons included, as Claude Desktop's are) and for an option inside a floating layer, so its corners nest within the layer's, `radius-lg` for floating layers and the user's message bubble, and `radius-pill` for pills. Panes have no radius; they meet at lines. Every line is `hairline` wide; focus and state change a line's color, never its width.

## Iconography

Icons are Lucide, drawn as Lucide draws them: an even stroke with round ends, 2 units wide on Lucide's 24-unit grid, so the stroke scales with the icon and stays in step with the type beside it. Each icon is sized to that type. Which Lucide package the interface takes is decided by the first ticket that draws an icon, under ADR 0015. An icon sits beside a label unless it is universally understood (close, copy, search), and then it has a tooltip and an accessible name. Icons take ink colors, except a mark that carries state: a tile's chevron, and the shield on a project that is not trusted.

## Components

Component style tokens are added here as component briefs reach `ready`. Each component brief (`CMP-*`) lists the tokens it consumes; this section holds the values.

- **The rail, a project heading and a tile**: `docs/design/RAIL.md`. Read it before changing any of them.
- **The composer and the session controls**: `docs/design/COMPOSER.md`. Read it before changing either.

## Do's and Don'ts

- Do give every state (hover, focus, pressed, disabled, loading, empty, error) a deliberate appearance in both themes.
- Do use the motion tokens for every transition, and keep motion calm.
- Do bring things in with a fade, a slide, or a glow, and take them out the same way; nothing pops in.
- Do loop only while work runs, and stop every loop when reduced motion is on.
- Do keep a tile in place when its status changes: its mark changes and breathes, the tile stays. Move tiles only for a new session or a reorder the user asked for.
- Do keep the rail and tiles visually quieter than the transcript, so attention lands on the conversation.
- Do add a token and a reason for every new color, gradient or glow.
- Do keep one focal glow per screen.
