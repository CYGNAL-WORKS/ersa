# The settings page

How the settings page looks, token by token. `LAY-SHELL` (The settings page) says where it sits, what opens it and what it hides; `FLOW-CLEAN-UP` says what Storage holds and does; `DESIGN.md` holds the tokens. Read this before changing the page or adding a section or a setting to it.

The page follows Claude Desktop's settings: the heading, the names of the sections at the leading side, and the chosen section's settings beside them, each a row with its name and what it does at the leading side and its control at the trailing side. Where Desktop shows nothing that settles a detail, the page follows VS Code's settings editor, and says so below. No board on the design canvas draws it, so this page is the reference: where the built page or the gallery differ from it, this page wins. The owner reviews it in the gallery, in both themes.

## The page

- **The frame** is the sessions page's (`RAIL.md`, The sessions page): in the transcript area on its canvas, its content `size-measure` wide and centered, padded `space-xxl` from the top. It fades in over `motion-settle` with `motion-ease-out`, and at once with reduced motion. It leaves at once when a tile, New session or View all is chosen, as the sessions page does.
- **"Settings"** is in `heading` in `ink`. Beneath it, `space-xl` apart, two columns `space-xxl` apart: the section names, as wide as the longest of them, and the chosen section, taking the rest.
- **A section name** is a control without an edge in `ui`, `size-control` high, padded `space-sm` at its sides, with `radius-md` corners: the chosen one on `material-pressed` in `ink`, the others in `ink-muted`. Under the pointer one not chosen takes `material-hover`, and pressed `material-pressed`; the chosen one stays as it is, as the sessions page's chosen tab does. With keyboard focus any of them shows its edge in `ink`. A fill changes over `motion-quick` with `motion-ease-out`. They look as the sessions page's tabs do, stacked, so the eye learns one shape. The names are a list of buttons one under the other in a navigation landmark, the chosen one marked current, `space-xs` apart, as Desktop's and VS Code's are. Storage is the first and the one the page opens on.

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

Storage holds Archive inactive sessions, then Clean up (`FLOW-CLEAN-UP`).

- **Archive inactive sessions**: "Archive a session, as Archive does, once it has had no activity for this long. Running, pinned, and unread sessions are never archived." The chip names the setting for assistive technology and offers Never, 1 day, 2 days, 7 days, 14 days and 30 days, Desktop's values. The words say what the setting does without promising that every idle session goes, since the sweep leaves a running session and some others alone (FLOW-CLEAN-UP, step 4), and leave the folder to Archive, which puts it away only when nothing else needs it (step 1).

## Clean up

Claude Desktop shows one list of the worktrees it can remove and the space freed, and removes nothing until the user approves (`claude-desktop/cleanup-2026-11-20.txt`); what it draws is not recorded, so the list is drawn from Ersa's own parts below.

- **Clean up inactive sessions** is a setting as above: "List the folders and branches Ersa no longer needs, with those of sessions inactive this long. Nothing is removed until you choose Clean up." Its chip offers 30 days, 60 days and 90 days, Desktop's values, and opens on 30. It is not kept: a choice reads the list again.
- **The list** is beneath that row, `space-md` from it and its parts `space-md` apart, read each time the page opens. Its items are in groups, in the brief's order: "Inactive and kept sessions", "Leftover workspaces", "Unused branches", "Folders Ersa can't identify". A group's heading is in `ui-small` at the `heading` weight in `ink-muted`, `space-xs` above its items; a group with nothing in it is not drawn. Its height eases over `motion-settle` with `motion-ease-out` as what it holds arrives, and what arrives fades in over `motion-quick`; at once with reduced motion.
- **An item** is a checkbox whose label is the whole row: a control without an edge, padded `space-sm` with `radius-md` corners, pulled out by its padding so its words line up with the setting's above. Under the pointer it takes `material-hover`, pressed `material-pressed`, and with keyboard focus its edge turns `ink`. The box is Lucide's square at `size-icon-sm`, `ink-faint` unticked and `accent` ticked, as the Worktree checkbox's; unavailable, `ink-faint` and unchanged under the pointer. Beside it, `space-sm` apart:
  - its name in `ui` in `ink`: a session's folder by its sessions' titles, a branch by its name and a folder by its own name, those two in `mono`;
  - beneath, `space-xs` apart, in `ui-small` in `ink-muted` and `space-md` apart: its project, when it was last active in the sessions page's words, its branch in `mono` and its size once measured, in File Explorer's units; a folder named by its own name gives its path on the line beneath, in `mono`;
  - beneath, what removing it loses: "Nothing is lost." or, for a merged branch, "Its changes are already in the base branch." in `ink-muted`; else each loss led by the caution glyph in `ink`, as the delete question's: the changed files and commits that exist nowhere else, commits that can't be counted, a repository of its own, or, for a folder, that Ersa can't count what it holds or couldn't finish looking through it.
- **Checking.** While the list is read it says "Checking…" after the context ring's turning arc at `size-ring-sm`, and so does each item until it is checked; such an item's box is unavailable and unticked. The ring holds still with reduced motion.
- **Can't be read.** The list, or a project whose branches and folders Core couldn't look through, says so and why as a setting's problem does. An item that couldn't be checked says "Ersa can't count what's in it." and why, the same way, and can't be ticked: removing it needs what checking gives.
- **Empty.** "Nothing to clean up. Folders and branches Ersa no longer needs appear here." in `ui-small` in `ink-muted`.
- **The foot**, `space-md` beneath a `hairline` `line` rule: how many are chosen in `ui-small` in `ink-muted` at the leading side, and **Clean up…** at the trailing side, a plain button, unavailable while nothing is chosen.
- **When it ends**, above the list: "Removed 2 of 4 items." in `ui` in `ink` after a glyph, `positive` when everything went and neutral otherwise, then "Stopped before the rest." when it was stopped, and each item not removed as a problem naming it, with why. It stays until the next clean-up or a change of days. Focus goes back to Clean up…, or, while the list is read again and Clean up… is gone, to the chip just above it.
- **While Ersa can't reach Core**, and while the question is open, the chip, the boxes and Clean up… are unavailable and the list keeps what was read.

## The clean-up question

The delete question's dialog (`RAIL.md`, The delete question): centered over the window, which is inert and not dimmed, `size-dialog` wide on `material-tile` with a `line` edge and `radius-lg` corners, fading in over `motion-settle`. Focus starts on Cancel and stays within it.

- **The title** names what goes: "Clean up 3 folders and 2 branches?".
- **Beneath**, in `ink-muted`: "This frees 1.2 GB." when a folder goes, leaving out what wasn't measured, and "Sessions whose folders go keep their conversations and branches." when a session's folder goes, Desktop's promise.
- **What is lost**, each led by the caution glyph in `ink`: "You lose 3 changed files and 2 commits that exist nowhere else for good.", commits that can't be counted, how many of the folders hold a repository of their own, how many folders Ersa can't count, and how many it couldn't finish looking through, which the space freed leaves out.
- **The buttons**: Cancel and Clean up, which reads "Discard and clean up" when anything chosen loses work or is a folder Ersa can't identify. Its label is in `danger`, as Delete's is.
- **Removing.** The question stays, saying "Removed 1 of 3…", the count of those done. Cancel is unavailable and the confirming button becomes Stop, with focus on it; pressed, it reads "Stopping…" after the ring until the item under way ends, and keeps focus. Then the question goes at once.
