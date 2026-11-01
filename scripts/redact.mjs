// Redacts personal details from Claude Code recordings before they reach disk.
//
//   node scripts/redact.mjs <file>...   rewrite files in place (used to scrub old fixtures)
//
// Recordings are captured under a real login: the `initialize` response's
// `account` object carries an email and organization, and paths carry the
// Windows user name. Every recorder passes all text through `redact` first.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export const PLACEHOLDER_EMAIL = "user@example.com";
export const PLACEHOLDER_ORG = "Example Org";
export const PLACEHOLDER_HOME = "C:/Users/user";

// Any email address. The placeholder itself matches and is left as is.
export const EMAIL_RE = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g;
// A home directory in any spelling: `C:\Users\x`, `C:/Users/x`, JSON-escaped
// `C:\\Users\\x`, `/home/x`, `/Users/x`. The drive letter is optional so a
// bare `\Users\x` is caught too. Group 1 is the user name.
export const HOME_RE = /(?:[A-Za-z]:)?(?:\\+|\/)(?:Users|home)(?:\\+|\/)([^\\/"'`\s:*?<>|]+)/gi;

export function redact(text) {
  return text
    // Organization names often embed the email, so they go first.
    .replace(/("organization(?:Name)?"\s*:\s*)"(?:[^"\\]|\\.)*"/g, `$1"${PLACEHOLDER_ORG}"`)
    .replace(EMAIL_RE, PLACEHOLDER_EMAIL)
    .replace(HOME_RE, PLACEHOLDER_HOME);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  for (const file of process.argv.slice(2)) {
    writeFileSync(file, redact(readFileSync(file, "utf8")));
  }
}
