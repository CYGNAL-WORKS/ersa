// Redacts personal details from Claude Code recordings before they reach disk.
//
//   node scripts/redact.mjs <file>...   rewrite files in place (used to scrub old fixtures)
//
// Recordings are captured under a real login: the `initialize` response's
// `account` object carries an email, organization and the plan's name, every
// `rate_limit_event` carries how much of each usage window the login has
// used, a synced plugin's path carries a pair of account ids, paths carry the
// Windows user name, and the working directory, the scratchpad and the command line
// carry the repository's own location on the owner's disk. Every recorder
// passes all text through `redact` first. A sign-in recording also carries the
// one-time values of the sign-in address, which `redact` replaces, and must
// carry no token, key or code, which `secretsIn` finds so nothing is written.

import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir, userInfo } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const PLACEHOLDER_EMAIL = "user@example.com";
export const PLACEHOLDER_ORG = "Example Org";
export const PLACEHOLDER_HOME = "C:/Users/user";
export const PLACEHOLDER_UUID = "00000000-0000-0000-0000-000000000000";
export const PLACEHOLDER_PLAN = "Example Plan";
// A usage window's share, from 0 to 1: a number, as the recorded one is.
export const PLACEHOLDER_USAGE = 0.5;
// The repository, as a path and as Claude Code munges one (every character
// other than an ASCII letter or digit becomes `-`).
export const PLACEHOLDER_REPO = "C:/Users/user/repo";
export const PLACEHOLDER_MUNGED_REPO = "C--Users-user-repo";

