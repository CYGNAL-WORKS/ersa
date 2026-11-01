import { useState } from "react";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import "./AddProject.css";

interface AddProjectProps {
  /** Adds the folder at `path` as a project; resolves true once Core added it. */
  onAdd: (path: string) => Promise<boolean>;
  adding?: Sending;
  /** For the gallery: a path already typed. */
  initialPath?: string;
}

/** Adds a project by its folder's path on this machine. */
export function AddProject({ onAdd, adding, initialPath = "" }: AddProjectProps) {
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
        onChange={(event) => setPath(event.target.value)}
      />
      <Button type="submit" disabled={busy || path.trim() === ""}>
        {busy ? "Adding…" : "Add"}
      </Button>
      {adding?.state === "failed" && (
        <p className="problem add-project-problem" role="alert">
          {adding.message}
        </p>
      )}
    </form>
  );
}
