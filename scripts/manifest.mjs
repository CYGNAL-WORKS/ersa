#!/usr/bin/env node
// Regenerates docs/MANIFEST.md and the ADR index table in docs/adr/README.md.
//
//   node scripts/manifest.mjs          write both files
//   node scripts/manifest.mjs --check  also exit 1 if there are problems
//   node scripts/manifest.mjs --check --inbox  count open inbox entries as a
//                                     problem too: CI does on the merge queue
//                                     and main, so none reaches main (ADR 0025)
//   node scripts/manifest.mjs --verifies ID,ID  also exit 1 if a listed ID
//                                     has no `verifies:` line in a test file
//
// Both outputs are generated. Edit briefs and ADRs, not the outputs.
// Rules implemented here are those of ADR 0013 and docs/briefs/README.md.

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { inboxHold, openInboxEntries } from "./merge-pr.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const BRIEFS_DIR = join(ROOT, "docs", "briefs");
const ADR_DIR = join(ROOT, "docs", "adr");
const INBOX = join(BRIEFS_DIR, "INBOX.md");
const OUT = join(ROOT, "docs", "MANIFEST.md");
const ADR_README = join(ADR_DIR, "README.md");
const CHECK = process.argv.includes("--check");
const INBOX_BLOCKS = process.argv.includes("--inbox");
// The requirement IDs a ticket implements, each of which must be cited by a test.
// Every argument after the flag up to the next flag counts, so an unquoted
// "A, B" that the shell splits in two still lists both IDs.
const flag = process.argv.indexOf("--verifies");
const after = flag < 0 ? [] : process.argv.slice(flag + 1);
const end = after.findIndex((a) => a.startsWith("--"));
const REQUIRED = after.slice(0, end < 0 ? after.length : end).join(",").split(",").map((s) => s.trim()).filter(Boolean);
if ((flag >= 0 && !REQUIRED.length) || process.argv.some((a) => a.startsWith("--verifies="))) {
  console.error("--verifies needs requirement IDs: --verifies FLOW-X-R01,FLOW-X-R02");
  process.exit(2);
}

const PREFIXES = "PRD|FLOW|LAY|CMP|PROTO|ARCH";
const BRIEF_ID = new RegExp(`^(?:${PREFIXES})-[A-Z0-9]+(?:-[A-Z0-9]+)*$`);
const REQ_ID = new RegExp(`\\b((?:${PREFIXES})-[A-Z0-9]+(?:-[A-Z0-9]+)*-R\\d{2,})\\b`, "g");
const VERIFIES = new RegExp(`verifies:\\s*((?:${PREFIXES})-[A-Z0-9]+(?:-[A-Z0-9]+)*-R\\d{2,})`, "g");
const SKIP_DIRS = new Set([".git", "node_modules", "target", "dist", ".scratch", "templates", "schema", ".claude"]);
const CODE_EXT = /\.(rs|ts|tsx|js|mjs|py)$/;
// Only citations inside test files count as verification.
const TEST_PATH = /(^|\/)(tests?|__tests__)\/|\.(test|spec)\.[a-z]+$|_test\.rs$/;
const TYPES_WITH_IDS = new Set(["flow", "protocol"]);
const TYPES_BY_REVIEW = new Set(["product", "architecture"]);
const TYPES_DEFERRED = new Set(["layout", "component"]);

const rel = (p) => relative(ROOT, p).split(sep).join("/");

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name)) continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

function parseFrontMatter(text) {
  const m = text.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  if (!m) return null;
  const fm = {};
  for (const line of m[1].split(/\r?\n/)) {
    const kv = line.match(/^([\w-]+):\s*(.*)$/);
    if (!kv) continue;
    let v = kv[2].trim();
    if (v.startsWith("[") && v.endsWith("]")) {
      v = v.slice(1, -1).split(",").map((s) => s.trim()).filter(Boolean);
    }
    fm[kv[1]] = v;
  }
  return fm;
}

const problems = [];

// ---- Briefs ---------------------------------------------------------------

