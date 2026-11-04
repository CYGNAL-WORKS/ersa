// A typecheck fixture for the lookup (docs/design/CONTENT.md, "The string
// catalog", Values): `pnpm typecheck` fails if a call marked as an expected
// error compiles, so each one shows a mistake the types catch.
// The lookup's unit tests use the same sample catalog.

import { createElement } from "react";
import { lookupIn, type Section } from "./lookup";

export const sample = {
  quit: {
    title: "Quit Ersa?",
    quitButton: "Quit",
    detail: {
      one: "{count} session is working and will stop. Closing the window instead keeps it running.",
      other: "{count} sessions are working and will stop. Closing the window instead keeps them running.",
    },
    busySummary: { one: "{readout} session is working.", other: "{readout} sessions are working." },
    stoppingOn: { one: "Stopping {count} session on {host}…", other: "Stopping {count} sessions on {host}…" },
  },
  rail: {
    core: { unreachableError: "Core isn't connected on {host}. Ersa is trying again." },
  },
} as const satisfies Section;

const t = lookupIn(sample, "en-US");

// Correct calls compile, and text-only values give a string.
export const fine: string[] = [
  t("quit.title"),
  t("quit.detail", { count: 2 }),
  t("quit.stoppingOn", { count: 1, host: "devbox" }),
  t("rail.core.unreachableError", { host: "devbox" }),
];

// An element among the values gives a node, not a string.
// @ts-expect-error: a fragment isn't a string.
export const notText: string = t("rail.core.unreachableError", { host: createElement("b", null, "devbox") });

export function mistakes() {
  // @ts-expect-error: no such key.
  t("quit.titel");
  // @ts-expect-error: a section isn't a message.
  t("rail.core");
  // @ts-expect-error: {host} is missing.
  t("rail.core.unreachableError");
  // @ts-expect-error: {host} is misspelled.
  t("rail.core.unreachableError", { hots: "devbox" });
  // @ts-expect-error: a plural needs its count.
  t("quit.detail");
  // @ts-expect-error: the count is a number.
  t("quit.detail", { count: "2" });
  // @ts-expect-error: {host} is missing from a plural's values.
  t("quit.stoppingOn", { count: 2 });
  // @ts-expect-error: a message without values takes none.
  t("quit.title", { count: 2 });
}
