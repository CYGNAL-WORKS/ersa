# The rail

How the rail, its headings, a tile and their menus look, token by token. `LAY-SHELL` (The rail) says what the rail contains and how it behaves; `DESIGN.md` holds the tokens and the rules every surface follows. Read this before changing any of the three.

The owner reviewed this design as a mockup. Its renders are in `reference/`: `rail-states-dark.png` and `rail-states-light.png` show the states, and `shell-dark.png` shows the rail in the window. The renders also draw parts that are not specified yet: the shelf and the title bar with the Ersa menu at the top of the rail. The owner's design canvas (its Rail menus artboard) draws the arrange menu, a tile's menu with Move to section open, the Pinned group, a section heading and the delete question. Build what this page and `LAY-SHELL` describe; where a render and this page differ, this page wins. Once the gallery shows a state, the gallery is the reference for it, and the renders are deleted when it shows them all.

## The rail

- The rail is `size-rail` wide on `material-rail`, with a `line` hairline where it meets the transcript. Its parts are padded and spaced by `space-sm`.
- **New session** is the button every other surface uses, at the rail's top, with a plus icon at `size-icon` before its label. It fills the row but for **the arrange button** at its trailing side, `space-xs` apart: an icon button `size-control` square with a sliders icon at `size-icon-sm`, reading `ink` and `material-pressed` while its menu is open.
- **The tree** scrolls between New session and the foot. Groups sit `space-sm` apart, and the tiles inside one `space-xs` apart.
- **Notes** (loading, no projects, a list that could not be read) are `ui` text in `ink-muted`; a hint or a reason beneath one is `ui-small`. A list that could not be read leads with a danger glyph at `size-glyph`.
- **The foot's problem** is a note like those, with no tile and no fill. While Ersa is trying again it follows a ring at `size-glyph` in `ink-muted`, open by a quarter so its turning shows, turning once per `motion-loop-ring`; once Ersa has stopped trying, a danger glyph. Its reason sits beneath in `ui-small`, and Retry beneath that is the button every other surface uses; both start under the note's words, not under its glyph. It fades in over `motion-settle` with `motion-ease-out`. "Attached to" and its label are one line of `ui-small` in `ink-muted`.

A tile, a group heading, a menu item and an icon button are controls without an edge, and follow `DESIGN.md`'s rule for one (Pointer and press). Their fills change over `motion-quick` with `motion-ease-out`. Unavailable, one reads `ink-faint` and does not change under the pointer.

## A group heading

A project heading is drawn below; the other headings are the same row with less in it. **Pinned** leads with a pin icon at `size-icon-sm` in `ink-faint` after the chevron. **A section heading** names the section and, under the pointer or keyboard focus, shows More alone at the trailing edge; renaming turns its name into a field in place, `size-control-sm` high so the heading keeps its height, `ui-small` at the heading weight on `surface`, `radius-md`, with a `line` edge that turns `accent` while focused. **A date heading** and "No section" have no actions. Grouped by none, the tiles after the Pinned group have no heading.

## A project heading

- One row, `size-control` high, in `ui-small` at the heading weight and `ink-muted`; under the pointer it reads `ink`.
- From the leading edge: a chevron at `size-icon-sm` in `ink-faint`, pointing down when the project is expanded and right when it is collapsed; the project's name, cut with an ellipsis; then the host's name at the regular weight when the project is not on the local machine.
- Toward the trailing edge, at the regular weight: "Not trusted" after a shield icon at `size-icon-sm` in `caution`, when the project is not trusted; and, when the project is collapsed, how many sessions it holds, after a glyph at `size-glyph` in the tone of what they want when any of them needs the user. A collapsed project that is not trusted shows both, "Not trusted" first.
- New session and More are icon buttons at the trailing edge, `size-control` square, in `ink-muted`, reading `ink` under the pointer. They fade in and out over `motion-quick` with `motion-ease-out` as the pointer or keyboard focus enters and leaves the heading. Their place is kept while they are hidden, so nothing in the heading shifts.
- Collapsing and expanding, the tiles slide closed or open and fade over `motion-settle` with `motion-ease-out`, and the chevron turns over `motion-quick` with `motion-ease-out`.

## A tile

A tile is two lines with no box at rest, padded `space-sm`. Line two keeps its height when it has nothing to show, so every tile is the same height.

| | Leading, `size-icon-sm` wide | Middle | Trailing |
|---|---|---|---|
| Line one | the backend's icon, `ink-muted` | the title in `ui`, cut with an ellipsis | the state mark, in a `size-control-sm` square |
| Line two | the context ring at `size-ring-sm`, centered under the icon | a phrase, then the cost, in `ui-small` with tabular figures | |

**The state mark**, by the session's state:

