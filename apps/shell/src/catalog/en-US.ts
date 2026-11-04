// The string catalog's US English source (docs/design/CONTENT.md, "The string
// catalog"). Every string the Interface shows lives here, keyed by area and
// purpose, and a key's suffix names its kind (`Button`, `Error`, `Progress`,
// `Status`, `Placeholder`, `Empty`, `Confirmation`), which the catalog test
// checks. Another locale is a sibling file with the same keys.
//
// After a change, run `node scripts/gen-catalog.mjs` to rewrite the text
// export, en-US.gen.md, which is what review and Vale read.
//
// Plain data with no runtime imports, so that script can load it directly.

import type { Section } from "./lookup";

export const enUS = {
  rail: {},
  transcript: {},
  composer: {},
  newSession: {},
  sessionControls: {},
  quit: {},
  trust: {},
  errors: {},
} as const satisfies Section;
