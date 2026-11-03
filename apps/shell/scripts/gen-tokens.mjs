#!/usr/bin/env node
// Generates the Interface's CSS custom properties from the tokens in the
// front matter of docs/design/DESIGN.md, so every colour, size, radius and
// duration the interface uses has exactly one source.
//
//   node scripts/gen-tokens.mjs          write src/theme/tokens.gen.css
//   node scripts/gen-tokens.mjs --check  fail if that file is stale
//
// Names: `colors.light.ink` -> `--color-ink` (light and dark set the same
// names), `typography.ui.fontSize` -> `--type-ui-size`, `rounded.md` ->
// `--radius-md`, `spacing.lg` -> `--space-lg`, `line.hairline` ->
// `--line-hairline`, `size.rail` -> `--size-rail`, `motion.quick` ->
// `--motion-quick` (milliseconds), `motion.ease-out` -> `--motion-ease-out`,
// `material.tile` -> `--material-tile`, `glow.dark.focal` -> `--glow-focal`,
// `shadow.dark.transient` -> `--shadow-transient`, `stack.menu` -> `--stack-menu`
// (a bare stacking number).
//
// Pending tokens: src/theme/tokens.pending.yaml holds tokens an implementing
// session needs before a planning commit adds them to DESIGN.md (they are
// proposed in docs/briefs/INBOX.md). They are merged in, and a pending token
// DESIGN.md already names is refused, so the local copy goes once planning
// lands. Delete the file when it is empty.
//
// Color roles: in `material`, `glow` and `shadow`, `{colors.accent}` becomes
// `var(--color-accent)`, so a gradient or glow follows the theme and the
// palette. Those groups are declared again in every theme block: a custom
// property substitutes its var()s where it is declared, so one declared only
// on :root would keep the light colors inside a `data-theme="dark"` subtree.
//
// Themes: light by default; dark when the system asks for it, unless an
// element (or an ancestor) sets `data-theme="light"`. `data-theme="dark"` forces
// dark, which is how the gallery shows both themes side by side.

import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "yaml";

const SHELL = join(dirname(fileURLToPath(import.meta.url)), "..");
const DESIGN = join(SHELL, "..", "..", "docs", "design", "DESIGN.md");
const OUT = join(SHELL, "src", "theme", "tokens.gen.css");
const PENDING = join(SHELL, "src", "theme", "tokens.pending.yaml");
const CHECK = process.argv.includes("--check");

export function tokensFrom(markdown) {
  const match = markdown.replace(/\r\n/g, "\n").match(/^---\n([\s\S]*?)\n---\n/);
  if (!match) throw new Error("DESIGN.md has no front matter");
  return parse(match[1]);
}

// Adds the pending tokens to DESIGN.md's, group by group (and theme by theme).
export function withPending(design, pending, path = []) {
  const merged = { ...design };
  for (const [key, value] of Object.entries(pending ?? {})) {
    const here = [...path, key];
    if (!(key in merged)) merged[key] = value;
    else if (typeof value === "object" && typeof merged[key] === "object") merged[key] = withPending(merged[key], value, here);
    else throw new Error(`tokens.pending.yaml: ${here.join(".")} is in DESIGN.md now; delete it here`);
  }
  return merged;
}

// The tokens the interface builds from: DESIGN.md's, plus any still pending.
export function designTokens() {
  const design = tokensFrom(readFileSync(DESIGN, "utf8"));
  return existsSync(PENDING) ? withPending(design, parse(readFileSync(PENDING, "utf8"))) : design;
}

// Resolves `{typography.ui.fontFamily}` references against the whole token tree.
function resolve(value, root) {
  if (typeof value !== "string") return value;
  return value.replace(/\{([^}]+)\}/g, (_, path) => {
    const found = path.split(".").reduce((node, key) => node?.[key], root);
    if (found === undefined) throw new Error(`DESIGN.md: unresolved reference {${path}}`);
    return resolve(found, root);
  });
}

// `{colors.<role>}` names a role rather than a value: the theme's own variable.
function roles(value, tokens) {
  return value.replace(/\{colors\.([\w-]+)\}/g, (_, role) => {
    if (!(role in tokens.colors.light)) throw new Error(`DESIGN.md: no color role {colors.${role}}`);
    return `var(--color-${role})`;
  });
}

const kebab = (s) => s.replace(/([a-z])([A-Z])/g, "$1-$2").toLowerCase();

export function css(tokens) {
  const colorLines = (set) => Object.entries(set).map(([name, v]) => `  --color-${name}: ${v};`);
  const roleLines = (group, set) =>
    Object.entries(set).map(([name, v]) => `  --${group}-${name}: ${resolve(roles(v, tokens), tokens)};`);
  const themeLines = (theme) => [
    ...colorLines(tokens.colors[theme]),
    ...roleLines("material", tokens.material),
    ...roleLines("glow", tokens.glow[theme]),
    ...roleLines("shadow", tokens.shadow?.[theme] ?? {}),
  ];
  const shared = [];
  for (const [style, props] of Object.entries(tokens.typography)) {
    for (const [prop, v] of Object.entries(props)) {
      const short = { fontFamily: "family", fontSize: "size", lineHeight: "leading", fontWeight: "weight" }[prop] ?? kebab(prop);
      shared.push(`  --type-${style}-${short}: ${resolve(v, tokens)};`);
    }
  }
  for (const [name, v] of Object.entries(tokens.rounded)) shared.push(`  --radius-${name}: ${v};`);
  for (const [name, v] of Object.entries(tokens.spacing)) shared.push(`  --space-${name}: ${v};`);
  for (const [name, v] of Object.entries(tokens.line)) shared.push(`  --line-${name}: ${v};`);
  for (const [name, v] of Object.entries(tokens.size)) shared.push(`  --size-${name}: ${v};`);
  for (const [name, v] of Object.entries(tokens.stack ?? {})) shared.push(`  --stack-${name}: ${v};`);
  for (const [name, v] of Object.entries(tokens.motion)) {
    shared.push(`  --motion-${name}: ${typeof v === "number" ? `${v}ms` : v};`);
  }
  const dark = themeLines("dark").join("\n");
  return [
    "/* Generated by scripts/gen-tokens.mjs from docs/design/DESIGN.md. Do not edit; run `pnpm gen`. */",
    "",
    ":root {",
    ...shared,
    "}",
    "",
    ":root,\n[data-theme=\"light\"] {",
    "  color-scheme: light;",
    ...themeLines("light"),
    "}",
    "",
    "@media (prefers-color-scheme: dark) {",
    "  :root:not([data-theme=\"light\"]) {",
    "    color-scheme: dark;",
    dark.replace(/^/gm, "  "),
    "  }",
    "}",
    "",
    "[data-theme=\"dark\"] {",
    "  color-scheme: dark;",
    dark,
    "}",
    "",
  ].join("\n");
}

// Run only as a script, so the test can import the functions above.
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const text = css(designTokens());
  let current = null;
  try {
    current = readFileSync(OUT, "utf8").replace(/\r\n/g, "\n");
  } catch {}
  if (current !== text) {
    if (CHECK) {
      console.error("Stale src/theme/tokens.gen.css. Run `pnpm gen` in apps/shell and commit the result.");
      process.exit(1);
    }
    mkdirSync(dirname(OUT), { recursive: true });
    writeFileSync(OUT, text);
  }
}