| State | Mark | Phrase on line two |
|---|---|---|
| Starting | an arc in `accent` at `size-icon-xs`, turning once per `motion-loop-ring` | "Starting" |
| Being deleted | the same arc in `ink-muted` | "Deleting" |
| Working | a glyph at `size-glyph` in `accent`, breathing (`DESIGN.md`, Motion) | none |
| Needs you | a chevron at `size-icon-sm` pointing right: `caution` for a permission request, `accent` for a question | the want, in the chevron's tone |
| Idle | nothing | none |
| Disconnected | a hollow ring at `size-glyph` with a `hairline` edge in `ink-faint` | "Disconnected" |
| Error | a glyph at `size-glyph` in `danger` | "Error" |
| Ended | nothing | "Ended" |
| One Ersa doesn't know | nothing | "Unknown state" |

- **Line two** is `ink-muted`: the phrase, then a middle dot, then the cost. The dot stands only between a phrase and a cost.
- **The title** is `ink`, and `ink-muted` when the session is idle or has ended. An unseen session's title reads at the heading weight in `ink`.
- **No text on a tile is `ink-faint`.** A tile's fills under the pointer, pressed and active are darker than the surfaces `ink-faint` is measured against.
- **The context ring** is the session controls' ring at a smaller size: a `line` track and an arc in `ink-muted`, which turns `caution` when the ring is near full (`LAY-SHELL`).
- **The backend's icon** is each backend's own mark. Until the vendors' brand terms are checked, every backend shows one neutral icon.

**More** fades in over the state mark, in the same `size-control-sm` square, under the pointer or keyboard focus, over `motion-quick` with `motion-ease-out`; the mark returns when it goes. While the tile's menu is open, More stays, reading `ink` on `material-pressed`. A session that needs the user keeps its chevron, and More sits before it in a second `size-control-sm` square whose place is kept while More is hidden, so the title never shifts.

**An archived tile**, shown when the rail shows archived sessions, has an archive icon at `size-icon-xs` in `ink-muted` as its mark, and its title reads `ink-muted`.

**Renaming on the tile**, the title becomes a field in its place: `ui` on `surface`, with a `line` edge that is `accent` while focused, `radius-md`, the tile's height unchanged.

**The active session's tile** takes `material-open-session` and a `line` edge, and does not change under the pointer or when pressed: it is already chosen. With keyboard focus its edge turns `ink`, as any tile's does.

## Menus

The arrange menu and a tile's menu are one kind of floating layer. It sits above the rail by `stack-menu`, on `surface` with a `line` edge, `radius-lg` corners and `shadow-transient`, padded `space-xs`, as wide as its widest item. A tile's menu opens below the tile's More button, aligned to its trailing edge, or at the pointer for a right-click; the arrange menu opens below its button. Each opens and closes with a fade over `motion-quick` with `motion-ease-out`. Opened from the keyboard, focus goes to the first item it can take; from the pointer, to the menu. Arrow keys move through the items, Right and Left open and close a submenu, Escape closes the innermost menu, and closing a menu returns focus to what opened it: More, the arrange button, or the tile for a right-click or the context-menu key. Choosing an item closes every menu.

- **An item** is a row `size-control` high, padded `space-sm`, in `ui`, `radius-md`, with an icon at `size-icon-sm` in `ink-muted` before its label. Under the pointer or keyboard focus it takes `material-hover`. A submenu's item ends with a chevron at `size-icon-sm` in `ink-faint` and opens its submenu beside the menu, top edges aligned. A chosen option in a submenu shows a check at `size-icon-sm` in `ink` where the icon goes.
- **An unavailable item** reads `ink-faint`, its icon too, and does not change under the pointer; its reason sits beneath its label in `ui-small` in `ink-muted`, and the row grows to hold it.
- **Groups of items** are parted by a `hairline` `line` rule with `space-xs` above and below. Delete stands apart in its own group, its label and icon in `danger`.
- **The submenu's own actions** ("New section…" after the sections and "No section") sit after a rule.

## The delete question

A dialog centered over the window, which is not dimmed, above every other layer by `stack-dialog`, on `material-tile` with a `line` edge, `radius-lg` corners and `shadow-transient`, padded `space-lg`, `size-dialog` wide at most. From the top: the question in `heading` naming the session; what goes with it in `ui`, `ink-muted`, the branch's name in `mono`; then, when work would be lost, a line led by a glyph at `size-glyph` in `caution` that says how many uncommitted and untracked files, ignored files and unshared commits will be lost, in `ui` in `ink`; then the buttons at the trailing edge, `space-sm` apart: Cancel, the button every other surface uses, and the confirming button, the same button with its label in `danger`, never a danger fill. A failed deletion shows its reason in place of the counts, led by a danger glyph, and the confirming button reads Try again. The dialog fades in over `motion-settle` with `motion-ease-out`; keyboard focus starts on Cancel and stays within the dialog. While the deletion runs, the confirming button shows the ring at `size-icon-xs` before its label, as work of unknown length does, and both buttons are unavailable; Escape does nothing until it ends. Escape or Cancel otherwise closes the dialog and returns focus to the tile. After a deletion, focus goes to the next tile in the rail, else the previous, else New session; after a failure, to Try again.

With reduced motion on, nothing here moves or fades: the arc, the foot's ring and the working glyph hold still at full tone, a chevron appears without its breath, a heading's tiles and buttons appear and go at once, menus and the delete question appear at once, and tiles take their new places at once when the rail is regrouped or re-sorted.
