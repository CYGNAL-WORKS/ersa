# 0036: Each backend shows its own mark as an ink-toned glyph

- Status: accepted
- Date: 2026-11-15
- Related briefs: LAY-SHELL

## Context

A rail tile leads with its backend's icon, which is how the user tells a Claude Code session from a Copilot one. The design held every backend to one neutral icon until the vendors' brand terms were checked, and the placeholder, a generic robot head, read as Copilot's on a Claude Code session. The terms were read on 2026-11-15 (#399): both Anthropic and GitHub ask for written permission before their logos appear in another product, and both allow naming the product in plain text.

## Options considered

1. **One neutral icon for every backend, told apart by name.** Needs nobody's permission, and loses the at-a-glance difference between backends.
2. **Each vendor's mark, after asking each vendor in writing.** What the terms ask for; holds the rail on two answers that may not come.
3. **Each backend's own mark now, as a small glyph in the tile's ink** (the owner's decision, 2026-11-15).

## Decision

Option 3. Ersa is not a commercial product, and a mark here says which tool a session runs on, as a file manager's icon says which program opens a file.

- A Claude Code session shows the Claude spark; a Copilot session shows Copilot's icon; a backend Ersa doesn't know shows a neutral terminal icon, never a robot head.
- Each mark is one filled glyph drawn in the tile's ink in both themes and every state. It never takes a brand color, and Ersa's own name, icon and marketing use no vendor's mark.
- The marks' paths are copied from the simple-icons set as data; no package is added for them.
- Offering Ersa commercially, or a vendor asking for a mark's removal, reopens this.

## Consequences

Backends read apart at a glance. The marks are the one exception to the rule that icons are stroked Lucide icons. The use rests on the owner's judgment of the vendors' terms, not on their permission, so it is one to revisit before Ersa is sold or distributed widely.
