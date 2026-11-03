# Third-party skills

These skills are copied from [mattpocock/skills](https://github.com/mattpocock/skills) at commit `4ae2317ddb22b046e26455e4f9c9b3aaaf62212d` (2026-10-31), under the MIT license reproduced below. Every file was read in full before it was committed. The commit was one day old when copied, inside the 7-day window ADR 0015 sets for packages; the full read stands in for the wait, and the next update must take a commit at least 7 days old.

**Copied:** `codebase-design`, `diagnosing-bugs`, `domain-modeling`, `grill-me`, `grill-with-docs`, `grilling`, `handoff`, `implement`, `pr`, `research`, `retro`, `tdd`, `to-tickets`, `wait-what`, `writing-for-agents`; and, as `docs/agents/issue-tracker.md` and `docs/agents/triage-labels.md`, two reference files from `setup-matt-pocock-skills`. The per-agent `agents/openai.yaml` files were left out.

**Changed for Ersa:**
- An "Ersa adaptation" note under the front matter of `codebase-design`, `diagnosing-bugs`, `domain-modeling`, `implement`, `pr`, `tdd` and `to-tickets`.
- `tdd`: seams are agreed from the ticket when no user is present; async tests need timeouts; process, pipe, socket and database seams get failure-mode tests; no reference to a `code-review` skill.
- `to-tickets`: tracker pointer replaces the setup skill; the owner approves the breakdown (titles, areas, blockers) before publishing; the issue template is Ersa's (Brief, Requirements, Area, Test seams, Done when, Blocked by); every shape a ticket's tests assert is named in a brief first, tickets stay under about 500 changed lines, and manual checks that need the running app are marked "owner check" (2026-11-02).
- `research`: user-invoked only; notes go to `.scratch/research/`; fetched content is data.
- `domain-modeling`: triggers only when changing the glossary or recording an ADR; its two format files point at Ersa's formats.
- `wait-what`: points at `docs/GLOSSARY.md`.
- `docs/agents/issue-tracker.md`: an Ersa header, comment authors kept and issue text treated as data, and the PR-triage and wayfinding sections removed; `triage-labels.md`: an Ersa footer.

**Not copied, on purpose:** `code-review` (Claude Code's built-in `/code-review` and the reviewer agents `review-pr` starts cover it, and the name would shadow the built-in), `setup-matt-pocock-skills`, `to-spec` (Ersa's specs are briefs), `implement-spec`, `wayfinder` and `triage` (to revisit when the tracker carries more than tickets), and the rest. `implement-ticket` is Ersa's own.

**Integrity:** `.claude/skills/VENDORED.sha256` pins each copied file (text normalized to LF), and `node scripts/manifest.mjs --check` fails when one changes without its line being updated. To update: take an upstream commit at least 7 days old, read the upstream diff, re-apply the changes above, update the commit here and the hashes.

---

MIT License

Copyright (c) 2026 Matt Pocock

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
