// What components import: `t("quit.quitButton")`, `t("rail.sessionCount", { count })`.

import { enUS } from "./en-US";
import { lookupIn } from "./lookup";

/** Looks up a US English string by key and fills its values. */
export const t = lookupIn(enUS, "en-US");
