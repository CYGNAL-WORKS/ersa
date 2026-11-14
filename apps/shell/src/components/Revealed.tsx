// Text the agent sent, with every character that would otherwise hide shown as its code point: the transcript, the
// shelf and the permission answers all show agent text so.

import type { ReactNode } from "react";
import { t } from "../catalog";

/**
 * Characters that change what text appears to say without showing
 * themselves: every control (other than tab, newline, and a carriage return
 * before a newline), every format character (bidirectional marks, overrides
 * and isolates, zero-width characters, tag characters), every default-
 * ignorable code point (fillers, variation selectors), line and paragraph
 * separators, and the blank braille pattern. An agent could use them to make
 * a command read as something else.
 */
const HIDDEN =
  /\r(?!\n)|(?![\t\n\r])[\p{Cc}\p{Cf}\p{Default_Ignorable_Code_Point}\p{Zl}\p{Zp}\u2800]/gu;

/** Letters of the scripts whose spelling uses joiners. Never Latin: there a joiner only hides a split word. */
const JOINING = /^[\p{Script=Arabic}\p{Script=Syriac}\p{Script=Devanagari}\p{Script=Bengali}\p{Script=Gurmukhi}\p{Script=Gujarati}\p{Script=Oriya}\p{Script=Tamil}\p{Script=Telugu}\p{Script=Kannada}\p{Script=Malayalam}\p{Script=Sinhala}]$/u;

/**
 * Joiners and the emoji presentation selector do real work inside emoji
 * (a warning sign drawn as emoji, family sequences) and between letters of
 * scripts that need them (Arabic, Indic); there they hide nothing and are
 * left as they are. Anywhere else, they are marked.
 */
function benign(text: string, at: number, hidden: string): boolean {
  const code = hidden.codePointAt(0);
  const prev = text.slice(Math.max(0, at - 2), at);
  const after = text.slice(at + hidden.length, at + hidden.length + 2);
  if (code === 0xfe0f) return /\p{Extended_Pictographic}$/u.test(prev);
  if (code === 0x200d && /(\p{Extended_Pictographic}|\p{Emoji_Modifier}|\uFE0F)$/u.test(prev)) {
    return /^\p{Extended_Pictographic}/u.test(after);
  }
  if (code === 0x200c || code === 0x200d) {
    // The letter before, past any marks on it (harakat, vowel signs).
    const base = text.slice(0, at).replace(/\p{M}+$/u, "").slice(-1);
    return JOINING.test(base) && JOINING.test(after.slice(0, 1));
  }
  return false;
}

/** Past this many, hidden characters are counted rather than each marked, so a flood of them cannot stall the window. */
const MARKED = 64;

/** `text` with every hidden character shown as a marked code point, so what the user reads is what the agent sent. */
export function Revealed({ text }: { text: string }) {
  const parts: ReactNode[] = [];
  let last = 0;
  let marked = 0;
  for (const match of text.matchAll(HIDDEN)) {
    const at = match.index;
    if (benign(text, at, match[0])) continue;
    if (marked === MARKED) {
      // Past the cap each hidden character stays where it was, as the
      // replacement character in one plain run of text (cheap however many
      // there are), and a note at the end says how many there were.
      const rest = text.slice(last);
      let more = 0;
      parts.push(
        rest.replace(HIDDEN, () => {
          more++;
          return "\uFFFD";
        }),
        <span key="more" className="transcript-hidden" title={t("transcript.hidden.manyLabel")}>
          {t("transcript.hidden.moreLabel", { count: more, mark: "\uFFFD" })}
        </span>,
      );
      return <>{parts}</>;
    }
    marked++;
    if (at > last) parts.push(text.slice(last, at));
    const code = (match[0].codePointAt(0) ?? 0).toString(16).toUpperCase().padStart(4, "0");
    parts.push(
      <span key={at} className="transcript-hidden" title={t("transcript.hidden.oneLabel")}>
        {t("transcript.hidden.codePointLabel", { code })}
      </span>,
    );
    last = at + match[0].length;
  }
  if (parts.length === 0) return text;
  if (last < text.length) parts.push(text.slice(last));
  return <>{parts}</>;
}
