# The settings page

How the settings page looks, token by token. `LAY-SHELL` (The settings page) says where it sits, what opens it and what it hides; `FLOW-CLEAN-UP` says what Storage holds and does; `DESIGN.md` holds the tokens. Read this before changing the page or adding a section or a setting to it.

The page follows Claude Desktop's settings: the heading, the names of the sections at the leading side, and the chosen section's settings beside them, each a row with its name and what it does at the leading side and its control at the trailing side. Where Desktop shows nothing that settles a detail, the page follows VS Code's settings editor, and says so below. No board on the design canvas draws it, so this page is the reference: where the built page or the gallery differ from it, this page wins. The owner reviews it in the gallery, in both themes.

## The page

- **The frame** is the sessions page's (`RAIL.md`, The sessions page): in the transcript area on its canvas, its content `size-measure` wide and centered, padded `space-xxl` from the top. It fades in over `motion-settle` with `motion-ease-out`, and at once with reduced motion. It leaves at once when a tile, New session or View all is chosen, as the sessions page does.
- **"Settings"** is in `heading` in `ink`. Beneath it, `space-xl` apart, two columns `space-xxl` apart: the section names, as wide as the longest of them, and the chosen section, taking the rest.
- **A section name** is a control without an edge in `ui`, `size-control` high, padded `space-sm` at its sides, with `radius-md` corners: the chosen one on `material-pressed` in `ink`, the others in `ink-muted`. Under the pointer it takes `material-hover`, pressed `material-pressed`, and with keyboard focus its edge reads `ink`; its fill changes over `motion-quick` with `motion-ease-out`. They look as the sessions page's tabs do, stacked, so the eye learns one shape. The names are a list of buttons one under the other in a navigation landmark, the chosen one marked current, `space-xs` apart, as Desktop's and VS Code's are. Storage is the first and the one the page opens on.

## A section

- **Its heading** names the section in `ui-small` at the `heading` weight in `ink-muted`, as a project heading in the rail is, `size-control` high so its text lines up with the chosen section name beside it. "Settings" holds the page's one `heading`.
- **A setting** is a row beneath the heading, padded `space-md` above and below, with a `hairline` `line` rule between settings, as Desktop and VS Code separate theirs. At the leading side, its name in `ui` in `ink`, and beneath it, `space-xs` apart, what it does in `ui-small` in `ink-muted`, wrapping to as many lines as it needs. At the trailing side, `space-xl` from the words and centered on them, its control.
- **A choice among a few values** is a chip (`COMPOSER.md`, The session controls), Ersa's one control that opens a list of choices: Desktop shows a dropdown there. Its list opens beneath it, or above where the window has no room, and marks the value in force.

## A setting's states

- **Loading.** Until the settings are read, the chip shows the context ring's turning arc before "Loading…", and does not open. The ring holds still with reduced motion.
- **Can't be read.** There is no value to show, so the chip is not drawn, and `space-sm` beneath the row's words a problem says that the setting can't be read and why, drawn as the rail's notes draw a problem: the danger dot before the lead in `ui` in `ink-muted`, and the reason beneath it in `ui-small`. It starts where the words do and fades in over `motion-quick` with `motion-ease-out`, at once with reduced motion.
- **Changing.** A choice shows at once and is sent. While Core answers, the chip keeps its look, its value and keyboard focus, and does not open, as a session control does while its change is under way; assistive technology hears it is busy.
- **Change refused.** The chip goes back to the value in force, and beneath the row a problem says the setting wasn't changed and why, drawn as above. It goes when a change is made.
- **While Ersa can't reach Core.** The chip keeps the value last read and is unavailable: `ink-faint`, unchanged under the pointer. The rail's foot says why, so the page adds nothing.
- **Hover, pressed and focus** on the chip are the chip's own (`COMPOSER.md`).

## Storage

Storage holds Archive inactive sessions, and the Clean up list will follow it (`FLOW-CLEAN-UP`).

- **Archive inactive sessions**: "Archive a session, as Archive does, once it has had no activity for this long. Running, pinned, and unread sessions are never archived." The chip names the setting for assistive technology and offers Never, 1 day, 2 days, 7 days, 14 days and 30 days, Desktop's values. The words say what the setting does without promising that every idle session goes, since the sweep leaves a running session and some others alone (FLOW-CLEAN-UP, step 4), and leave the folder to Archive, which puts it away only when nothing else needs it (step 1).
