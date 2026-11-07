# The composer

How the composer and the session controls look, token by token. `FLOW-SEND-MESSAGE` says how the composer behaves, `FLOW-NEW-SESSION` how the session controls choose where and how a session runs, and `LAY-SHELL` where both sit; `DESIGN.md` holds the tokens and the rules every surface follows. Read this before changing the composer or the session controls.

The owner reviewed this design as mockups on the design canvas: the Composer board draws the composer's states, and the Place picker board draws the project and workspace lists, the no-projects state and the trust question. Build what this page and the briefs describe; where a board, the built components or the gallery differ from this page, this page wins. The chips built before this page have an edge; the ticket that restyles the session controls changes them, and the gallery follows. The Composer board also draws a microphone for dictation, which is not specified (FLOW-SEND-MESSAGE, Open questions): leave it out.

## The composer

The composer is one column, `size-measure` wide at most and centered, holding from top to bottom: the status line, the box and the session controls, `space-xs` apart. In a session it sits on the transcript's canvas with no line between them: the transcript's last `space-xxxl` fades out behind it.

- **The box** holds what the user writes: the attach button on the leading edge, the words, then send. It is `material-composer` with a `line` edge at `hairline`, `radius-md`, padded `space-sm`, its parts `space-sm` apart and aligned to the bottom, so the buttons stay by the last line as the words grow.
- **The words** are `prose` in `ink`, padded `space-xs` by `space-sm`, with no edge of their own; the placeholder is `ink-faint`. The field is three lines tall on the new-session screen and one line in a session, and grows with the words.
- **Attach** is a quiet icon button, `size-control-lg` square, `radius-md`, with a plus icon at `size-icon` in `ink-muted`. It is a control without an edge (`DESIGN.md`, Pointer and press): under the pointer it takes `material-hover` and reads `ink`, pressed or while its menu is open it takes `material-pressed` and reads `ink`, and with keyboard focus its edge turns `ink`.
- **Send** is the primary button made square: `size-control-lg`, `radius-md`, `material-primary`, with an up arrow at `size-icon` in `accent-ink`. Under the pointer `material-hover` lies over its fill, and pressed `material-pressed`; with keyboard focus it takes an `ink` edge. With nothing to send it has no fill, a `line` edge and an `ink-faint` arrow, and does not change under the pointer.
- **Stop** takes send's place from the moment the user sends until the turn ends: the same square on `material-tile` with a `line` edge, holding a filled square at `size-icon-sm` in `ink`. It is a control with an edge (`DESIGN.md`, Pointer and press), and with keyboard focus its edge turns `ink`. When stop turns back into send, keyboard focus stays on the button.

Their fills and edges change over `motion-quick` with `motion-ease-out`. Send becoming stop and back crossfades over `motion-quick`.

## Focus

- **Focused**, the box's edge is `accent` and it carries `glow-focal`, fading in over `motion-settle` with `motion-ease-out`: it is the screen's one focal glow.
- **Focused while a permission request is open**, the box's edge alone is `accent`; the glow stays on the request (`DESIGN.md`, Elevation & Depth).
- **Under the pointer**, unfocused, its edge is `ink-faint`.

## The status line

One line above the box, `size-control` high at least, showing one message at a time, first that Ersa can't reach the sessions, then a send whose outcome Ersa can't tell, then a refused message, then detecting backends, then a host with no backend, then a send on its way; padded `space-sm` at the sides, in `ui-small` and `ink-muted`. It is empty, and keeps no height, while there is nothing to say. The composer keeps its box and session controls where they are: the status line opens upward, growing and fading in over `motion-settle` with `motion-ease-out`, and closes the same way.

