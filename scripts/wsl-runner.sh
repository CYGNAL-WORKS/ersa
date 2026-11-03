#!/bin/bash
# Sets up the Linux CI runners in a WSL Ubuntu 24.04 distro (docs/ci-runners.md).
# Run as root; safe to run again. With a registration token on stdin it also
# registers any runner not yet registered:
#   gh api -X POST repos/KPandaK/ersa/actions/runners/registration-token --jq .token |
#     wsl.exe -d Ubuntu-24.04 -u root --exec bash scripts/wsl-runner.sh
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

RUNNER_VERSION=2.338.2
RUNNER_SHA256=baebf204307bac6193480852732f0aba2ae97b6c5597c244f4d5558390147c40
RUNNERS=2
REPO=KPandaK/ersa

# Nothing runs in the background: no systemd, and the runners run in a
# terminal. Windows PATH entries stay out of jobs.
cat > /etc/wsl.conf <<'EOF'
[boot]
systemd=false

[interop]
appendWindowsPath=false
EOF

# What the jobs expect an ubuntu-latest image to have, and the Tauri libraries
# the shell job would otherwise install. The interface job installs Chromium
# and its libraries itself.
apt-get update -q
apt-get install -yq --no-install-recommends \
  build-essential pkg-config libssl-dev git curl wget ca-certificates gnupg \
  unzip zip xz-utils zstd jq sudo tar gzip file \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev

# The runners run as their own user, with passwordless sudo as on a hosted
# runner.
id runner >/dev/null 2>&1 || { useradd --create-home --shell /bin/bash runner; passwd -l runner; }
echo 'runner ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/runner
chmod 0440 /etc/sudoers.d/runner

tgz=/home/runner/actions-runner-linux-x64-$RUNNER_VERSION.tar.gz
[ -f "$tgz" ] || curl -fsSL -o "$tgz" \
  "https://github.com/actions/runner/releases/download/v$RUNNER_VERSION/actions-runner-linux-x64-$RUNNER_VERSION.tar.gz"
echo "$RUNNER_SHA256  $tgz" | sha256sum -c --quiet -
for n in $(seq "$RUNNERS"); do
  dir=/home/runner/runner-$n
  [ -f "$dir/config.sh" ] || { mkdir -p "$dir"; tar xzf "$tgz" -C "$dir"; }
done
/home/runner/runner-1/bin/installdependencies.sh >/dev/null
chown -R runner:runner /home/runner

token=
[ -t 0 ] || { read -r token || true; }
token=${token%$'\r'}
for n in $(seq "$RUNNERS"); do
  dir=/home/runner/runner-$n
  name=$(hostname)-wsl-$n
  if [ ! -f "$dir/.runner" ]; then
    [ -n "$token" ] || { echo "$name is not registered; pipe a registration token in." >&2; exit 1; }
    (cd "$dir" && sudo -u runner ./config.sh --unattended --url "https://github.com/$REPO" \
      --token "$token" --name "$name" --labels self-hosted,Linux,X64 --work _work --replace >/dev/null)
  fi
  echo "$name: registered"
done

# The runners run in a terminal, as the Windows one does, which also keeps
# the distro from stopping. Ctrl+C or closing the terminal stops them.
cat > /home/runner/run.sh <<'EOF'
#!/bin/bash
cd /home/runner
# WSLg leaves /run/user/<uid> open to others, and Core refuses to listen in a
# runtime folder anyone else can enter. A private one, as on a hosted runner.
export XDG_RUNTIME_DIR=/tmp/runner-runtime
mkdir -p "$XDG_RUNTIME_DIR" && chmod 0700 "$XDG_RUNTIME_DIR"
trap 'trap - INT TERM HUP; kill 0' INT TERM HUP
for dir in runner-*/; do
  n=${dir#runner-}; n=${n%/}
  ("./$dir/run.sh" 2>&1 | sed -u "s/^/[$n] /") &
done
wait
EOF
chmod 0755 /home/runner/run.sh
chown runner:runner /home/runner/run.sh
