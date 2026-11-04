# 0023: The gallery keeps screenshot baselines for Windows and Linux, and CI checks both

- Status: accepted
- Date: 2026-11-04
- Related briefs: ARCH-CORE

## Context

The gallery's screenshot tests compare pixels exactly (ADR 0008). Chromium draws text differently on Windows and Linux, so a baseline matches only renders from the platform that made it. Baselines were Linux only: CI checks them on a WSL runner, and a session on the owner's Windows machine rewrote them in a pinned Playwright container, which starts Docker. That meant two Linux setups to keep identical by hand, and no screenshot check on the platform the owner develops on and Ersa ships for first (ADR 0006), where the Shell renders in WebView2, which is Chromium.

## Options considered

1. **Linux only, rendered by CI.** Retire the container and take new baselines from a failed CI run. One renderer and nothing to drift, but no local check, and a CI round trip for every visual change.
2. **Windows only.** A local check on the owner's platform, closest to what users see. Windows renders depend on the machine (Windows build, font smoothing, graphics driver), so the screenshot check can no longer be reproduced on a pinned Linux setup.
3. **Both platforms, each checked by CI.** Sessions check Windows locally and CI checks both, so neither set goes unchecked. Every visual change updates two sets of images.

## Decision

Option 3. Each shot has a Windows and a Linux baseline. A session runs the screenshot tests directly on Windows, with no container, and rewrites the Windows baselines there. CI checks the Windows baselines on the Windows runner and the Linux baselines on a Linux runner. New Linux baselines come from a failed CI run. The Playwright container is retired.

## Consequences

A session sees a visual regression before it pushes, on the platform users see first, and Docker is no longer part of development. Linux keeps a check on a pinned setup that the owner's machine settings can't change. A visual change commits twice the images and costs a CI round trip for its Linux half; a reviewer reads both sets. The Windows runner and the owner's sessions run on the same machine as the same account today, so their renders agree; isolating the runner under another account (issue #114) has to keep the settings that affect text rendering the same, or the Windows set stops matching local renders. Revisit if the two sets keep diverging for reasons other than the platform, or if keeping both costs more than the Linux check catches.