| When | It says | Leading mark | The box |
|---|---|---|---|
| Ersa can't reach the sessions | that, and that it is trying again | none | unavailable |
| Ersa can't tell whether a message was sent | that, and that the transcript shows it if it arrived | a glyph at `size-glyph` in `caution` | as at rest, the message kept |
| A message was refused | the reason, in the user's terms | a glyph at `size-glyph` in `danger` | as at rest, the message kept |
| Backends are being detected | that it is detecting the backends on this host | none | unavailable |
| The host lists its backends and none can run (a failed listing is FLOW-NEW-SESSION-R11's) | With every backend `missing`: "No backend is available on this host. Install Claude Code or GitHub Copilot on it to start a session." With any backend in `error`: that backend by its label and what to do, such as "Claude Code isn't working on this host. Check that it's installed and signed in, then open New session again." (`backend.list` carries no reason for an error yet) | none | unavailable |
| A message is on its way | how much is copied, as a share, or that the backend is starting | the ring at `size-ring`, its arc filling to the share over `motion-settle` | as at rest, stop offered |

A refused message's line is announced to assistive technology as an alert. When the box becomes unavailable, keyboard focus returns to the words once it is available again. **Unavailable**, the box is `canvas`, the words and their placeholder read `ink-faint`, and attach and send read `ink-faint` with no fill and do not change under the pointer.

## Attachments

Attachments sit in the box above the words, across its full width, wrapping, `space-sm` apart, padded `space-xs` at the top and sides.

- **An attachment** is `size-control-lg` high on `material-tile`, with a `line` edge and `radius-md`, padded `space-sm` at the leading edge and `space-xs` at the trailing one, its parts `space-sm` apart, in `ui-small`. From the leading edge: its icon at `size-icon-sm` in `ink-muted` (an image, a file or a folder), its name in `ink` cut with an ellipsis, its detail in `ink-faint` (a size such as 84 KB, a line count such as 212 lines, or "folder"), and remove.
- **Remove** is an icon button `size-control-sm` square, `radius-sm`, with a cross at `size-icon-xs` in `ink-faint`; under the pointer it takes `material-hover` and reads `ink`, pressed `material-pressed`, and with keyboard focus its edge turns `ink`. Removing one moves keyboard focus to the next attachment's remove, else to the words.
- **One Ersa will copy** (`attachment.inspect`'s `copied`, or any attachment for a new workspace) says, after its detail, that it will be copied to the workspace's `.ersa` folder, in `ink-faint`.
- **One Ersa can't send** shows a glyph at `size-glyph` in `danger` in place of its icon, and its detail gives the reason in `danger`.
- **Dragging over the box**, its edge is `accent`, with no glow, until the drop or the drag leaves.
- An attachment fades in over `motion-quick` as it is added and out as it is removed.

## The attach menu

The attach menu is a chip's list (below) that opens upward from attach, `space-xs` above it. Its options lead with an icon at `size-icon-sm` in `ink-muted` in place of the option mark: Add files…, Add images…, Add a folder…. A note beneath them, in `ui-small` and `ink-faint`, padded `space-xs` by `space-sm`, says that attachments can also be pasted or dropped on the box.

## The session controls

One row beneath the box, `size-control` high at least, in `ui-small`, its chips `space-xs` apart. The where chips (host, project, workspace) sit at the leading edge; the how chips (backend, model, effort, permission mode) and then the context ring sit at the trailing edge.

- **A chip** is a pill: `size-control` high, `radius-pill`, padded `space-sm` at the sides, with no fill and a `hairline` edge in no color at rest, its text `ink-muted`. A where chip leads with its icon at `size-icon-xs` in `ink-faint` (a monitor for the host, a folder for the project, a branch for the workspace). A chip that opens a list ends with a down chevron at `size-icon-xs` in `ink-faint`. A chip with a prefix, such as Effort, sets the prefix in `ink-faint` before its value.
- A chip is a control without an edge (`DESIGN.md`, Pointer and press). **Under the pointer** it takes `material-hover` and reads `ink`; **pressed**, `material-pressed`; **open**, `surface-raised` with a `line` edge, reading `ink`, whatever the pointer does; **with keyboard focus**, an `ink` edge. Unavailable, it reads `ink-faint` and does not change under the pointer.
- **In a session**, the chips are the same pills without the chevron or any change under the pointer: they show the session's values and do not open.
- **Detecting backends**, the how chips are replaced by one unavailable chip led by the context ring's track with a quarter arc in `ink-muted`, turning once per `motion-loop-ring` with `motion-linear`, saying that backends are being detected.
- **The context ring** is `size-ring`, a `line` track with an arc in `ink-muted` at the gauge's stroke, then the share used in tabular figures, `space-sm` apart. Near full (`LAY-SHELL`) the arc turns `caution`. Before anything is used it is the track alone, with no figure.

## A chip's list

A chip's list floats `space-xs` from its chip, beneath it where the window has room and above it where it does not. It is `surface` with a `line` edge, `radius-lg`, `shadow-transient`, padded `space-xs`, at least as wide as its chip and at most `size-menu`, on `stack-menu`. It fades and slides `space-xs` toward its place over `motion-quick` with `motion-ease-out`, and leaves the same way.

Opening a list moves keyboard focus to its chosen option, or to the workspace list's search box; the arrow keys move among the options, Enter chooses one, and choosing or Escape closes the list and returns focus to its chip. The attach menu does the same, returning focus to attach.

- **An option** is `size-control` high, `radius-md`, padded `space-sm` at the sides, in `ui`: the option mark, its label in `ink`, and a status at its trailing edge in `ui-small` and `ink-faint`. An unavailable option reads `ink-faint`, keeps its status, and does not change under the pointer. The chosen option's mark is a dot at `size-option-mark` in `accent`; the others leave its place empty. An option is a control without an edge: under the pointer it takes `material-hover`, pressed `material-pressed`, and with keyboard focus an `ink` edge, so the focused option stays distinct from the one under the pointer.
- **An action at the end of a list**, such as Add project…, sits below a `line` hairline with `space-xs` above and below it, and leads with a plus icon at `size-icon-sm` in `ink-muted` in the mark's place. Its shortcut, Ctrl+O, sits at the trailing edge in `ui-small` and `ink-faint`.

## The project list

The project chip's list is the widest list, `size-menu` wide. Each project is an option two lines tall, padded `space-xs` top and bottom:

- **Line one**: the option mark, the project's name in `ui` and `ink`, then at the trailing edge its kind, Git or Folder, in `ui-small` and `ink-faint`. A project that is not trusted shows "Not trusted" after a shield icon at `size-icon-sm` in `caution`, in `ui-small` and `ink-muted`, before its kind.
- **Line two**: the folder's path in `ui-small` and `ink-faint`, under the name, cut with an ellipsis at its start so the folder's own name stays in view.

Add project… ends the list. **With no projects** the chip reads Choose a folder in `ink` with its folder icon, and opening it shows Add project… alone.

## The workspace list

Two options, New workspace and Project folder, each with a status in `ink-faint`: "Isolated" and "Shared". For a git project with New workspace chosen, a `line` hairline follows them and then the base branch:

- A heading, "Base branch", in `ui-small` and `ink-muted`, padded `space-xs` by `space-sm`.
- **The search box**: `size-control` high, `radius-md`, on `canvas` with a `line` edge, a search icon at `size-icon-sm` in `ink-faint` inside its leading edge, its text `ui` in `ink` and its placeholder `ink-faint`. Focused, its edge is `accent`, with no glow. Typing narrows the branches beneath it.
- **The branches**, as options with the option mark, names in `ui`, and "Default" as the status of the repository's default branch. They scroll past `size-list-max`, eight rows. With nothing matching, a note in `ui-small` and `ink-muted` says no branch matches. While the branches are read, the same note says "Reading the branches…"; when they can't be read, it leads with a danger glyph at `size-glyph`, as the rail's unreadable list does, and says "Ersa couldn't read this project's branches. The new workspace will start at the default branch." (FLOW-NEW-SESSION-R28). In both, the search box is unavailable and opening the list moves keyboard focus to the chosen option. When the branches arrive, the list grows to hold them over `motion-quick` with `motion-ease-out`.

The chip reads the choice: "New workspace from" and the branch, or "Project folder". With New workspace chosen and no branch to name, while the branches are read or when they can't be, it reads "New workspace" (FLOW-NEW-SESSION-R28).

## The trust question

After a folder is picked, the trust question Ersa asks when a project is added opens above the composer's box, in the status line's place. It reads like the Claude Code CLI's folder-trust prompt, in Ersa's words, from top to bottom:

- **Title**: "Trust" and the folder's name, as a question, in `ui` at the heading weight.
- **Path**: the folder's full path in `mono` and `ink-muted`.
- **Safety check**: "Is this a project you created or one you trust, like your own code, a well-known open source project, or work from your team? If not, review what's in this folder first."
- **What agents can do**: "Agents will be able to read, edit, and run files here."
- **Answers**, in a row at its foot: Cancel, then Trust. Neither is primary: unlike other confirmations (CONTENT.md), trusting is never the default.

The text below the path is `ui-small` in `ink-muted`. The card is on `material-tile` with a `line` edge, `radius-lg`, padded `space-lg`. While it is open it holds the focal glow and the box shows its focus by its edge alone. It opens upward as the status line does, fading and growing over `motion-settle` with `motion-ease-out`, and leaves the same way. Keyboard focus moves to Cancel, so Enter cancels until the user moves to Trust; the arrow keys and Tab move between the answers, and Escape cancels. Once it is answered or canceled, and when the folder dialog is canceled, keyboard focus returns to the composer's words.

With reduced motion on, nothing here moves or fades: the glow, the status line, attachments and lists appear and go at once, and the detecting ring holds still at full tone.
