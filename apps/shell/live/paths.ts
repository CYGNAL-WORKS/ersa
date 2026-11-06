import path from "node:path";
import { fileURLToPath } from "node:url";

/** The repository root; this file is apps/shell/live/paths.ts. */
export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

/**
 * A target folder of the live checks' own: their `ersa` embeds the built Interface (Tauri's `custom-protocol`),
 * where the ordinary debug build loads it from a dev server, and building both into one folder would rebuild Tauri
 * each time the other is built.
 */
export const targetDir = path.join(root, "target", "live-check");

/** The executables a live check runs, as `build.ts` builds them. */
export function binaries() {
  const debug = path.join(targetDir, "debug");
  return {
    core: path.join(debug, "ersa-core.exe"),
    shell: path.join(debug, "ersa.exe"),
    standIn: path.join(debug, "examples", "stand_in_backend.exe"),
  };
}
