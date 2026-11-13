# The transcript

How the transcript looks, token by token: the agent's formatted text, tool cards and diff cards. `FLOW-READ-TRANSCRIPT` says how they behave, `LAY-SHELL` where the transcript sits, and `DESIGN.md` holds the tokens, the spacing of turns, the user's bubble and the motion of streaming text. Read this before changing the transcript.

The owner approves this page, not each ticket built from it (owner, 2026-11-13). The Main board on the design canvas draws a working session: the agent's words with no bubble, and two tool cards, one done and one running. Everything else here follows Claude Desktop where the board is silent. Where the board, the built components or the gallery differ from this page, this page wins.

## The page

The transcript is one column, `size-measure` wide at most and centered, padded `space-lg` at the top and `space-xl` at the sides. Blocks and turns are spaced as `DESIGN.md` (Layout) says. Its last `space-xxxl` fades out behind the composer.

## Formatted text

The agent's words are Markdown, rendered as `ADR 0034` says, in `prose` and `ink`, with no bubble.

- **Paragraphs** sit `space-md` apart.
- **Headings** are rare and quiet: levels 1 and 2 at the `heading` token's size and weight, levels 3 to 6 at `prose` size and the `heading` weight, all in the `prose` face (`heading`'s face is for titles), `space-lg` above and `space-xs` below.
- **Bold** takes the `heading` weight; *italic* and ~~strikethrough~~ are as the face draws them.
- **Inline code** is `mono` in `ink`, on `surface-raised` with `radius-sm`, padded `space-xs` at the sides.
- **A code block** is `mono` in `ink` on `surface` with a `line` edge and `radius-md`. A head row, `size-control` high and padded `space-xs` by `space-md`, holds the block's language in `ui-small` and `ink-faint` at the leading edge (nothing when the block names none) and Copy at the trailing edge. The code is padded `space-md`, keeps its lines unwrapped and scrolls sideways when wider than the column. Code has no syntax colors.
- **Copy** is a quiet icon button, `size-control-sm` square, `radius-md`, a copy icon at `size-icon-sm` in `ink-faint`, with the tooltip and accessible name "Copy code". Under the pointer it takes `material-hover` and reads `ink`, pressed `material-pressed`, with keyboard focus an `ink` edge. Once copied, its icon is a check in `ink-muted` and its tooltip says "Copied" until the pointer leaves it or focus moves, when it turns back; each change crossfades over `motion-quick`.
- **Lists** indent `space-xl`, items `space-xs` apart, the bullet or number in `ink-muted`. A task item's box is drawn, checked or not, at `size-icon-sm` in `ink-muted`, and can't be changed.
- **A quotation** has a `line` hairline at its leading edge, padded `space-md` from it, its words in `ink-muted`.
- **A table** is `ui` in `ink`, its header row at the `heading` weight, rows divided by `line` hairlines, cells padded `space-xs` by `space-sm`. It scrolls sideways when wider than the column.
- **A rule** is a `line` hairline with `space-lg` above and below.
- **A link** is `accent`, underlined at `hairline`, so it never depends on color alone. Its tooltip gives its address. Under the pointer its underline turns `ink`; with keyboard focus it takes an `ink` edge with `radius-sm`.
- **What is not shown as itself**: raw HTML shows as the text it is, and an image shows as its alternative text, styled as a link to the image's address (ADR 0034).

Streaming text fades in chunk by chunk as `DESIGN.md` (Motion) says. A construct still open mid-stream, such as a code block whose closing fence hasn't arrived, shows as what it is so far and closes in place, so nothing jumps when the rest arrives.

## Tool cards

A tool card is one row for one tool call, as on the Main board.

- **The row** is `material-tile` with a `line` edge, `radius-md`, padded `space-sm` by `space-md`, its parts `space-sm` apart, in `ui` and `ink`. From the leading edge: the state mark, the tool's name, its target in `mono`, then the detail at the trailing edge in `ui-small`, `ink-faint` and tabular figures.
- **The state mark** is `size-icon`. Running, it is the ring at `size-icon` turning once per `motion-loop-ring` with `motion-linear`, as on the board. Waiting for the user's permission, a glyph at `size-glyph` in `caution`. Done, a check in `ink-muted`. Failed, a glyph at `size-glyph` in `danger`. Each change crossfades over `motion-quick`.
- **The target** is the one thing the call acts on, as `PROTO-AGENT-EVENTS` carries it: a path, a command, a search, an address. A path inside the session's workspace folder is shown relative to it, whichever separators either uses. It is cut with an ellipsis where it would overflow: a path at its start, so the file's own name stays in view, anything else at its end.
- **The detail** is, while the call runs live, the backend's progress text when it gives one, else the elapsed time; once done, the call's duration: the time from its request to its result, as their events' `ts` give them. Times are written as `CONTENT.md` says (42s, 3m 5s) and show from 1s. Failed, it reads "Failed" in `danger`.
- **Opening a card**: the row is a control with an edge (`DESIGN.md`, Pointer and press): under the pointer its edge turns `ink-faint` and its fill steps up to `surface-raised`, pressed it sinks to `canvas`, and with keyboard focus its edge turns `ink`. Choosing it opens the card below the row, growing over `motion-settle` with `motion-ease-out`, and a chevron at `size-icon-xs` in `ink-faint` after the detail turns to show it. Choosing it again closes it the same way.
- **The open card** holds the input and then the output, each under a label in `ui-small` and `ink-muted` ("Input", "Output", or "Error" for a failed call), `space-sm` apart, padded `space-sm` by `space-md`, divided from the row by a `line` hairline. The input is its fields, each name in `ui-small` and `ink-muted` above its value in `mono`. The output is `mono` in `ink` on `surface` with `radius-sm`, padded `space-sm`, its lines kept and wrapping, scrolling past `size-card-max`.
- **A subagent's calls** sit inside the card of the call that started them, in its open card above its output, as tool cards of their own, `space-xs` apart.

## Diff cards

A diff card is a tool card whose call changes files: the same row, then the change, open from the start.

- **The row** reads as a tool card's, with the file's path as the target. In place of the duration its detail is the lines added and removed, "+12 −3", the added count in `positive` and the removed count in `danger`, each figure after its sign, so the counts never depend on color alone. A call that changes several files has one change per file under one row, which names the first file and how many more, from a catalog pattern such as "{path} and {count} more".
- **The change** is `mono` on `surface`, under a `line` hairline, scrolling past `size-card-max` and sideways when a line is wider than the column. Each line leads with its sign in a gutter `space-lg` wide, in `ink`: "+" on `material-diff-added`, "−" on `material-diff-removed`, and a space for a line kept, whose text is `ink-muted`. The tints carry the color; the signs, in `ink`, keep their contrast on them. A line's text is `ink` whatever its fill. A file written whole is all added lines. The diff has no line numbers: the call carries the text it replaces, not where it sits in the file.
- **Closing** works as a tool card's; the chevron says it is open.
- **States**: while the call waits for the user's permission, the change is what the agent proposes, the state mark is the `caution` glyph, and the detail keeps the counts. The permission card beneath it is `FLOW-APPROVE-PERMISSION`'s and unchanged. A failed call keeps its change, with the detail reading "Failed" in `danger`, so the user sees what was not applied.

## Thinking

A thinking block is a quiet row with no card: the state mark (the ring while thinking, nothing once done) and "Thinking…" or "Thought" in `ui-small` and `ink-muted`. When the backend gives a summary, the row opens as a tool card does and shows it in `prose` and `ink-muted`.

With reduced motion on, nothing here moves or fades: cards open and close at once, streamed text appears whole, and the ring holds still at full tone.
