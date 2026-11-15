// The session controls rendered to markup, as the window would first draw them (docs/design/COMPOSER.md).

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Project } from "../protocol/core-api.gen";
import { SessionControls } from "./SessionControls";

const project: Project = { id: "p1", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true };
const host = { id: "local", name: "devbox", platform: "windows" } as const;

describe("the workspace list's base branch", () => {
  it("says the project has no branches, not that none matches, when the repository lists none", () => {
    const shown = renderToStaticMarkup(
      createElement(SessionControls, {
        hosts: [host],
        localHostId: "local",
        projects: [project],
        backends: [],
        values: { host_id: "local", project_id: "p1", workspace: "isolated" },
        branches: { state: "read", branches: [] },
        onChoose: () => {},
        open: "workspace",
      }),
    );
    expect(shown).toContain("This project has no branches yet.");
    expect(shown).not.toContain("No branch matches.");
  });
});
