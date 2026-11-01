import { useState } from "react";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import "./AddProject.css";

interface AddProjectProps {
  /** Adds the folder at `path` as a project; resolves true once Core added it. */
  onAdd: (path: string) => Promise<boolean>;
  adding?: Sending;
  /** For the gallery: a path already typed, and a pointer or keyboard state shown without one. */
  initialPath?: string;
  force?: "hover" | "focus";
}

/** Adds a project by its folder's path on this machine. */
export function AddProject({ onAdd, adding, initialPath = "", force }: AddProjectProps) {
  const [path, setPath] = useState(initialPath);
  const busy = adding?.state === "sending";
  return (
    <form
      className="add-project"
      aria-label="Add a project"
      onSubmit={async (event) => {
        event.preventDefault();
        if (busy || path.trim() === "") return;
        if (await onAdd(path.trim())) setPath("");
      }}
    >
      <input
        className="add-project-input mono"
        aria-label="Project folder"
        placeholder="Folder path"
        value={path}
        readOnly={busy}
        data-force={force}
        onChange={(event) => setPath(event.target.value)}
      />
      <Button type="submit" disabled={busy || path.trim() === ""}>
        {/* Both labels share one cell, so the button is always as wide as the wider and nothing beside it moves. */}
        <span className="add-project-label">
          <span data-shown={!busy || undefined}>Add</span>
          <span data-shown={busy || undefined}>Adding…</span>
        </span>
      </Button>
      {adding?.state === "failed" && (
        <p className="problem add-project-problem" role="alert">
          {adding.message}
        </p>
      )}
    </form>
  );
}
