# The workbench and the developer pane

How the workbench and its first pane look, token by token. `LAY-SHELL` says where the workbench sits and what opens it, `FLOW-READ-RAW-LOG` how the developer pane behaves, and `DESIGN.md` holds the tokens. Read this before changing either.

No board on the design canvas draws these, and Claude Desktop has no developer pane, so this page is the reference: where the built components or the gallery differ from it, this page wins. The owner reviews the pane in the gallery, in both themes, once it is built.

## The workbench

- **The region** fills the window's height at its right edge, on `surface`, with a `line` hairline at its leading edge and no radius: panes meet at lines. It is `size-workbench` wide when first opened, never narrower than `size-workbench-min`, and never wider than `size-workbench-max` of the width it shares with the transcript area. When the window narrows, the workbench gives way first, down to its least width.
- **Opening and closing.** Its width grows from nothing over `motion-settle` with `motion-ease-out` while the transcript area narrows over the same, and closing runs it back. With reduced motion on it appears and leaves at once.
- **The resize edge** is the hairline itself. A strip `space-sm` wide, centered on the line, takes the pointer and shows the column-resize cursor. Under the pointer the line turns `ink-faint`; while dragged, and with keyboard focus, `ink`. Dragging follows the pointer with no easing. It is named "Resize workbench"; Left and Right move it by `space-lg`, Home and End to its least and greatest width.
- **The head** is a row `size-control` high, padded `space-sm` by `space-md`, with a `line` hairline beneath: the pane's name in `ui` at the `heading` weight in `ink`, and at the trailing edge Close, an icon button `size-control` square with an x icon at `size-icon-sm`, its tooltip and accessible name "Close". An icon button is a control without an edge (`DESIGN.md`, Pointer and press).
- **In the arrange menu**, "Developer pane" sits above Settings in the last group and shows the check of an item that is on (`RAIL.md`, Menus) while the workbench is open.

## The developer pane

The pane's name is "Developer". Beneath the head, from top to bottom: the toolbar, the list, and the detail when a row is selected.

- **The toolbar** is a row padded `space-sm` by `space-md`, its parts `space-sm` apart: the filter field, which takes the width left over and keeps at least `size-row-title-min`; the count, while the field holds text; then Open diagnostic log.
- **The filter field** is the sessions page's search field (`RAIL.md`), with a filter icon at `size-icon-sm` in `ink-faint` before the words. Its placeholder and accessible name are "Filter lines". Escape empties it.
- **The count** reads "{shown} of {total}" in `ui-small`, `ink-muted` and tabular figures.
- **Open diagnostic log** is a quiet button: its label in `ui-small` and `ink-muted`, `size-control` high, `radius-md`, padded `space-sm`, with no edge. Under the pointer it takes `material-hover` and reads `ink`, pressed `material-pressed`, and with keyboard focus its edge turns `ink`. Unavailable, it reads `ink-faint`, doesn't change under the pointer, and its tooltip gives the reason.
- **A problem** (the raw log can't be read, the diagnostic log couldn't be opened) is one line under the toolbar, padded `space-xs` by `space-md`: a dot at `size-glyph` in `danger`, the message in `ui-small` and `ink`, then the reason in `ink-faint`, cut with an ellipsis and whole in its tooltip. It fades in and out over `motion-quick` with `motion-ease-out`.

### The list

- **A row** is `size-log-row` high, padded `space-md` at the sides, its parts `space-sm` apart, edge to edge with no radius and no rule between rows. From the leading edge: the time as 24-hour `HH:MM:SS.mmm` in `ui-small`, `ink-faint` and tabular figures; the direction mark; the line's start in `mono` and `ink`, on one line, cut with an ellipsis at its end. A row draws at most the line's first 2,000 characters.
- **The direction mark** is an arrow at `size-icon-xs` in `ink-muted`: pointing down and left for a line from the backend, up and right for a line to it. Shape carries the meaning, not color. Assistive technology hears "From {backend}" or "To {backend}", with the backend's own label.
- **Under the pointer** a row takes `material-hover`. **Selected**, it takes `material-open-session`, the rail's quiet highlight for the open session; while the list has keyboard focus the selected row also has an `ink` hairline edge, drawn inside it.
- **Earlier lines are gone**: the first row reads "Earlier lines are no longer held." in `ui-small` and `ink-faint`, centered, and can't be selected.
- **Jump to latest** floats over the list's foot, centered, `space-md` above it: a pill `size-control` high on `surface-raised` with a `line` edge, `radius-pill` and `shadow-transient`, padded `space-md` at the sides, holding an arrow-down icon at `size-icon-sm` and the count of new lines in `ui-small` and `ink` ("3 new lines"). Its accessible name is "Jump to latest". It fades in and out over `motion-quick` with `motion-ease-out`.
- **New rows** appear at once: a row that faded in would blur a fast log, and the list is not prose. Returning to the end, by Jump to latest or End, is at once too.
- **Nothing to list** is said in the middle of the list's area, in `ui` and `ink-muted`, centered, at most `size-dialog` wide. With no active session: "No session is open. Choose a session to see its raw log." With no lines: "No lines yet. Lines appear here while the session runs." Before the first answer: "Reading…" With a filter no line contains: "No lines contain that text."

### The detail

- **The detail** takes the lower half of the pane beneath the toolbar, divided from the list by a `line` hairline, and grows in over `motion-settle` with `motion-ease-out`; the list keeps the selected row in view.
- **Its head** is a row `size-control` high, padded `space-xs` by `space-md`: "From {backend}" or "To {backend}", the time, and the line's size as `CONTENT.md` writes a number with its unit (1.2 KB, 12 MB), in `ui-small` and `ink-muted`, `space-sm` apart. At the trailing edge are Copy, the transcript's Copy (`TRANSCRIPT.md`) with the tooltip and accessible name "Copy line", then an icon button with an x icon named "Close line", which clears the selection.
- **The line** is `mono` in `ink`, padded `space-md`, its lines kept and wrapping wherever they must, so the detail scrolls down and never sideways.
- **A cut line** says above its text, in `ui-small` and `ink-muted`: "This line is {size}. Its first 256 KB are shown."

## Copy

Every string is in the string catalog, under `workbench` and `developer`. Beyond those above: the arrange menu's "Developer pane"; the button "Open diagnostic log"; its unavailable reason, "This build is attached to {label}, so it has no diagnostic log to open."; "Ersa can't read the raw log. It's trying again."; and "Ersa couldn't open the diagnostic log."
