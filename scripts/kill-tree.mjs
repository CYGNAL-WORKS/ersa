// Kills a process and every process it started, also after it has exited.
// On Windows an orphan keeps its parent's id, so the tree is walked by parent
// id, taking only processes created since `since` (a Date) so a reused id
// never takes in someone else's process. Elsewhere it kills the process group
// of a child spawned detached. Used by the PR shepherd's scripts.

import { spawnSync } from "node:child_process";

const LIST =
  "Get-CimInstance Win32_Process | ForEach-Object { '{0} {1} {2}' -f $_.ProcessId, $_.ParentProcessId, $_.CreationDate.ToUniversalTime().ToString('o') }";

// The ids of `pid` and its descendants in `procs` ({ pid, ppid, created }).
export function descendants(pid, procs, since) {
  const tree = new Set([pid]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const p of procs)
      if (!tree.has(p.pid) && tree.has(p.ppid) && p.created >= since.getTime() - 1000) {
        tree.add(p.pid);
        grew = true;
      }
  }
  return [...tree];
}

export function killTree(pid, since) {
  if (!pid) return;
  if (process.platform !== "win32") {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {
      // Not a group leader, or the group is gone.
    }
    return;
  }
  const ps = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", LIST], { encoding: "utf8", timeout: 60000, windowsHide: true });
  const procs = (ps.stdout ?? "")
    .split("\n")
    .map((l) => l.trim().split(" "))
    .filter((p) => p.length === 3)
    .map(([a, b, c]) => ({ pid: Number(a), ppid: Number(b), created: Date.parse(c) }));
  // The root's id, once it exited, may name an older, unrelated process.
  const reused = procs.some((p) => p.pid === pid && p.created < since.getTime() - 1000);
  const ids = descendants(pid, procs, since).filter((id) => id !== process.pid && !(reused && id === pid));
  if (ids.length) spawnSync("taskkill", ["/F", ...ids.flatMap((id) => ["/PID", String(id)])], { stdio: "ignore", timeout: 60000, windowsHide: true });
}
