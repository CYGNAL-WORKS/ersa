import { useRef, useState } from "react";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import { TrustQuestion, folderName } from "./TrustQuestion";
import "./AddProject.css";

interface AddProjectProps {
  /** Adds the folder at `path` as a project, trusted or not; resolves true once Core added it. */
  onAdd: (path: string, trusted: boolean) => Promise<boolean>;
  adding?: Sending;
  /** For the gallery: a path already typed, the trust question already asked, and a pointer or keyboard state shown without one. */
  initialPath?: string;
  initialAsking?: boolean;
  force?: "hover" | "focus";
}

/** Adds a project by its folder's path on this machine, once the user has said whether they trust it. */
export function AddProject({ onAdd, adding, initialPath = "", initialAsking = false, force }: AddProjectProps) {
  const [path, setPath] = useState(initialPath);
  // Whether the trust question is showing; the project is added only once it is answered.
  const [asking, setAsking] = useState(initialAsking);
  const busy = adding?.state === "sending";
  // Focus goes back to the path once the question closes, where the user was before it opened.
  const input = useRef<HTMLInputElement>(null);
  const close = () => {
    setAsking(false);
    input.current?.focus();
  };
  const answer = async (trusted: boolean) => {
    close();
    if (await onAdd(path.trim(), trusted)) setPath("");
  };
  return (
    <form
      className="add-project"
      aria-label="Add a project"
      onSubmit={(event) => {
        event.preventDefault();
        if (busy || path.trim() === "") return;
        setAsking(true);
      }}
    >
      <input
        ref={input}
        className="add-project-input mono"
        aria-label="Project folder"
        placeholder="Folder path"
        value={path}
        readOnly={busy}
        data-force={force}
        onChange={(event) => {
          // The question was about the path as it was; a new path is asked about afresh.
          setAsking(false);
          setPath(event.target.value);
        }}
      />
      <Button type="submit" disabled={busy || asking || path.trim() === ""}>
        {/* Both labels share one cell, so the button is always as wide as the wider and nothing beside it moves. */}
        <span className="add-project-label">
          <span data-shown={!busy || undefined}>Add</span>
          <span data-shown={busy || undefined}>Adding…</span>
        </span>
      </Button>
      {asking && (
        <div className="add-project-question">
          <TrustQuestion name={folderName(path.trim())} onAnswer={answer} onDismiss={close} />
        </div>
      )}
      {adding?.state === "failed" && (
        <p className="problem add-project-problem" role="alert">
          {adding.message}
        </p>
      )}
    </form>
  );
}
