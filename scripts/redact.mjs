// Redacts personal details from Claude Code recordings before they reach disk.
//
//   node scripts/redact.mjs <file>...   rewrite files in place (used to scrub old fixtures)
//
// Recordings are captured under a real login: the `initialize` response's
// `account` object carries an email and organization, paths carry the Windows
// user name, and the working directory, the scratchpad and the command line
// carry the repository's own location on the owner's disk. Every recorder
// passes all text through `redact` first.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const PLACEHOLDER_EMAIL = "user@example.com";
export const PLACEHOLDER_ORG = "Example Org";
export const PLACEHOLDER_HOME = "C:/Users/user";
export const PLACEHOLDER_UUID = "00000000-0000-0000-0000-000000000000";
// The repository, as a path and as Claude Code munges one (every character
// other than an ASCII letter or digit becomes `-`).
export const PLACEHOLDER_REPO = "C:/Users/user/repo";
export const PLACEHOLDER_MUNGED_REPO = "C--Users-user-repo";

// Any email address. The placeholder itself matches and is left as is.
export const EMAIL_RE = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g;
// A home directory in any spelling: `C:\Users\x`, `C:/Users/x`, JSON-escaped
// `C:\\Users\\x`, `/home/x`, `/Users/x`. The drive letter is optional so a
// bare `\Users\x` is caught too. The last group is the user name; the shared
// `Public` profile is not personal and is left alone.
export const HOME_RE = /(?:[A-Za-z]:)?(?:\\+|\/)(Users|home)(?:\\+|\/)(?!Public\b)([^\\/"'`\s:*?<>|]+)/gi;
// Claude Code also names project directories after the path with every
// separator turned into a dash: `C--Users-<name>-AppData-...`.
export const MUNGED_HOME_RE = /\b([A-Za-z])--(Users|home)-(?!Public-)([^-\\/"'`\s]+)/gi;

// The repository on this machine: the main checkout, which a worktree of it
// sits under, with forward slashes. Recordings are made from one or the other.
const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const common = spawnSync("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], { cwd: ROOT, encoding: "utf8" }).stdout?.trim();
export const REPO = (common ? dirname(common) : ROOT).replace(/\\/g, "/").replace(/\/+$/, "");

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
// The repository path in any spelling of its separators (`\`, `/`, JSON-escaped
// `\\`), whole: `<repo>2` is another folder.
export const REPO_RE = new RegExp(REPO.split("/").map(escape).join("[\\\\/]+") + "(?![^\\\\/\"'`\\s])", "gi");
export const MUNGED_REPO_RE = new RegExp("\\b" + escape(REPO.replace(/[^A-Za-z0-9]/g, "-")) + "(?![A-Za-z0-9])", "gi");

export function redact(text) {
  return text
    // Organization names often embed the email, so they go first.
    .replace(/("organization(?:Name)?"\s*:\s*)"(?:[^"\\]|\\.)*"/g, `$1"${PLACEHOLDER_ORG}"`)
    // The account's organization id, which session records carry.
    .replace(/("organizationUuid"\s*:\s*)"[^"]*"/g, `$1"${PLACEHOLDER_UUID}"`)
    .replace(EMAIL_RE, PLACEHOLDER_EMAIL)
    // The repository before the home rules, which would otherwise rewrite a
    // repository under the home directory first.
    .replace(REPO_RE, PLACEHOLDER_REPO)
    .replace(MUNGED_REPO_RE, PLACEHOLDER_MUNGED_REPO)
    // Windows homes become the placeholder; Unix ones keep their shape.
    .replace(HOME_RE, (_m, dir) => (dir === "home" ? "/home/user" : PLACEHOLDER_HOME))
    .replace(MUNGED_HOME_RE, (_m, drive, dir) => `${drive}--${dir}-user`);
}

// The other direction, for a committed file a recorder hands to the CLI: an
// `--mcp-config` fixture names the repository by its placeholder, and the
// recorder gives the CLI an expanded copy in the scratch directory instead.
export function expandRepo(text) {
  return text.split(PLACEHOLDER_REPO).join(REPO);
}

export function expandMcpConfig(extra, cwd) {
  const i = extra.indexOf("--mcp-config");
  if (i === -1 || !extra[i + 1]) return;
  const expanded = join(cwd, "mcp-config.json");
  writeFileSync(expanded, expandRepo(readFileSync(extra[i + 1], "utf8")));
  extra[i + 1] = expanded;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  for (const file of process.argv.slice(2)) {
    writeFileSync(file, redact(readFileSync(file, "utf8")));
  }
}
