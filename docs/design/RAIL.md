# The rail

How the rail, a project heading and a tile look, token by token. `LAY-SHELL` (The rail) says what the rail contains and how it behaves; `DESIGN.md` holds the tokens and the rules every surface follows. Read this before changing any of the three.

The owner reviewed this design as a mockup. Its renders are in `reference/`: `rail-states-dark.png` and `rail-states-light.png` show the states, and `shell-dark.png` shows the rail in the window. The renders also draw parts that are not specified yet: a tile's More menu, and the title bar with the Ersa menu at the top of the rail. Build what this page and `LAY-SHELL` describe; where a render and this page differ, this page wins. Once the gallery shows a state, the gallery is the reference for it, and the renders are deleted when it shows them all.

## The rail

- The rail is `size-rail` wide on `material-rail`, with a `line` hairline where it meets the transcript. Its parts are padded and spaced by `space-sm`.
- **New session** is the button every other surface uses, as wide as the rail, at its top, with a plus icon at `size-icon` before its label.
- **The tree** scrolls between New session and the foot. Projects sit `space-sm` apart, and the tiles inside one `space-xs` apart.
- **Add project** is a quiet row at the end of the tree: `size-control` high, `ui-small` in `ink-muted`, a plus icon at `size-icon-sm` before its label.
- **Notes** (loading, no projects, a list that could not be read) are `ui` text in `ink-muted`; a hint or a reason beneath one is `ui-small`. A list that could not be read leads with a danger glyph at `size-glyph`.
- **The foot's problem** is a note like those, with no tile and no fill. While Ersa is trying again it follows a ring at `size-glyph` in `ink-muted`, open by a quarter so its turning shows, turning once per `motion-loop-ring`; once Ersa has stopped trying, a danger glyph. Its reason sits beneath in `ui-small`, and Retry beneath that is the button every other surface uses; both start under the note's words, not under its glyph. It fades in over `motion-settle` with `motion-ease-out`. "Attached to" and its label are one line of `ui-small` in `ink-muted`.

A tile, a project heading, Add project and an icon button are controls without an edge, and follow `DESIGN.md`'s rule for one (Pointer and press). Their fills change over `motion-quick` with `motion-ease-out`. Unavailable, one reads `ink-faint` and does not change under the pointer.

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

## The shelf

The shelf is drawn in the Rail mockup's state "the shelf, pulled out of its tile". `FLOW-APPROVE-PERMISSION` says what it holds and when it opens and closes. Its tone is the shown request's: `accent` for a question, and `caution` for a permission request or one Ersa can't show. While the shelf is open, the tile's chevron and want take that tone too, so the shape has one.

- **One shape.** While its shelf is open, the tile takes `surface` and a `hairline` edge in the tone, open at its trailing side where the shelf joins it, and its trailing corners are square. The tile does not change under the pointer or when pressed while its shelf is open. The shelf starts at the tile's top edge and continues the shape: `surface`, the same edge, `radius-lg` corners except the one that meets the tile, which is square. When the shelf rises to fit the window, that corner is `radius-lg` too.
- **The layer.** The shelf is `size-shelf` wide, stacks at `stack-shelf`, and casts `shadow-transient`, as a transient layer does. It carries no focal glow: its edge in the tone is its mark, and the screen's one glow stays where it was. With keyboard focus on the shelf itself, its edge and the open tile's turn `ink`. Its parts are padded and spaced `space-md`.
- **The chevron** turns to point back toward the rail while its shelf is open, and takes `material-pressed`, which does not change under the pointer.
- **The heading line** is `ui-small` in `ink-muted`: the kind of request; and at its trailing end, when more than one request is pending, the place ("1 of 3") in tabular figures.
- **What is asked** is `ui` at the heading weight in `ink`. A question's text is `ui` in `ink`.
- **The details** (a permission request's input) are `mono` on `canvas` with a `line` edge and `radius-sm` corners, padded `space-sm` and `space-md`, and scroll past `card-max`. The blocked path and the reason sit beneath in `ui-small`, `ink-muted`.
- **The readouts** are the context ring at `size-ring`, its percentage, a middle dot, and the cost, in `ui-small` with tabular figures in `ink-muted`; each appears only when the tile shows it.
- **The answers** are the button every other surface uses, `space-sm` apart, on one row at the shelf's foot: Deny at the leading edge, Allow at the trailing edge, and the primary answer is `material-primary`. A suggestion is a button above that row, as wide as the shelf, its label cut with an ellipsis: its label is a sentence the request supplies, too long for the row the mockup drew. For a question, Decline sits at the leading edge and Answer at the trailing edge as the primary.
- **A question** leads with its header, when it has one, in `ui-small` at the heading weight in `ink-muted`. Its options are the rows a floating layer's options use (`radius-md`, at least `size-control` high), the label in `ui` and its description beneath in `ui-small`, `ink-muted`; a chosen one shows a check icon at `size-icon-sm` in `accent`. The user's own words take the text field every other surface uses, beneath the options.
- **Notes** in the shelf (loading, reconnecting, a refused answer and why) are the rail's notes; a refused answer or a read that failed leads with a danger glyph at `size-glyph`, with Retry beneath it when reading failed.
- **Unavailable** answers, while one is on its way or while Ersa can't reach Core, read `ink-faint` on the plain button's material, the primary included, and do not change under the pointer. The answer on its way leads its label with a ring at `size-icon-xs` in `ink-muted`, open by a quarter and turning once per `motion-loop-ring`.

**Motion.** The shelf slides out from under the tile's trailing edge and fades in over `motion-settle` with `motion-ease-out`, and slides back and fades the same way when it closes. The chevron turns over `motion-quick` with `motion-ease-out`. When the shelf moves to the next request, its contents cross-fade over `motion-quick` with `motion-ease-out` while the shelf itself holds still, and its height eases to the new contents over `motion-settle` with `motion-ease-out`. With reduced motion on, the shelf appears and goes at once, the chevron points its new way without turning, the ring on an answer on its way holds still, and the next request replaces the last at once.
