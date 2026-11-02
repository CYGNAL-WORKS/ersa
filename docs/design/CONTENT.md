# Ersa content

These rules cover every word Ersa shows and every document in this repository. They are taken from the [Microsoft Writing Style Guide](https://learn.microsoft.com/style-guide/welcome/), chosen rule by rule, and a machine checks each one that can be checked: Vale in CI for prose, and the string catalog's test for interface copy. ADR 0020 records why. Spelling is US English.

## Voice

Plain, calm and brief. Name the thing and what happens. Address the person as "you", and name the agent's backend ("Claude Code") rather than "the AI". Ersa never says "I" or "we" in the interface, never jokes, and never uses exclamation marks or emoji.

## Where the rules apply

| Text | Rules | Checked by |
|---|---|---|
| Interface copy (the string catalog) | All rules and all interface patterns | Vale and the catalog test |
| Product docs (`README.md`, `docs/design/`, user-facing docs) | All rules | Vale |
| Agent-facing docs (briefs, skills, `CLAUDE.md`, `docs/agents/`) | Spelling, terms and punctuation only; their structure is governed by the `writing-for-agents` skill | Vale |
| Accepted ADRs | None: they are records, never rewritten | Not checked |

## Rules

Each rule names the Vale rule that enforces it. `Microsoft.*` rules come from the Microsoft package; `Ersa.*` rules are ours.

**Words**

- Spell in US English: color, behavior, canceled, license. (`Vale.Spelling`, US dictionary)
- Use the glossary term, and only that term: a session, not a chat; the rail, not the sidebar. (`Ersa.Terms`, built from `docs/GLOSSARY.md`)
- Use "you". Avoid "we" and "I" in the interface. (`Microsoft.We`, `Microsoft.FirstPerson`)
- Use contractions: don't, can't, isn't, it's. (`Microsoft.Contractions`)
- Prefer the active voice and the present tense. (`Microsoft.Passive`, as a suggestion)
- Keep sentences short: under 25 words in the interface and under 30 in docs. (`Microsoft.SentenceLength`, `Ersa.InterfaceLength`)
- In instructions, say "select", not "click" or "tap". (`Microsoft.UIVerbs`)
- Cut filler and hedging adverbs. (`Microsoft.Wordiness`, `Microsoft.Adverbs`)
- Use bias-free words, and use "stop" rather than "kill" or "abort". (`Microsoft.BiasFree`, `Microsoft.GenderBias`, `Microsoft.Militaristic`)
- Write "for example" and "that is", not "e.g." and "i.e.". (`Microsoft.Foreign`)

**Capitals and punctuation**

- Use sentence case for headings, titles, labels and buttons: "Add project", not "Add Project". (`Microsoft.Headings`, the catalog test)
- Use the serial comma: "host, project, and workspace". (`Microsoft.OxfordComma`)
- Write em dashes without spaces, and prefer a colon or parentheses. Use an en dash for ranges. (`Microsoft.Dashes`)
- Put a period or comma inside closing quotation marks. (`Microsoft.Quotes`)
- Use the ellipsis character (…) only for something in progress or a command that asks for more before it acts. Never type three periods. (`Microsoft.Ellipses`, the catalog test)
- Write numbers as numerals, and units with them: 4 sessions, 12 MB. (`Microsoft.Units`, `Microsoft.Ordinal`)
- Name keys as the keyboard shows them, joined with a plus sign: Ctrl+Enter, Shift+Enter.

**Not adopted from Microsoft.** `Microsoft.Avoid` and `Microsoft.Terms` are left out: they ban "backend" and rename "agent", which are Ersa's own terms. `Microsoft.Vocab` and the Microsoft product names are replaced by our glossary.

## Interface patterns

- **Buttons and commands.** Lead with a verb, use sentence case, and end without punctuation. Name the action: "Quit", "Allow", "Add project". Never use "OK" or "Yes" for a choice with consequences.
- **Errors.** Say what failed, then what to do, as sentences ending with a period: "Core isn't connected. Ersa is trying again." Don't blame the person, and don't prefix "Error:".
- **Empty states.** Say what will appear here and how to make it appear: "Sessions appear once Core is connected."
- **Progress.** Use a present participle with an ellipsis: "Saving…", "Reconnecting to Core…".
- **Confirmations.** The question names the consequence, and the primary button repeats its verb: "Quit Ersa? 2 sessions are running and will stop." with Quit and Cancel.
- **Status.** One or two words in sentence case: "Running", "Needs you".
- **Placeholders.** Say what to write, and never let a placeholder be a field's only label.

## The string catalog

Every string the Interface shows lives in the string catalog, keyed by area and purpose. Components refer to keys and never hold copy. A string with values names them as placeholders (`{count}`) and gives its plural forms. US English is the source locale; other locales are added beside it.

The catalog's test fails on copy written into a component, and on a string that starts lowercase, ends a button with punctuation, uses `!` or `...`, or is an error without a closing period. Vale reads the catalog for everything else.

## Checks

Vale runs in CI over the catalog and the docs in scope, with the Microsoft package pinned to one release and the `Ersa` style beside it. An error fails the build; a warning is reported; a suggestion shows only in the editor. A rule that misfires is fixed in the `Ersa` style or turned down, never silenced line by line.
