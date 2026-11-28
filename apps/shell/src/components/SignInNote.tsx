import { useState } from "react";
import { t } from "../catalog";
import type { NoteState, SignInWords } from "../session/signIn";
import { reducedMotion } from "../theme/motion";
import { Button, ButtonLabels } from "./Button";
import { ContextRing } from "./ContextRing";
import "./SignInNote.css";

/** What a failure says, by the backend's label, then the reason Core or the backend gave (FLOW-SIGN-IN-R03, R05, R12 to R14). */
function failureWords(label: string, said: SignInWords): string {
  switch (said.kind) {
    case "refused":
      // An empty reason, as a backend's own error text may be, leaves the sentence alone (FLOW-SIGN-IN-R03).
      return said.reason ? `${t("composer.signIn.startError", { label })} ${said.reason}` : t("composer.signIn.startError", { label });
    case "unopened":
      return t("composer.signIn.unopenedError", { label });
    case "failed":
      return said.reason ? `${t("composer.signIn.failedError", { label })} ${said.reason}` : t("composer.signIn.failedError", { label });
    case "timedOut":
      return t("composer.signIn.timedOutError", { label });
    case "stillSignedOut":
      return t("composer.signIn.stillSignedOutError", { label });
  }
}

/** Whether the words crossfade: not with reduced motion, nor where the window can't say (a test's DOM). */
function fades(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && !reducedMotion();
}

/**
 * The sign-in note (docs/design/COMPOSER.md, The sign-in note): one row above the status line saying, by the
 * backend's label, that it isn't signed in, with one button that follows the sign-in: Sign in, Starting… while Core
 * answers, Cancel while the person signs in in their browser. It is one button throughout, so keyboard focus stays on
 * it through its changes. Nothing in it takes typing (FLOW-SIGN-IN-R08): the sign-in is the backend's own, on its
 * vendor's page. The words it says while signed out are the place's own: what signing in lets the person do there.
 */
export function SignInNote({
  label,
  state,
  place = "newSession",
  onSignIn,
  onCancel,
  focal,
  force,
}: {
  /** The backend's label, such as "Claude Code". */
  label: string;
  state: NoteState;
  /** Where it stands: on the new-session screen, or under a session whose message failed as not signed in (FLOW-SIGN-IN-R17). */
  place?: "newSession" | "session";
  onSignIn: () => void;
  onCancel: () => void;
  /** The note holds the screen's focal glow: on the new-session screen, while nothing else there can be acted on. */
  focal?: boolean;
  /** For the gallery: a pointer or keyboard state on the button without one. */
  force?: "hover" | "focus" | "pressed";
}) {
  const said = state.kind === "signedOut" ? state.said : undefined;
  const waiting = state.kind === "waiting";
  const words = said
    ? failureWords(label, said)
    : waiting
      ? t("composer.signIn.waitingProgress", { label })
      : t(place === "session" ? "composer.signIn.sessionNote" : "composer.signIn.signedOutNote", { label });

  // The words crossfade over motion-quick: the ones going fade out in the same cell as the ones coming fade in, so
  // the row keeps its place (docs/design/COMPOSER.md).
  const [shown, setShown] = useState({ now: words, was: undefined as string | undefined, turn: 0 });
  if (shown.now !== words) setShown({ now: words, was: fades() ? shown.now : undefined, turn: shown.turn + 1 });

  // Starting… while Core answers a Sign in; Sign in held while it answers a Cancel.
  const busy = state.kind === "starting";
  const held = busy || state.kind === "canceling";
  const tone = said ? "danger" : "caution";
  return (
    <div className="sign-in-note" data-focal={focal || undefined}>
      <span className="sign-in-mark">
        {waiting ? <ContextRing turning /> : <span className="glyph" data-tone={tone} aria-hidden="true" />}
      </span>
      <p className="sign-in-words">
        {shown.was !== undefined && (
          <span
            key={`was-${shown.turn}`}
            className="sign-in-line"
            data-leaving
            aria-hidden
            onAnimationEnd={() => setShown((s) => (s.turn === shown.turn ? { ...s, was: undefined } : s))}
          >
            {shown.was}
          </span>
        )}
        <span key={`now-${shown.turn}`} className="sign-in-line" data-arriving={shown.turn > 0 || undefined}>
          {words}
        </span>
      </p>
      {/* Mounted with the note, so words arriving in them are announced: the wait as a status, a failure as an alert. */}
      <span className="heard-only" role="status">
        {waiting ? words : ""}
      </span>
      <span className="heard-only" role="alert">
        {said ? words : ""}
      </span>
      {/* Unavailable while Core answers, yet still focusable, so keyboard focus stays on it. */}
      <Button
        variant={waiting ? "quiet" : "primary"}
        aria-disabled={held || undefined}
        onClick={() => {
          if (held) return;
          if (waiting) onCancel();
          else onSignIn();
        }}
        force={force}
      >
        {/* Every label in one cell, so the button keeps its width and place from one state to the next. */}
        <ButtonLabels
          labels={{
            signIn: t("composer.signIn.signInButton"),
            starting: t("composer.signIn.startingProgress"),
            cancel: t("composer.signIn.cancelButton"),
          }}
          shown={busy ? "starting" : waiting ? "cancel" : "signIn"}
          busy="starting"
        />
      </Button>
    </div>
  );
}
