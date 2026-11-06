# The rail

How the rail, a project heading and a tile look, token by token. `LAY-SHELL` (The rail) says what the rail contains and how it behaves; `DESIGN.md` holds the tokens and the rules every surface follows. Read this before changing any of the three.

The owner reviewed this design as a mockup. Its renders are in `reference/`: `rail-states-dark.png` and `rail-states-light.png` show the states, and `shell-dark.png` shows the rail in the window. The renders also draw parts that are not specified yet: the shelf, a tile's More menu, and the title bar with the Ersa menu at the top of the rail. Build what this page and `LAY-SHELL` describe; where a render and this page differ, this page wins. Once the gallery shows a state, the gallery is the reference for it, and the renders are deleted when it shows them all.

## The rail

- The rail is `size-rail` wide on `material-rail`, with a `line` hairline where it meets the transcript. Its parts are padded and spaced by `space-sm`.
- **New session** is the button every other surface uses, as wide as the rail, at its top, with a plus icon at `size-icon` before its label.
- **The tree** scrolls between New session and the foot. Projects sit `space-sm` apart, and the tiles inside one `space-xs` apart.
- **Notes** (loading, no projects, a list that could not be read) are `ui` text in `ink-muted`; a hint or a reason beneath one is `ui-small`. A list that could not be read leads with a danger glyph at `size-glyph`.
- **The foot's problem** is a note like those, with no tile and no fill. While Ersa is trying again it follows a ring at `size-glyph` in `ink-muted`, open by a quarter so its turning shows, turning once per `motion-loop-ring`; once Ersa has stopped trying, a danger glyph. Its reason sits beneath in `ui-small`, and Retry beneath that is the button every other surface uses; both start under the note's words, not under its glyph. It fades in over `motion-settle` with `motion-ease-out`. "Attached to" and its label are one line of `ui-small` in `ink-muted`.

A tile, a project heading and an icon button are controls without an edge, and follow `DESIGN.md`'s rule for one (Pointer and press). Their fills change over `motion-quick` with `motion-ease-out`. Unavailable, one reads `ink-faint` and does not change under the pointer.

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

**The active session's tile** takes `material-open-session` and a `line` edge, and does not change under the pointer or when pressed: it is already chosen. With keyboard focus its edge turns `ink`, as any tile's does.

With reduced motion on, nothing here moves or fades: the arc, the foot's ring and the working glyph hold still at full tone, a chevron appears without its breath, and a heading's tiles and buttons appear and go at once.
