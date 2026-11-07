// A message's attachments in the composer (FLOW-SEND-MESSAGE, COMPOSER.md Attachments): what each shows, which are
// the same, which will be copied into the workspace, and what `session.send` gets. Kept free of React so the tests
// drive it with plain data.

import { t } from "../catalog";
import type { Attachment, AttachmentInspectResult } from "../protocol/core-api.gen";

/** One attachment as the composer holds it: what `attachment.inspect` said of a path the user picked. */
export interface Attached {
  path: string;
  name: string;
  kind: AttachmentInspectResult["kind"];
  /** Its detail: a size, a line count, or that it is a folder (FLOW-SEND-MESSAGE-R09). */
  detail: string;
  /** Whether sending copies it, as Core judged it for one workspace, or for none yet (FLOW-SEND-MESSAGE-R17). */
  copied?: { workspace: string | undefined; copied: boolean };
  /** A pasted image's bytes, sent inline rather than by path (FLOW-SEND-MESSAGE-R08). */
  inline?: { media_type: string; data: string };
  /** Why Ersa can't send it, said beside it; the message waits until it is removed (FLOW-SEND-MESSAGE-R10). */
  problem?: string;
}

/** A size as people read one: bytes, then KB, MB and GB in steps of 1024. */
export function size(bytes: number): string {
  const units = ["composer.attachment.kilobytesDetail", "composer.attachment.megabytesDetail", "composer.attachment.gigabytesDetail"] as const;
  if (bytes < 1024) return t("composer.attachment.bytesDetail", { count: bytes });
  let value = bytes / 1024;
  let unit = 0;
  // Promoted on the value as shown, so a size just under a unit reads as one of it, not 1024 of the one below.
  const shown = (v: number) => (v < 10 ? Math.round(v * 10) / 10 : Math.round(v));
  while (shown(value) >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return t(units[unit] ?? units[0], { size: value < 10 ? value.toFixed(1).replace(/\.0$/, "") : String(Math.round(value)) });
}

/**
 * The detail an attachment shows (FLOW-SEND-MESSAGE-R09): the size of an image, the line count of a text file and the
 * size of any other file, or that it is a folder.
 */
export function detail(info: AttachmentInspectResult): string {
  if (info.kind === "folder") return t("composer.attachment.folderDetail");
  if (info.kind === "file" && info.lines !== undefined) return t("composer.attachment.linesDetail", { count: info.lines });
  return size(info.size_bytes ?? 0);
}

/** A path compared as Windows does, without regard to case or which slash, and with no trailing slash. */
function key(path: string): string {
  const slashed = path.replaceAll("\\", "/").replace(/\/+$/, "");
  return /^[a-z]:\//i.test(slashed) ? slashed.toLowerCase() : slashed;
}

/** Whether two paths name the same file or folder. */
export function same(a: string, b: string): boolean {
  return key(a) === key(b);
}

/** `info` as the composer holds it, with whether sending copies it, as Core judged it for `workspace`. */
export function attached(info: AttachmentInspectResult, workspace?: string): Attached {
  return { path: info.path, name: info.name, kind: info.kind, detail: detail(info), copied: { workspace, copied: info.copied } };
}

/**
 * Whether sending `a` copies it into the workspace's `.ersa` folder (FLOW-SEND-MESSAGE-R17): Core's `copied`
 * alone, since Core alone resolves links and junctions. One Core judged for another workspace than the composer's
 * now, as after choosing another on the new-session screen, is taken to be copied, so the note never says a copy
 * won't happen when it may.
 */
export function willCopy(a: Attached, workspace: string | undefined): boolean {
  if (a.copied === undefined || a.copied.workspace !== workspace) return a.kind !== "image";
  return a.copied.copied;
}

/** A path Ersa couldn't read, as it no longer exists: named by its last part and marked with why (R10). */
export function unreadable(path: string, problem: string): Attached {
  const name = path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;
  return { path, name, kind: "file", detail: "", problem };
}

/** `list` with each of `more` it does not hold yet, in order: the same file or folder is attached once (R12). */
export function addAll(list: readonly Attached[], more: readonly Attached[]): Attached[] {
  const held = new Set(list.map((a) => key(a.path)));
  const out = [...list];
  for (const a of more) {
    if (held.has(key(a.path))) continue;
    held.add(key(a.path));
    out.push(a);
  }
  return out;
}

/**
 * What `session.send` gets: each attachment by its kind and the path the user picked or dropped
 * (FLOW-SEND-MESSAGE-R13), and a pasted image by its bytes (R08).
 */
export function sendable(list: readonly Attached[]): Attachment[] {
  return list.map(({ kind, path, inline }) => (inline ? { kind: "image" as const, ...inline } : { kind, path }));
}

// Each pasted image gets its own key in place of a path, so two pastes are two attachments.
let pastes = 0;

/**
 * An image pasted into the composer, attached inline (FLOW-SEND-MESSAGE-R08): named as pasted, its detail its size.
 * Whether the backend takes its kind and size is Core's to say when the message is sent (R10).
 */
export async function pastedImage(file: Blob): Promise<Attached> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  // btoa takes a string of bytes; built in chunks, so a large image does not overflow the call stack.
  let binary = "";
  for (let at = 0; at < bytes.length; at += 0x8000) binary += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  pastes += 1;
  return {
    path: `pasted:${pastes}`,
    name: t("composer.attachment.pastedName"),
    kind: "image",
    detail: size(bytes.length),
    inline: { media_type: file.type, data: btoa(binary) },
  };
}
