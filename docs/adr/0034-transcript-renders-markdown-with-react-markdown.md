# 0034: The transcript renders Markdown with react-markdown, never as HTML

- Status: accepted
- Date: 2026-11-13
- Related briefs: FLOW-READ-TRANSCRIPT, PRD-ERSA

## Context

M1 asks for the agent's words as formatted text (PRD-ERSA-R08). The agent writes Markdown, and the Interface has no renderer. What the agent writes is untrusted: it can quote a web page, a file or a tool's output, and the transcript runs in the webview that talks to Core, so Markdown turned into HTML and inserted into the page could run a script with the window's access. A renderer is a dependency, chosen under ADR 0015.

## Options considered

1. **react-markdown with remark-gfm**: parses Markdown to a syntax tree and builds React elements from it, never HTML text. It never passes raw HTML into the page unless a plugin is added, and link addresses are filtered by default. GitHub's extensions come from remark-gfm. Its cost is the unified family behind it, about 60 small packages from one maintainer group, all MIT.
2. **markdown-it** (or marked): fast, few dependencies, but produces an HTML string that the page must insert, so safety rests on its escaping and on a sanitizer Ersa adds and keeps configured.
3. **Write a parser**: no dependency, but Markdown's edge cases (nested lists, fences, tables, streaming partial text) are years of other people's work.

## Decision

Option 1, at versions that pass the release-age check: react-markdown for the renderer and remark-gfm for tables, strikethrough, task lists and autolinks. No plugin that passes raw HTML through is ever added. Raw HTML shows as the text it is; a Markdown image shows as its alternative text linked to its address and is never loaded, so the agent's text can't make the window fetch anything; a link opens in the system's default handler only for `http`, `https` and `mailto`. Code blocks have no syntax colors for now.

## Consequences

The transcript can't run what the agent writes, by construction rather than by a sanitizer's configuration. The Interface takes on the unified family's packages and their updates, which the vulnerability scan watches. Rendering a streaming block re-parses its text on each delta; blocks are memoized, so only the growing block re-renders, and a very long message may need the block split before it slows. Opening links needs the shell's opener, which the ticket that builds links adds. Revisit if syntax colors are wanted (their library is a separate choice under ADR 0015) or if re-parsing a streaming block proves slow.
