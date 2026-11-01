#!/usr/bin/env bash
# Runs the gallery's screenshot tests in the same Playwright container CI uses,
# so baselines rendered on any machine match CI's pixels.
#
#   scripts/screenshots-in-container.sh            check against the baselines
#   scripts/screenshots-in-container.sh --update   rewrite the baselines
#
# Needs Docker. The repository is mounted read-only and copied inside the
# container (the host's node_modules are for the host's OS); only the
# baselines are copied back, and only with --update.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$here/../.." && pwd)"
# Keep in step with @playwright/test in package.json and the CI job's container.
image="mcr.microsoft.com/playwright:v1.64.1-noble@sha256:47b510dd9230bf0b9bfc9b20102241e577ea7a99334ab4c3ca19f19770e9b8ea"
update="${1:-}"

docker run --rm \
  -v "$repo:/src:ro" \
  -v "$here/e2e:/out" \
  -e UPDATE="$update" \
  "$image" bash -euo pipefail -c '
    mkdir /work && cd /src
    tar --exclude=node_modules --exclude=target --exclude=.git -cf - \
      pnpm-workspace.yaml pnpm-lock.yaml protocol docs/design apps/shell | tar -xf - -C /work
    cd /work
    npm install --global corepack@0.36.0 >/dev/null && corepack enable pnpm
    cd apps/shell && pnpm install --frozen-lockfile >/dev/null && cd ../..
    cd apps/shell
    if [ "$UPDATE" = "--update" ]; then
      pnpm exec playwright test --update-snapshots
      rm -rf /out/__screenshots__ && cp -r e2e/__screenshots__ /out/
    else
      pnpm exec playwright test
    fi
  '