// Any email address. The placeholder itself matches and is left as is.
export const EMAIL_RE = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g;
// The same, written into an address, where `@` is `%40`.
export const ENCODED_EMAIL_RE = /[A-Za-z0-9._+-]+%40[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g;
// The one-time values a sign-in address carries (a PKCE challenge, a state, a
// nonce) and the email it may be filled in with. Every other parameter keeps
// its value, so an authorization code stays for `secretsIn` to fail.
const SIGN_IN_PARAM_RE = /([?&#](?:state|code_challenge|nonce|login_hint|email)=)[^&#\s"'\\]+/gi;
export const PLACEHOLDER_PARAM = "REDACTED";
// A home directory in any spelling: `C:\Users\x`, `C:/Users/x`, JSON-escaped
// `C:\\Users\\x`, `/home/x`, `/Users/x`. The drive letter is optional so a
// bare `\Users\x` is caught too. The last group is the user name; the shared
// `Public` profile is not personal and is left alone.
export const HOME_RE = /(?:[A-Za-z]:)?(?:\\+|\/)(Users|home)(?:\\+|\/)(?!Public\b)([^\\/"'`\s:*?<>|]+)/gi;
// Claude Code also names project directories after the path with every
// separator turned into a dash: `C--Users-<name>-AppData-...`.
export const MUNGED_HOME_RE = /\b([A-Za-z])--(Users|home)-(?!Public-)([^-\\/"'`\s]+)/gi;

// The account details a recording carries as values, each as a JSON field or
// a path segment. A JSON-escaped spelling (`\"key\":`) is matched too. The
// first group is what stays; the second, where there is one, is the value.
const PLAN_RE = /(\\?"subscriptionType\\?"\s*:\s*\\?")([^"\\]*)(?=\\?")/g;
const USAGE_RE = /(\\?"utilization\\?"\s*:\s*)(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g;
// `plugins/synced/<id>_<id>/<plugin>`: the ids name the account the plugin
// was synced from.
const SYNCED_RE = /(synced(?:\\+|\/))([A-Za-z0-9-]+_[A-Za-z0-9-]+)/g;
const PLACEHOLDER_SYNCED = `${PLACEHOLDER_UUID}_${PLACEHOLDER_UUID}`;
const ACCOUNT_RULES = [
  { field: "subscriptionType", re: PLAN_RE, placeholder: PLACEHOLDER_PLAN },
  { field: "utilization", re: USAGE_RE, placeholder: String(PLACEHOLDER_USAGE) },
  { field: "synced plugin ids", re: SYNCED_RE, placeholder: PLACEHOLDER_SYNCED },
];

// Each account detail `text` still carries as recorded: its 1-based line and
// the field's name. Never the value, so a report of it leaks nothing.
export function accountDetails(text) {
  const found = [];
  text.split("\n").forEach((line, i) => {
    for (const { field, re, placeholder } of ACCOUNT_RULES) {
      for (const m of line.matchAll(re)) if (m[2] !== placeholder) found.push({ line: i + 1, field });
    }
  });
  return found;
}

// For a recorder, after it writes: names each file, line and field that still
// carries an account detail, and fails the run, so nothing reaches a commit
// unnoticed. The value itself is never printed.
export function checkWritten(files) {
  for (const file of files) {
    let text;
    try {
      text = readFileSync(file, "utf8");
    } catch {
      continue; // a file this run did not write
    }
    for (const { line, field } of accountDetails(text)) {
      console.error(`${file}:${line}: ${field} still carries the recorded value; redact it before committing`);
      process.exitCode = 1;
    }
  }
}

// The number a new recording takes in its version folder: one past the
// highest there, two digits at least. Counting the files instead would give a
// number already taken once a fixture is removed or the numbers have a gap.
export function nextFixtureNumber(dir) {
  const taken = readdirSync(dir).map((f) => /^(\d{2,})-.*\.jsonl$/.exec(f)).filter(Boolean).map((m) => Number(m[1]));
  return String(Math.max(-1, ...taken) + 1).padStart(2, "0");
}

// This checkout, and the repository on this machine: the main checkout, which
// a worktree of it sits under, both with forward slashes. Recordings are made
// from one or the other; a worktree elsewhere is caught by the guard test.
export const CHECKOUT = join(fileURLToPath(import.meta.url), "..", "..").replace(/\\/g, "/").replace(/\/+$/, "");
const common = spawnSync("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], { cwd: CHECKOUT, encoding: "utf8", timeout: 5000 }).stdout?.trim();
export const REPO = common ? dirname(common).replace(/\\/g, "/").replace(/\/+$/, "") : CHECKOUT;

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
// The repository path in any spelling of its separators (`\`, `/`, JSON-escaped
// `\\`), whole: `<repo>2` and `<repo>-x` are other folders, while `<repo>.`
// ends a sentence. A Unix path's munged form starts with `-`, where `\b`
// would never match, so the boundary is spelled out.
export function repoRules(repo) {
  return {
    plain: new RegExp(repo.split("/").map(escape).join("[\\\\/]+") + "(?![A-Za-z0-9_-])", "gi"),
    munged: new RegExp("(?<![A-Za-z0-9])" + escape(repo.replace(/[^A-Za-z0-9]/g, "-")) + "(?![A-Za-z0-9])", "gi"),
  };
}
export const { plain: REPO_RE, munged: MUNGED_REPO_RE } = repoRules(REPO);
// A worktree outside the main checkout's folder is a second path to redact.
const checkoutRules = CHECKOUT === REPO || CHECKOUT.startsWith(REPO + "/") ? [] : [repoRules(CHECKOUT)];

export function redact(text) {
  // The repository before the home rules, which would otherwise rewrite a
  // repository under the home directory first.
  for (const { plain, munged } of [{ plain: REPO_RE, munged: MUNGED_REPO_RE }, ...checkoutRules]) {
    text = text.replace(plain, PLACEHOLDER_REPO).replace(munged, PLACEHOLDER_MUNGED_REPO);
  }
  return text
    // Organization names often embed the email, so they go first.
    .replace(/("organization(?:Name)?"\s*:\s*)"(?:[^"\\]|\\.)*"/g, `$1"${PLACEHOLDER_ORG}"`)
    // The account's organization id, which session records carry.
    .replace(/("organizationUuid"\s*:\s*)"[^"]*"/g, `$1"${PLACEHOLDER_UUID}"`)
    // The plan's name, the usage figures and a synced plugin's ids, each to a
    // fixed placeholder of the same type.
    .replace(PLAN_RE, `$1${PLACEHOLDER_PLAN}`)
    .replace(USAGE_RE, `$1${PLACEHOLDER_USAGE}`)
    .replace(SYNCED_RE, `$1${PLACEHOLDER_SYNCED}`)
    // A sign-in address's one-time values, before the email rules, which
    // would otherwise turn a filled-in email into the placeholder address.
    .replace(SIGN_IN_PARAM_RE, `$1${PLACEHOLDER_PARAM}`)
    .replace(EMAIL_RE, PLACEHOLDER_EMAIL)
    .replace(ENCODED_EMAIL_RE, PLACEHOLDER_EMAIL)
    // Windows homes become the placeholder; Unix ones keep their shape.
    .replace(HOME_RE, (_m, dir) => (dir === "home" ? "/home/user" : PLACEHOLDER_HOME))
    .replace(MUNGED_HOME_RE, (_m, drive, dir) => `${drive}--${dir}-user`);
}

// The machine's user name, as a word in any case, becomes `user`: streamed
// tool input can split a path mid-name, past the home rules. A name under
// three letters would match too much and is left.
export function scrubUserName(text, name = userInfo().username) {
  if (name.length < 3) return text;
  return text.replace(new RegExp(`\\b${escape(name)}\\b`, "gi"), "user");
}

// What a token, a key or an authorization code looks like. A recording that
// carries one is not written at all, so these err toward a false alarm.
const SECRET_RULES = [
  { kind: "an Anthropic key or token", re: /sk-ant-[A-Za-z0-9_-]{6,}/g },
  { kind: "a JSON web token", re: /eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g },
  { kind: "a bearer token", re: /\bBearer\s+[A-Za-z0-9._~+/-]{8,}/gi },
  // `code=true` is a flag the sign-in address really carries; a code is longer.
  {
    kind: "a code or token in an address",
    re: /[?&#](?:code|(?:access|refresh|id)_token|token|api_?key|key|code_verifier|client_secret)=(?!(?:true|false|REDACTED)\b)[^&#\s"'\\]{6,}/gi,
  },
  {
    kind: "a token, key or code field",
    re: /\\?"(?:(?:access|refresh|id|session)_?token|token|api_?key|authorization_?code|code_?verifier|client_?secret|password|secret)\\?"\s*:\s*\\?"(?!\\?")/gi,
  },
];
// What a recording carries that looks random and is not a secret, cut before
// the random-looking rule runs: a thinking block's signature, an image's
// base64 (PNG, JPEG, GIF, WebP), and the API's message, tool and request ids.
const NOT_SECRET_RE = [
  /(?<=\\?"signature\\?"\s*:\s*\\?")[^"\\]+/g,
  /(?:iVBORw0KGgo|\/9j\/|R0lGOD|UklGR)[A-Za-z0-9+/=]*/g,
  /\b(?:msg|toolu|srvtoolu|req)_[A-Za-z0-9]+/g,
];
// A run of 32 or more URL-safe characters, mixing upper and lower case and
// digits, whose kind of character changes at three positions in ten or more,
// as random text does and words, paths and ids do not. A munged path
// (`C--Users-…`, `-home-…`) is a path.
function looksRandom(run) {
  if (/^[A-Za-z]--|^-[a-z]/.test(run)) return false;
  const kind = (c) => (/[A-Z]/.test(c) ? 1 : /[a-z]/.test(c) ? 2 : /[0-9]/.test(c) ? 3 : 4);
  const count = (re) => (run.match(re) ?? []).length;
  let changes = 0;
  for (let i = 1; i < run.length; i++) if (kind(run[i]) !== kind(run[i - 1])) changes++;
  return count(/[A-Z]/g) >= 2 && count(/[a-z]/g) >= 2 && count(/[0-9]/g) >= 1 && changes / run.length >= 0.3;
}

// Each line of `text` that carries something shaped like a token, a key or an
// authorization code, with its kind: 1-based line and kind, never the value,
// so a report of it leaks nothing.
export function secretsIn(text) {
  const found = [];
  text.split("\n").forEach((line, i) => {
    const at = (kind) => found.push({ line: i + 1, kind });
    for (const { kind, re } of SECRET_RULES) if (line.match(re)) at(kind);
    const rest = NOT_SECRET_RE.reduce((l, re) => l.replace(re, ""), line);
    if ((rest.match(/[A-Za-z0-9_-]{32,}/g) ?? []).some(looksRandom)) at("a random-looking string");
  });
  return found;
}

// The other direction, for a committed file a recorder hands to the CLI: an
// `--mcp-config` fixture names the repository by its placeholder, which
// expands to this checkout, so a worktree records against its own files.
export function expandRepo(text) {
  return text.split(PLACEHOLDER_REPO).join(CHECKOUT);
}

// The arguments for the CLI: `args` with its `--mcp-config` file replaced by
// an expanded copy in a scratch directory of its own (the CLI's working
// directory stays empty), or `args` itself. The recording keeps the
// arguments as given.
export function expandMcpConfig(args) {
  const i = args.indexOf("--mcp-config");
  if (i === -1 || !args[i + 1]) return args;
  const expanded = join(mkdtempSync(join(tmpdir(), "ersa-rec-mcp-")), "mcp-config.json");
  writeFileSync(expanded, expandRepo(readFileSync(args[i + 1], "utf8")));
  return [...args.slice(0, i + 1), expanded, ...args.slice(i + 2)];
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  for (const file of process.argv.slice(2)) {
    writeFileSync(file, redact(readFileSync(file, "utf8")));
  }
}
