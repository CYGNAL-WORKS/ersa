// Holds the rules of docs/design/CONTENT.md, "The string catalog", as a check
// rather than a review, in the style of the design-token test: every catalog
// string against the rules for all strings and for its kind, and component
// source against copy written inline.

import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, sep } from "node:path";
import { parseSync } from "vite";
import { describe, expect, it } from "vitest";
// @ts-expect-error: a plain script without type declarations.
import { isCurrent, markdown } from "../../scripts/gen-catalog.mjs";
import { enUS } from "./en-US";
import allowed from "./inline-copy.allow.json";
import { isPlural, type Section } from "./lookup";

const SRC = join(import.meta.dirname, "..");

// --- Rules for catalog strings ---------------------------------------------

/** The kinds a key may end in ("The string catalog", Kinds). */
const KINDS = ["Button", "Error", "Progress", "Status", "Placeholder", "Empty", "Confirmation"] as const;
type Kind = (typeof KINDS)[number];
const kindOf = (key: string): Kind | undefined => KINDS.find((kind) => key.endsWith(kind));

const CATEGORIES: readonly string[] = ["zero", "one", "two", "few", "many", "other"] satisfies Intl.LDMLPluralRule[];
const CAMEL = /^[a-z][a-zA-Z0-9]*$/;

/** What is wrong with one string (a whole message, or one plural form) under its key. */
function problems(key: string, text: string): string[] {
  const found: string[] = [];
  // Every string ("The string catalog", last paragraph; "Capitals and punctuation").
  if (text.trim() === "") found.push("is empty");
  if (/^\p{Ll}/u.test(text)) found.push("starts lowercase");
  if (text.includes("!")) found.push("uses !");
  if (text.includes("...")) found.push("uses ... rather than …");
  // Values ("The string catalog", Values): braces only around a camelCase name.
  if (/[{}]/.test(text.replace(/\{[a-z][a-zA-Z0-9]*\}/g, ""))) found.push("has a brace that isn't a {value}");
  // By kind ("The string catalog", last paragraph; "Interface patterns").
  // Trailing space would hide the last character from these checks.
  const kind = kindOf(key);
  const end = text.trimEnd();
  if (kind === "Button" && /\p{P}$/u.test(end) && !end.endsWith("…")) found.push("is a Button ending in punctuation");
  if ((kind === "Error" || kind === "Empty") && !end.endsWith(".")) found.push(`is ${kind === "Error" ? "an Error" : "an Empty"} string without a closing period`);
  if (kind === "Progress" && !end.endsWith("…")) found.push("is a Progress string not ending in …");
  if (kind === "Status" && text.trim().split(/\s+/).length > 2) found.push("is a Status longer than two words");
  return found;
}

/** Every problem in a catalog: its keys, its plurals, and each string under its key's rules. */
function catalogProblems(section: Section, path: string[] = []): string[] {
  return Object.entries(section).flatMap(([name, entry]) => {
    const key = [...path, name].join(".");
    // Keys ("The string catalog", Keys): nested, camelCase.
    const keyProblems = CAMEL.test(name) ? [] : [`${key}: the key isn't camelCase`];
    if (typeof entry === "string") return [...keyProblems, ...problems(key, entry).map((p) => `${key}: ${p}`)];
    if (isPlural(entry)) {
      // Plurals ("The string catalog", Plurals): forms keyed by category.
      return [
        ...keyProblems,
        ...Object.entries(entry).flatMap(([category, text]) => [
          ...(CATEGORIES.includes(category) ? [] : [`${key}: ${category} isn't a plural category`]),
          ...(typeof text === "string" ? problems(key, text).map((p) => `${key}.${category}: ${p}`) : [`${key}.${category}: isn't a string`]),
        ]),
      ];
    }
    return [...keyProblems, ...catalogProblems(entry, [...path, name])];
  });
}

describe("catalog strings (The string catalog)", () => {
  it("follow every rule", () => {
    expect(catalogProblems(enUS)).toEqual([]);
  });

  it("have one section per area, so each migration adds to its own", () => {
    expect(Object.keys(enUS)).toEqual(["rail", "transcript", "composer", "newSession", "sessionControls", "quit", "trust", "errors"]);
  });
});

