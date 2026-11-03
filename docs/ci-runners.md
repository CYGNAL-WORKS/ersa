# CI runners

Every CI job runs on the owner's Windows machine, registered with `KPandaK/ersa` as self-hosted runners. Jobs wait while the machine is offline. Self-hosting is only for while the repository is private: a public repository would run a fork's pull request on this machine.

| Runners | Labels | Where |
|---|---|---|
| `DESKTOP-2GEVM7K` | `self-hosted, Windows, X64` | Windows itself |
| `DESKTOP-2GEVM7K-wsl-1`, `DESKTOP-2GEVM7K-wsl-2` | `self-hosted, Linux, X64` | The WSL distro `Ubuntu-24.04`, as the `runner` user |

The repository's runner list is under **Settings > Actions > Runners**, or `gh api repos/KPandaK/ersa/actions/runners`.

## The Linux runners

`scripts/wsl-runner.sh` sets up the distro: the packages the jobs expect, Docker Engine for the container job, the `runner` user with passwordless sudo, 2 runners in `/home/runner/runner-1` and `runner-2`, and `/home/runner/run.sh`, which runs them both. Before each job a hook hands the work folder back to `runner`, since the container job writes into it as root. The script is safe to run again, and it is the record of what the distro holds: change the distro by changing the script.

To set up the distro from nothing, from the repository root in PowerShell:

```powershell
wsl --install -d Ubuntu-24.04 --no-launch
gh api -X POST repos/KPandaK/ersa/actions/runners/registration-token --jq .token | wsl -d Ubuntu-24.04 -u root --exec bash scripts/wsl-runner.sh
wsl --terminate Ubuntu-24.04
```

The last line restarts the distro so its `/etc/wsl.conf` applies. Docker Desktop's WSL integration stays off for this distro, which has its own Docker Engine.

The runners run in a terminal, as the Windows runner does. Keep it open beside the Windows runner's:

```powershell
wsl -d Ubuntu-24.04 -u runner /home/runner/run.sh
```

Each line is prefixed with its runner's number. Ctrl+C or closing the terminal stops both, and WSL then stops the distro a few seconds later.

## Keeping them running

- **Runners offline.** Open the runner terminal above. If GitHub says "A session for this runner already exists", the distro stopped while the runners were connected: they retry and connect within a few minutes.
- **Restart the runners.** Press Ctrl+C in their terminal and run the command again.
- **Re-register.** A runner that GitHub removed, for example after 14 days offline, needs registering again. Delete its `.runner` file, then rerun the setup line with the token:

  ```powershell
  wsl -d Ubuntu-24.04 -u root rm /home/runner/runner-1/.runner
  gh api -X POST repos/KPandaK/ersa/actions/runners/registration-token --jq .token | wsl -d Ubuntu-24.04 -u root --exec bash scripts/wsl-runner.sh
  ```

- **Update the runner.** The runners update themselves. To set up a new distro at a later version, change `RUNNER_VERSION` and `RUNNER_SHA256` in the script to a release from [actions/runner](https://github.com/actions/runner/releases).
- **Disk.** Each runner keeps its tool cache and Rust toolchain in `/home/runner/runner-<n>/_work/_tool`, and Docker keeps the Playwright image. `docker system prune` and deleting `_tool` are safe; the next job puts back what it needs.
