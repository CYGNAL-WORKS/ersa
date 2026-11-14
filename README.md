# Ersa

A desktop harness for running many agentic coding sessions at once, each in its own isolated workspace, on your machine or a remote one, by driving the vendor CLIs you already have (Claude Code, GitHub Copilot) instead of calling model APIs.

## Where things are

- `CLAUDE.md`: how we work. Read first.
- `docs/briefs/`: the source of truth for what Ersa does. Start with `PRD-ERSA.md` and `ARCH-CORE.md`.
- `docs/adr/`: why we decided what we decided.
- `docs/GLOSSARY.md`: the words we use, one per concept.
- `docs/design/`: look, feel, layout, and component contracts.
- `docs/MANIFEST.md`: generated index of briefs, their status, and requirement coverage.

Status: pre-alpha.

## Run the daily build

The daily build is Ersa built from the latest `main` and installed for your Windows account. It keeps its own sessions and data, apart from any development build you run beside it. ADR 0035 explains why.

To build it, you need Git, Rust, Node.js, and pnpm. From any checkout of this repository, run:

```bash
node scripts/daily-build.mjs --install
```

The script fetches `main`, builds it in a checkout of its own in `%LOCALAPPDATA%\ersa-daily-build`, and then runs the installer. Your own working trees don't change. Without `--install`, it prints the installer's path instead. The first build takes a while; later ones reuse its output.

The installer isn't signed, so Windows SmartScreen warns you the first time. Select **More info**, then **Run anyway**.

To update, quit the daily build, then run the script again. Your sessions and data in `%LOCALAPPDATA%\Ersa\daily` stay. To remove the daily build, uninstall Ersa from Windows settings.

## License

Ersa is licensed under the MIT license (`LICENSE`). The vendored third-party skills under `.claude/skills/` keep their own MIT license (`.claude/skills/THIRD-PARTY.md`).
