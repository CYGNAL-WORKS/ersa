#!/usr/bin/env node
// A stand-in for npm, for the sign-in recorder's tests of --cli-version
// (scripts/record-claude-sign-in.test.mjs). It fetches nothing: given
// `install --prefix <dir> ... <name>@<version>`, it lays out a package in
// <dir>/node_modules/<name> whose `bin` is the stand-in CLI
// (fake-claude-sign-in.mjs), printing that version.
//
// FAKE_NPM_LOG, when set, names a file the arguments are written to, as JSON.
// FAKE_NPM_PLAY changes what it does:
//   wrong-version    the package prints another version than the one asked for
//   fail             nothing is installed, and it exits 1

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const args = process.argv.slice(2);
if (process.env.FAKE_NPM_LOG) writeFileSync(process.env.FAKE_NPM_LOG, JSON.stringify(args));
if (process.env.FAKE_NPM_PLAY === "fail") process.exit(1);

const prefix = args[args.indexOf("--prefix") + 1];
const spec = args.at(-1);
const at = spec.lastIndexOf("@");
const [name, version] = [spec.slice(0, at), spec.slice(at + 1)];
const printed = process.env.FAKE_NPM_PLAY === "wrong-version" ? "0.0.1" : version;

const dir = join(prefix, "node_modules", ...name.split("/"));
mkdirSync(dir, { recursive: true });
const cli = pathToFileURL(join(fileURLToPath(import.meta.url), "..", "fake-claude-sign-in.mjs")).href;
writeFileSync(join(dir, "package.json"), JSON.stringify({ name, version, type: "module", bin: { claude: "claude.mjs" } }));
writeFileSync(join(dir, "claude.mjs"), `process.env.FAKE_CLAUDE_VERSION = ${JSON.stringify(printed)};\nawait import(${JSON.stringify(cli)});\n`);
