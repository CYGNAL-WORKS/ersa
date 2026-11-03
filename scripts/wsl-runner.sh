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

# systemd runs the runner services; Windows PATH entries stay out of jobs.
cat > /etc/wsl.conf <<'EOF'
[boot]
systemd=true

[interop]
appendWindowsPath=false
EOF

# What the jobs expect an ubuntu-latest image to have, Docker for the
# container job, and the Tauri libraries the shell job would otherwise install.
apt-get update -q
apt-get install -yq --no-install-recommends \
  build-essential pkg-config libssl-dev git curl wget ca-certificates gnupg \
  unzip zip xz-utils zstd jq sudo tar gzip file docker.io \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev
systemctl enable --now docker

# The runners run as their own user, with passwordless sudo as on a hosted
# runner. The docker group is root-equivalent anyway.
id runner >/dev/null 2>&1 || { useradd --create-home --shell /bin/bash runner; passwd -l runner; }
usermod -aG docker runner
echo 'runner ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/runner
chmod 0440 /etc/sudoers.d/runner

# The container job writes into the work folder as root. Before each job,
# hand everything back to the runner so checkout can clean it.
cat > /home/runner/job-started.sh <<'EOF'
#!/bin/bash
[ "$(id -un)" = runner ] || exit 0
sudo -n chown -R runner:runner "$(dirname "$(dirname "$GITHUB_WORKSPACE")")"
EOF
chmod 0755 /home/runner/job-started.sh

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
  grep -q '^ACTIONS_RUNNER_HOOK_JOB_STARTED=' "$dir/.env" 2>/dev/null ||
    echo 'ACTIONS_RUNNER_HOOK_JOB_STARTED=/home/runner/job-started.sh' >> "$dir/.env"
  chown runner:runner "$dir/.env"
  unit=actions.runner.${REPO/\//-}.$name.service
  [ -f "/etc/systemd/system/$unit" ] || (cd "$dir" && ./svc.sh install runner >/dev/null)
  systemctl restart "$unit"
  echo "$name: $(systemctl is-active "$unit")"
done