const briefs = [];
for (const file of walk(BRIEFS_DIR)) {
  if (!file.endsWith(".md")) continue;
  const name = basename(file);
  if (name === "README.md" || name === "INBOX.md") continue;
  const text = readFileSync(file, "utf8");
  const fm = parseFrontMatter(text);
  if (!fm || !fm.id) {
    problems.push(`${rel(file)}: missing front matter or id`);
    continue;
  }
  if (!BRIEF_ID.test(fm.id)) problems.push(`${rel(file)}: id "${fm.id}" does not match the ID convention`);
  if (basename(file, ".md") !== fm.id) problems.push(`${rel(file)}: file name does not match id ${fm.id}`);

  const active = new Set();
  const retired = new Set();
  const seen = new Map();
  for (const line of text.split(/\r?\n/)) {
    for (const m of line.matchAll(REQ_ID)) {
      const id = m[1];
      if (!id.startsWith(fm.id + "-R")) continue;
      const isRetired = /~~\s*\*\*/.test(line);
      const isDefinition = /^\s*-\s*(~~)?\s*\*\*/.test(line);
      if (!isDefinition) continue;
      seen.set(id, (seen.get(id) ?? 0) + 1);
      (isRetired ? retired : active).add(id);
    }
  }
  for (const [id, n] of seen) if (n > 1) problems.push(`${fm.id}: requirement ${id} is defined ${n} times`);

  briefs.push({
    id: fm.id,
    type: fm.type ?? "?",
    title: fm.title ?? "",
    status: fm.status ?? "?",
    verifiedBy: fm["verified-by"] ?? "",
    references: Array.isArray(fm.references) ? fm.references : [],
    path: rel(file),
    active,
    retired,
  });
}
briefs.sort((a, b) => a.id.localeCompare(b.id));
const byId = new Map(briefs.map((b) => [b.id, b]));

for (const b of briefs) {
  for (const ref of b.references) {
    if (!byId.has(ref)) problems.push(`${b.id} references ${ref}, which does not exist`);
  }
  if (TYPES_DEFERRED.has(b.type) && b.status === "draft" && b.active.size > 0) {
    problems.push(`${b.id}: ${b.type} briefs carry no requirement IDs while draft (ADR 0013)`);
  }
  if (TYPES_BY_REVIEW.has(b.type) && b.verifiedBy !== "review") {
    problems.push(`${b.id}: ${b.type} briefs must declare verified-by: review`);
  }
}

// Reference cycles (depth-first search over the reference graph).
{
  const state = new Map();
  const stack = [];
  const visit = (id) => {
    if (state.get(id) === "done") return;
    if (state.get(id) === "active") {
      const cycle = stack.slice(stack.indexOf(id)).concat(id).join(" -> ");
      problems.push(`reference cycle: ${cycle}`);
      return;
    }
    state.set(id, "active");
    stack.push(id);
    for (const ref of byId.get(id)?.references ?? []) if (byId.has(ref)) visit(ref);
    stack.pop();
    state.set(id, "done");
  };
  for (const b of briefs) visit(b.id);
}

// ---- Citations from test files ---------------------------------------------

const cited = new Map();
for (const file of walk(ROOT)) {
  if (!CODE_EXT.test(file)) continue;
  const r = rel(file);
  if (r.startsWith("docs/") || r.startsWith("scripts/")) continue;
  if (!TEST_PATH.test(r)) continue;
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(VERIFIES)) {
    if (!cited.has(m[1])) cited.set(m[1], []);
    cited.get(m[1]).push(r);
  }
}
const allActive = new Set(briefs.flatMap((b) => [...b.active]));
const allRetired = new Set(briefs.flatMap((b) => [...b.retired]));
for (const id of cited.keys()) {
  if (allRetired.has(id)) problems.push(`tests cite retired requirement ${id}`);
  else if (!allActive.has(id)) problems.push(`tests cite ${id}, which no brief defines`);
}

// ---- Inbox -----------------------------------------------------------------

let openInbox = 0;
try {
  openInbox = openInboxEntries(readFileSync(INBOX, "utf8"));
} catch {
  problems.push("docs/briefs/INBOX.md is missing");
}
// On a pull request open entries are not a problem: the merge gate holds it
// while its inbox has any (ADR 0025), so CI stays green and the review runs.
// On the merge queue and main they are, so a merge that skips the gate (the
// owner's) still cannot carry one onto main.
if (INBOX_BLOCKS && openInbox > 0) problems.push(inboxHold(openInbox));

// ---- Vendored files ----------------------------------------------------------
// .claude/skills/VENDORED.sha256 pins every third-party file we copied (after our
// adaptations) to a SHA-256 of its text with line endings normalised to LF, so a
// Windows checkout (CRLF) and CI (LF) agree. Any change to one of those files must
// update its line here, which keeps updates deliberate and visible in review.