describe("the rules for every string", () => {
  it("fail a string that starts lowercase", () => {
    expect(problems("quit.title", "quit Ersa?")).toEqual(["starts lowercase"]);
    expect(problems("quit.title", "Quit Ersa?")).toEqual([]);
    // A value at the start is no letter: the rule reads only written text.
    expect(problems("rail.sessionCount", "{count} sessions")).toEqual([]);
  });

  it("fail an exclamation mark", () => {
    expect(problems("quit.title", "Quit Ersa!")).toEqual(["uses !"]);
  });

  it("fail three periods, in any kind", () => {
    expect(problems("quit.detail", "Wait...")).toEqual(["uses ... rather than …"]);
    expect(problems("quit.detail", "Wait…")).toEqual([]);
  });

  it("fail a brace that isn't a value", () => {
    expect(problems("quit.detail", "Stopping {Count} sessions")).toEqual(["has a brace that isn't a {value}"]);
    expect(problems("quit.detail", "Stopping {count sessions")).toEqual(["has a brace that isn't a {value}"]);
    expect(problems("quit.detail", "Stopping {count} sessions on {host}")).toEqual([]);
  });

  it("fail an empty string", () => {
    expect(problems("quit.title", " ")).toEqual(["is empty"]);
  });
});

describe("the rules by kind", () => {
  it("fail a Button ending in punctuation other than the ellipsis", () => {
    expect(problems("quit.quitButton", "Quit.")).toEqual(["is a Button ending in punctuation"]);
    expect(problems("quit.quitButton", "Quit?")).toEqual(["is a Button ending in punctuation"]);
    expect(problems("quit.quitButton", "Quit. ")).toEqual(["is a Button ending in punctuation"]);
    expect(problems("quit.quitButton", "Quit")).toEqual([]);
    expect(problems("rail.addProjectButton", "Add project…")).toEqual([]);
  });

  it("fail an Error or Empty string without a closing period", () => {
    expect(problems("rail.core.unreachableError", "Core isn't connected")).toEqual(["is an Error string without a closing period"]);
    expect(problems("rail.core.unreachableError", "Core isn't connected. Ersa is trying again.")).toEqual([]);
    expect(problems("rail.sessionsEmpty", "Sessions appear once Core is connected")).toEqual(["is an Empty string without a closing period"]);
    expect(problems("rail.sessionsEmpty", "Sessions appear once Core is connected.")).toEqual([]);
  });

  it("fail a Progress string that doesn't end in the ellipsis", () => {
    expect(problems("rail.core.connectingProgress", "Reconnecting to Core")).toEqual(["is a Progress string not ending in …"]);
    expect(problems("rail.core.connectingProgress", "Reconnecting to Core…")).toEqual([]);
  });

  it("fail a Status longer than two words", () => {
    expect(problems("rail.waitingStatus", "Waiting for you")).toEqual(["is a Status longer than two words"]);
    expect(problems("rail.waitingStatus", "Needs you")).toEqual([]);
  });

  it("give Placeholder, Confirmation and other keys only the every-string rules", () => {
    expect(problems("composer.messagePlaceholder", "Write to the agent")).toEqual([]);
    expect(problems("quit.stopConfirmation", "Quit Ersa? 2 sessions are running and will stop.")).toEqual([]);
    expect(problems("quit.title", "Quit Ersa")).toEqual([]);
    expect(problems("composer.messagePlaceholder", "write to the agent")).toEqual(["starts lowercase"]);
  });
});

describe("the rules for keys and plurals", () => {
  it("fail a key that isn't camelCase", () => {
    expect(catalogProblems({ quit: { "quit-button": "Quit" } })).toEqual(["quit.quit-button: the key isn't camelCase"]);
    expect(catalogProblems({ Quit: { title: "Quit" } })).toEqual(["Quit: the key isn't camelCase"]);
  });

  it("fail a plural form keyed by something other than a category", () => {
    expect(catalogProblems({ rail: { sessionCount: { single: "{count} session", other: "{count} sessions" } } })).toEqual([
      "rail.sessionCount: single isn't a plural category",
    ]);
  });

  it("check each plural form under its key's kind", () => {
    expect(
      catalogProblems({ quit: { stoppingProgress: { one: "Stopping {count} session", other: "Stopping {count} sessions…" } } }),
    ).toEqual(["quit.stoppingProgress.one: is a Progress string not ending in …"]);
  });
});

// --- Copy written into components --------------------------------------------

/** Props whose text a person reads or hears, whatever it looks like. */
const READ_PROPS = new Set(["aria-label", "aria-description", "aria-roledescription", "aria-valuetext", "title", "placeholder", "label", "alt"]);

