# 0019: Ersa looks like a familiar agentic harness, calm and luminous, rather than an instrument

- Status: accepted
- Date: 2026-11-02
- Related briefs: PRD-ERSA

## Context

`DESIGN.md` framed Ersa as "an instrument, not a chat app": no speech bubbles, every numeric readout in tabular mono, no gradients, no glow and no purple, on the grounds that those "look like AI". Reviewing the design system against reference screenshots of current agentic tools (Conductor, VS Code's agent sessions, Zed, Warp, Antigravity, Claude Desktop), the owner found the instrument framing made Ersa unfamiliar without making it better, and that "not AI-looking" was a negative rule that agents applied as a ban on anything warm or bright. The owner wants Ersa to behave like the harnesses people already use and to be beautiful, with some light and animation.

## Options considered

1. **Keep the instrument direction**: distinctive on paper. Bubbles absent, readouts in mono and a flat surface make the transcript feel foreign next to every tool users already know, and the negative rules gave no direction for what to do instead.
2. **Copy a vendor design language** (Material, Fluent): a complete system to follow. It would make Ersa look like that vendor's app, and none of them is built for many agents at once.
3. **Familiar conversation, calm and luminous material, an identity from the name**: user messages in bubbles and agent output as a document, as other harnesses do; soft tonal gradients, a faint canvas wash and one focal glow; readouts in the UI face with tabular figures; character drawn from Ersa's myth (the goddess of dew, daughter of the moon).

## Decision

Option 3. `DESIGN.md` states what Ersa looks like rather than what it avoids. The colours are a palette, Midnight (near-black with a deep blue), generated from contrast targets; four more palettes from the same idea are offered for the user to choose. The faces (IBM Plex Sans, Oxanium for titles, JetBrains Mono for code) and the Balanced type scale are chosen in the Ersa design system; radii and icons stay placeholders until chosen there. A glowing halo around the window was tried and rejected as too strong, a flat surface as lifeless, and a green dawn palette as not right.

## Consequences

Interface code built under the old rules no longer matches: the transcript shows user messages with a rule rather than a bubble, readouts are in mono, and the rail outlines the active session in the accent. Those are changed as tickets, not in passing. Offering palettes commits the interface to naming colour roles only, with every palette meeting the contrast targets in both themes; choosing one needs a setting, which is specified in a brief before it is built. Gradients and glow are now allowed, so the risk moves from "too austere" to "too busy": every gradient or glow needs a token and a reason, and a screen has one focal glow. Revisit if users find the bubbles or the glow get in the way of reading many sessions at once.
