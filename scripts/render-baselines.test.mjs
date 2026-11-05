// Tests for render-baselines.mjs, run without GitHub or git.
// Run: node --test scripts/render-baselines.test.mjs

import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { after, test } from "node:test";

import { BASELINES, renderBaselines } from "./render-baselines.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const HEAD = "a".repeat(40);
const roots = [];
after(() => roots.forEach((r) => rmSync(r, { recursive: true, force: true })));

function fakeRoot() {
  const root = mkdtempSync(join(tmpdir(), "render-baselines-test-"));
  roots.push(root);
  mkdirSync(join(root, BASELINES), { recursive: true });
  writeFileSync(join(root, BASELINES, "stale.png"), "old");
  return root;
}

// A fake git and gh. `over` replaces one command's reply, keyed by its first two words.
function fake(over = {}) {
  const calls = [];
  const replies = {
    "git rev-parse --abbrev-ref": { status: 0, stdout: "feature\n" },
    "git rev-parse HEAD": { status: 0, stdout: `${HEAD}\n` },
    "git ls-remote": { status: 0, stdout: `${HEAD}\trefs/heads/feature\n` },
    "git status": { status: 0, stdout: "" },
    "gh workflow": { status: 0, stdout: "" },
    // Before the dispatch, an older run on the same head; after it, the new one too.
    "gh run list": () => ({
      status: 0,
      stdout: JSON.stringify([
        ...(calls.some((c) => c.startsWith("gh workflow")) ? [{ databaseId: 7, headSha: HEAD, createdAt: "2026-11-05T12:00:05Z" }] : []),
        { databaseId: 6, headSha: HEAD, createdAt: "2026-11-05T11:59:50Z" },
      ]),
    }),
    "gh run watch": { status: 0, stdout: "" },
    "gh run download": (args) => {
      writeFileSync(join(args[args.indexOf("--dir") + 1], "gallery.png"), "new");
      return { status: 0, stdout: "" };
    },
    ...over,
  };
  const run = (cmd, args) => {
    calls.push([cmd, ...args].join(" "));
    const key = Object.keys(replies).find((k) => [cmd, ...args].join(" ").startsWith(k));
    const reply = replies[key];
    return { stderr: "", ...(typeof reply === "function" ? reply(args) : reply) };
  };
  return { run, calls };
}

const go = (f, root, extra = {}) => renderBaselines({ run: f.run, sleep: async () => {}, root, findTries: 3, log: () => {}, ...extra });

test("a pushed branch's run replaces the local baselines with its artifact", async () => {
  const root = fakeRoot();
  const f = fake();
  assert.equal(await go(f, root), 7);
  assert.equal(readFileSync(join(root, BASELINES, "gallery.png"), "utf8"), "new");
  assert.equal(existsSync(join(root, BASELINES, "stale.png")), false);
  assert.ok(f.calls.includes("gh workflow run screenshots.yml --ref feature"));
  assert.ok(f.calls.includes("gh run watch 7 --exit-status"));
});

test("an unpushed branch is refused before anything starts", async () => {
  const f = fake({ "git ls-remote": { status: 0, stdout: "" } });
  await assert.rejects(go(f, fakeRoot()), /feature is not pushed/);
  assert.ok(!f.calls.some((c) => c.startsWith("gh")));
});

test("a remote behind the local head is refused", async () => {
  const f = fake({ "git ls-remote": { status: 0, stdout: `${"b".repeat(40)}\trefs/heads/feature\n` } });
  await assert.rejects(go(f, fakeRoot()), /origin\/feature is at bbbbbbb, not the local head aaaaaaa; push or pull/);
  assert.ok(!f.calls.some((c) => c.startsWith("gh")));
});