/** Props no person reads: names, styles, roles and references. */
const CODE_PROPS = new Set(["className", "id", "key", "role", "type", "href", "src", "htmlFor", "autoComplete", "inputMode", "lang"]);

// A component's own props (`summary`, `what`, `unavailable`) carry copy too.
// Any other prop counts when its text reads as words: a capital letter first,
// or a space, which a token such as "accent" or "md" never has.
const readsAsWords = (text: string) => /^\p{Lu}|\p{L}\s+\p{L}/u.test(text);
function propCopy(name: string, texts: string[]): string[] {
  if (READ_PROPS.has(name)) return texts;
  if (CODE_PROPS.has(name) || name.startsWith("data-") || name.startsWith("aria-")) return [];
  return texts.filter(readsAsWords);
}

// Text with a letter in it; spacing and punctuation alone aren't copy.
const isCopy = (text: string) => /\p{L}/u.test(text);
const tidy = (text: string) => text.replace(/\s+/g, " ").trim();

type Node = { type: string; [field: string]: unknown };
const isNode = (value: unknown): value is Node => typeof value === "object" && value !== null && typeof (value as Node).type === "string";

// The strings an expression in a JSX child or a prop would show: string
// literals and template text, through conditionals and `&&`.
function shown(expression: unknown): string[] {
  if (!isNode(expression)) return [];
  if (expression.type === "Literal") return typeof expression.value === "string" ? [expression.value] : [];
  if (expression.type === "TemplateLiteral") {
    return (expression.quasis as { value: { cooked: string } }[]).map((quasi) => quasi.value.cooked);
  }
  if (expression.type === "ConditionalExpression") return [...shown(expression.consequent), ...shown(expression.alternate)];
  if (expression.type === "LogicalExpression") return [...shown(expression.left), ...shown(expression.right)];
  if (expression.type === "JSXExpressionContainer") return shown(expression.expression);
  return [];
}

/** The copy written inline in a component's source: JSX text, and string literals in props a person reads. */
function inlineCopy(file: string, source: string): string[] {
  const { program, errors } = parseSync(file, source);
  if (errors.length > 0) throw new Error(`${file} doesn't parse: ${errors[0]?.message}`);
  const found: string[] = [];
  const visit = (node: unknown): void => {
    if (Array.isArray(node)) return node.forEach(visit);
    if (!isNode(node)) return;
    if (node.type === "JSXText") found.push(node.value as string);
    if (node.type === "JSXExpressionContainer" && node.parent !== "attribute") found.push(...shown(node.expression));
    if (node.type === "JSXAttribute") {
      const name = (node.name as Node).name;
      if (typeof name === "string") found.push(...propCopy(name, shown(node.value)));
      // The value was read above, or isn't read by a person; JSX inside it is still visited.
      if (isNode(node.value) && node.value.type === "JSXExpressionContainer") visit({ ...node.value, parent: "attribute" });
      return;
    }
    for (const [field, child] of Object.entries(node)) if (field !== "parent") visit(child);
  };
  visit(program);
  return [...new Set(found.filter(isCopy).map(tidy))].sort();
}

// The interface's components, by path from src with forward slashes. The
// gallery's own sample text is exempt ("The string catalog"); tests aren't shown.
function componentFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === "gallery" ? [] : componentFiles(path);
    return entry.name.endsWith(".tsx") && !entry.name.includes(".test.") ? [path] : [];
  });
}
const fromSrc = (path: string) => relative(SRC, path).split(sep).join("/");

type CopyByFile = Readonly<Record<string, readonly string[]>>;

// Compared both ways: new copy fails, and so does an entry left after its string moved.
function offList(found: CopyByFile, allowed: CopyByFile): string[] {
  const missing = (from: CopyByFile, other: CopyByFile) =>
    Object.entries(from).flatMap(([file, texts]) => texts.filter((text) => !other[file]?.includes(text)).map((text) => [file, text] as const));
  return [
    ...missing(found, allowed).map(([file, text]) => `${file}: ${JSON.stringify(text)} is inline copy; move it to the catalog`),
    ...missing(allowed, found).map(([file, text]) => `${file}: ${JSON.stringify(text)} is on the allow-list but no longer inline; remove it there`),
  ];
}

describe("component source (The string catalog: no copy in components)", () => {
  it("holds only the inline copy on the allow-list, which the migration tickets empty", () => {
    const found: Record<string, string[]> = {};
    for (const file of componentFiles(SRC)) {
      const copy = inlineCopy(file, readFileSync(file, "utf8"));
      if (copy.length > 0) found[fromSrc(file)] = copy;
    }
    expect(offList(found, allowed)).toEqual([]);
  });
});