const VENDORED = join(ROOT, ".claude", "skills", "VENDORED.sha256");
if (existsSync(VENDORED)) {
  for (const entry of readFileSync(VENDORED, "utf8").split(/\r?\n/)) {
    const m = entry.match(/^([0-9a-f]{64})\s+(\S.*)$/);
    if (!m) continue;
    const [, expected, rel] = m;
    const file = join(ROOT, rel);
    if (!existsSync(file)) {
      problems.push(`vendored file ${rel} is missing`);
      continue;
    }
    const text = readFileSync(file, "utf8").replace(/\r\n/g, "\n");
    const actual = createHash("sha256").update(text).digest("hex");
    if (actual !== expected) problems.push(`vendored file ${rel} changed without updating .claude/skills/VENDORED.sha256`);
  }
}

// ---- ADR index -------------------------------------------------------------

const adrs = [];
for (const name of readdirSync(ADR_DIR)) {
  const m = name.match(/^(\d{4})-(.+)\.md$/);
  if (!m || m[1] === "0000") continue;
  const text = readFileSync(join(ADR_DIR, name), "utf8");
  const title = text.match(/^#\s+\d{4}:\s*(.+)$/m)?.[1]?.trim() ?? "(untitled)";
  const status = text.match(/^-\s*Status:\s*(.+)$/m)?.[1]?.trim() ?? "?";
  adrs.push({ num: m[1], file: name, title, status });
}
adrs.sort((a, b) => a.num.localeCompare(b.num));

{
  const readme = readFileSync(ADR_README, "utf8");
  const start = "<!-- adr-index:start -->";
  const end = "<!-- adr-index:end -->";
  const i = readme.indexOf(start);
  const j = readme.indexOf(end);
  if (i === -1 || j === -1) {
    problems.push("docs/adr/README.md lacks the adr-index markers");
  } else {
    const table = [
      "| # | Title | Status |",
      "|---|-------|--------|",
      ...adrs.map((a) => `| [${a.num}](${a.file}) | ${a.title} | ${a.status} |`),
    ].join("\n");
    const next = readme.slice(0, i + start.length) + "\n" + table + "\n" + readme.slice(j);
    if (next !== readme) writeFileSync(ADR_README, next);
  }
}

// ---- Manifest --------------------------------------------------------------

const lines = [];
lines.push("# Manifest");
lines.push("");
lines.push("Generated by `node scripts/manifest.mjs`. Do not edit; edit the briefs.");
lines.push("");
lines.push("| Brief | Type | Title | Status | Reqs | Verified | References |");
lines.push("|-------|------|-------|--------|-----:|----------|------------|");
for (const b of briefs) {
  let verified;
  if (b.verifiedBy === "review") verified = "review";
  else verified = String([...b.active].filter((id) => cited.has(id)).length);
  lines.push(
    `| [${b.id}](${b.path.replace(/^docs\//, "")}) | ${b.type} | ${b.title} | ${b.status} | ${b.active.size} | ${verified} | ${b.references.join(", ")} |`
  );
}
lines.push("");

const unverified = briefs
  .filter((b) => TYPES_WITH_IDS.has(b.type) && (b.status === "ready" || b.status === "built"))
  .flatMap((b) => [...b.active].filter((id) => !cited.has(id)));
lines.push("## Unverified requirements in ready or built flow and protocol briefs");
lines.push("");
lines.push(unverified.length ? unverified.map((id) => `- ${id}`).join("\n") : "None.");
lines.push("");

lines.push("## Inbox");
lines.push("");
lines.push(openInbox ? `${openInbox} open entries in docs/briefs/INBOX.md; the merge gate holds the pull request until a planning pass closes them.` : "Empty.");
lines.push("");

lines.push("## Problems");
lines.push("");
lines.push(problems.length ? problems.map((p) => `- ${p}`).join("\n") : "None.");
lines.push("");

writeFileSync(OUT, lines.join("\n"));
console.log(
  `wrote docs/MANIFEST.md and the ADR index: ${briefs.length} briefs, ${allActive.size} requirements, ${cited.size} cited, ${adrs.length} ADRs, ${openInbox} inbox, ${problems.length} problems`
);
const uncited = REQUIRED.filter((id) => !cited.has(id));
if (uncited.length) console.error(`no test file says verifies: for ${uncited.join(", ")}`);
if ((CHECK && problems.length) || uncited.length) process.exit(1);