test("uncommitted changes are refused before anything starts, except earlier baselines", async () => {
  const f = fake({ "git status": { status: 0, stdout: " M pnpm-lock.yaml\n" } });
  await assert.rejects(go(f, fakeRoot()), /The working tree has uncommitted changes/);
  assert.ok(!f.calls.some((c) => c.startsWith("gh")));
  const g = fake();
  await go(g, fakeRoot());
  assert.ok(g.calls.includes("git status --porcelain -- . :(exclude)apps/shell/e2e/__screenshots__"));
});

test("a detached head is refused", async () => {
  await assert.rejects(go(fake({ "git rev-parse --abbrev-ref": { status: 0, stdout: "HEAD\n" } }), fakeRoot()), /detached/);
});

test("a workflow GitHub cannot start says it must be on main", async () => {
  const f = fake({ "gh workflow": { status: 1, stdout: "", stderr: "could not find any workflows named screenshots.yml" } });
  await assert.rejects(go(f, fakeRoot()), /Starting screenshots\.yml \(it must be on main\) failed: could not find/);
});

test("a run for another head, or one that existed before the dispatch, is never taken", async () => {
  const runs = [
    { databaseId: 5, headSha: "c".repeat(40), createdAt: "2026-11-05T12:00:05Z" },
    { databaseId: 6, headSha: HEAD, createdAt: "2026-11-05T12:00:06Z" },
  ];
  const f = fake({ "gh run list": { status: 0, stdout: JSON.stringify(runs) } });
  await assert.rejects(go(f, fakeRoot()), /No screenshots\.yml run for aaaaaaa appeared/);
  assert.equal(f.calls.filter((c) => c.startsWith("gh run list")).length, 4);
});

test("a failed run leaves the baselines alone and names the run", async () => {
  const root = fakeRoot();
  await assert.rejects(go(fake({ "gh run watch": { status: 1, stdout: "" } }), root), /Run 7 failed; read it with `gh run view 7 --log-failed`/);
  assert.equal(readFileSync(join(root, BASELINES, "stale.png"), "utf8"), "old");
});

test("an empty artifact leaves the baselines alone", async () => {
  const root = fakeRoot();
  await assert.rejects(go(fake({ "gh run download": { status: 0, stdout: "" } }), root), /artifact is empty/);
  assert.equal(readFileSync(join(root, BASELINES, "stale.png"), "utf8"), "old");
});

const imagePins = (text) => [...text.matchAll(/mcr\.microsoft\.com\/playwright:[^\s"']+/g)].map((m) => m[0]);
const read = (f) => readFileSync(join(ROOT, f), "utf8");

test("the baselines workflow sets up pnpm and installs as the interface job does", () => {
  const ci = read(".github/workflows/ci.yml");
  const lines = read(".github/workflows/screenshots.yml").split("\n").map((l) => l.trim().replace(/^run: /, ""));
  for (const step of ["npm install --global corepack@", "corepack enable pnpm", "corepack install", "pnpm install --frozen-lockfile"]) {
    const line = lines.find((l) => l.startsWith(step));
    assert.ok(line, `screenshots.yml has no step starting ${step}`);
    assert.ok(ci.includes(line), `ci.yml's interface job differs from screenshots.yml at: ${line}`);
  }
});

test("imagePins finds each Playwright image reference", () => {
  assert.deepEqual(imagePins('a\n    container: mcr.microsoft.com/playwright:v1-noble@sha256:ab\nimage="mcr.microsoft.com/playwright:v2"'), [
    "mcr.microsoft.com/playwright:v1-noble@sha256:ab",
    "mcr.microsoft.com/playwright:v2",
  ]);
});

test("ci.yml, the baselines workflow and the Docker script pin one Playwright image", () => {
  const files = [".github/workflows/ci.yml", ".github/workflows/screenshots.yml", "apps/shell/scripts/screenshots-in-container.sh"];
  const pins = files.map((f) => imagePins(read(f)));
  pins.forEach((p, i) => assert.equal(p.length, 1, `${files[i]} names ${p.length} Playwright images, not one`));
  assert.equal(new Set(pins.flat()).size, 1, `the Playwright images differ: ${pins.flat().join(", ")}`);
});