describe("the allow-list comparison", () => {
  it("fails copy that isn't on the list, in a listed file or a new one", () => {
    expect(offList({ "A.tsx": ["Quit", "Cancel"], "B.tsx": ["Save"] }, { "A.tsx": ["Quit"] })).toEqual([
      "A.tsx: \"Cancel\" is inline copy; move it to the catalog",
      "B.tsx: \"Save\" is inline copy; move it to the catalog",
    ]);
  });

  it("fails an entry whose string has moved, so the list only shrinks", () => {
    expect(offList({ "A.tsx": ["Quit"] }, { "A.tsx": ["Quit", "Cancel"], "B.tsx": ["Save"] })).toEqual([
      "A.tsx: \"Cancel\" is on the allow-list but no longer inline; remove it there",
      "B.tsx: \"Save\" is on the allow-list but no longer inline; remove it there",
    ]);
  });

  it("passes when the two agree", () => {
    expect(offList({ "A.tsx": ["Quit"] }, { "A.tsx": ["Quit"] })).toEqual([]);
  });
});

describe("the inline-copy scan", () => {
  const scan = (source: string) => inlineCopy("Sample.tsx", source);

  it("finds JSX text", () => {
    expect(scan(`export const A = () => <p>Quit Ersa?</p>;`)).toEqual(["Quit Ersa?"]);
  });

  it("finds strings a JSX child chooses between", () => {
    expect(scan(`export const A = ({ q }: { q: boolean }) => <p>{q ? "Quitting…" : "Quit"}{q && \`Busy\`}</p>;`)).toEqual([
      "Busy",
      "Quit",
      "Quitting…",
    ]);
  });

  it("finds string literals in props a person reads or hears", () => {
    expect(
      scan(`export const A = () => <input aria-label="Message" title={"Send"} placeholder={\`Write\`} label="Name" alt="Logo" />;`),
    ).toEqual(["Logo", "Message", "Name", "Send", "Write"]);
  });

  it("finds words in a component's own props, and passes its tokens", () => {
    expect(scan(`export const A = () => <StatusTile summary="Connecting…" what="The projects" tone="accent" size="md" />;`)).toEqual([
      "Connecting…",
      "The projects",
    ]);
    // A chip's `name` is its spoken label; a form field's `name` is a token.
    expect(scan(`export const A = () => <><Chip name="Branch" /><input name="branch" /></>;`)).toEqual(["Branch"]);
  });

  it("passes props no person reads, and text with no letters", () => {
    expect(scan(`export const A = () => <p className="quit-question" role="alert" data-state="open">{" "}·{1}</p>;`)).toEqual([]);
  });

  it("passes a component that renders only catalog keys", () => {
    expect(scan(`import { t } from "../catalog"; export const A = () => <p aria-label={t("quit.title")}>{t("quit.detail", { count: 2 })}</p>;`)).toEqual([]);
  });

  it("finds copy in JSX passed as a prop", () => {
    expect(scan(`export const A = () => <Card footer={<span>Done</span>} />;`)).toEqual(["Done"]);
  });
});

// --- The text export -----------------------------------------------------------

describe("the text export (The string catalog: Text export)", () => {
  const sample = { quit: { quitButton: "Quit", detail: { one: "{count} session will stop.", other: "{count} sessions will stop." } } };

  it("writes each key as a heading in code, and each plural form under its category", () => {
    expect(markdown(sample)).toContain(
      ["## `quit`", "", "### `quit.quitButton`", "", "Quit", "", "### `quit.detail`", "", "#### `one`", "", "{count} session will stop.", "", "#### `other`", "", "{count} sessions will stop."].join("\n"),
    );
  });

  it("is current for the committed module", () => {
    expect(isCurrent(enUS, join(SRC, "catalog", "en-US.gen.md"))).toBe(true);
  });

  it("is stale when the file differs from the module, which fails --check", () => {
    const dir = mkdtempSync(join(tmpdir(), "ersa-catalog-"));
    try {
      const file = join(dir, "en-US.gen.md");
      writeFileSync(file, markdown(sample));
      expect(isCurrent(sample, file)).toBe(true);
      writeFileSync(file, markdown(sample).replace("Quit\n", "Quit now\n"));
      expect(isCurrent(sample, file)).toBe(false);
      expect(isCurrent(sample, join(dir, "missing.md"))).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
