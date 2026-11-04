# 0023: The gallery keeps screenshot baselines for Windows and Linux, and CI checks both

- Status: accepted
- Date: 2026-11-04
- Related briefs: ARCH-CORE

## Context

The gallery's screenshot tests (ADR 0008) compare pixels within only a small per-pixel threshold. Chromium draws text differently on Windows and Linux, so a baseline matches only renders from the platform that made it. Baselines were Linux only: CI checks them on a WSL runner, and a session on the owner's Windows machine rewrote them in a pinned Playwright container, which starts Docker. That meant two Linux setups to keep identical by hand, and no screenshot check on the platform the owner develops on and Ersa ships for first (ADR 0006), where the Interface renders in WebView2, which is Chromium.

## Options considered

1. **Linux only, rendered by CI.** Retire the container and take new baselines from a failed CI run. One renderer and nothing to drift, but no local check, and a CI round trip for every visual change.
2. **Windows only.** A local check on the owner's platform, closest to what users see. Windows renders depend on the machine (Windows build, font smoothing, graphics driver), so the only check would depend on settings the repository can't fix.
3. **Both platforms, each checked by CI.** Sessions check Windows locally and CI checks both, so neither set goes unchecked. Every visual change updates two sets of images.

## Decision

Option 3: each shot has a Windows and a Linux baseline; a session checks and rewrites the Windows set directly on Windows, CI checks each set on its own platform's runner, and new Linux baselines come from a failed CI run. The Playwright container is retired.

Accepted by the owner on 2026-11-04.

## Consequences

A session sees a visual regression before it pushes, on the platform users see first, and Docker is no longer part of development. Linux keeps a check that the owner's Windows settings can't change. Nothing pins the Linux runner's fonts and Chromium the way the container's image did, so a runner change that alters text fails every Linux shot at once, and it must be recognized as drift before its renders are committed. The Windows screenshot check runs on every change the Linux one runs on, not only on Rust changes like the rest of the Windows job. A visual change commits twice the images and costs a CI round trip for its Linux half; a reviewer reads both sets. The Windows runner and the owner's sessions run on the same machine as the same account today, so their renders agree; isolating the runner under another account (issue #114) has to keep the settings that affect text rendering the same, or the Windows set stops matching local renders. Until the ticket that carries this out lands, the Linux set and the container remain as they are. Revisit if the two sets keep diverging for reasons other than the platform, or if keeping both costs more than the Linux check catches.
