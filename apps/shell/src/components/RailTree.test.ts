import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Project, Session } from "../protocol/core-api.gen";
import { RailTree } from "./RailTree";

// What the tree draws from the grouping (session/tree.test.ts covers the grouping, and FLOW-NEW-SESSION-R13 there).
// LAY-SHELL is a layout brief with no requirement IDs.
const project = (id: string, more: Partial<Project> = {}): Project => ({ id, host_id: "local", path: `D:\\${id}`, name: id, vcs: "git", trusted: true, ...more });
const session = (id: string, workspace_id: string, title: string): Session => ({
  id,
  workspace_id,
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at: "2026-11-01T09:00:00.000Z",
  archived: false,
  title,
  tile: { state: "idle", pending: [] },
});
type Props = Parameters<typeof RailTree>[0];
const render = (props: Partial<Props>) =>
  renderToStaticMarkup(
    createElement(RailTree, {
      projects: { state: "ready", projects: [project("ersa")] },
      sessions: { state: "ready", sessions: [] },
      workspaces: {},
      onSelect: () => {},
      localHost: "local",
      ...props,
    }),
  );

describe("the rail's tree", () => {
  it("names a project's host only when the project is not on the local machine", () => {
    const html = render({
      projects: { state: "ready", projects: [project("ersa"), project("runner", { host_id: "devbox" }), project("other", { host_id: "far" })] },
      // As host.list answers: every host, this machine included; "far" is not listed, so it is named by its id.
      hostNames: { local: "WORKSTATION", devbox: "devbox box" },
    });
    expect(html).toContain('<span class="rail-heading-host">devbox box</span>');
    expect(html).toContain('<span class="rail-heading-host">far</span>');
    expect(html).not.toContain("WORKSTATION");
  });

  it("says a project is not trusted on its heading, and nothing of trust for a trusted one", () => {
    const html = render({ projects: { state: "ready", projects: [project("ersa"), project("stranger", { trusted: false })] } });
    expect(html.match(/Not trusted/g)).toHaveLength(1);
    expect(html.indexOf("stranger")).toBeLessThan(html.indexOf("Not trusted"));
  });

  it("puts a session it can't place after every heading, and holds it back while workspaces are still read", () => {
    const props: Partial<Props> = {
      projects: { state: "ready", projects: [project("ersa"), project("notes")] },
      sessions: { state: "ready", sessions: [session("a", "w-ersa", "Placed"), session("b", "w-unknown", "Unplaced")] },
      workspaces: { "w-ersa": { project_id: "ersa" } },
    };
    const html = render(props);
    expect(html.indexOf("Placed")).toBeLessThan(html.indexOf("notes"));
    expect(html.indexOf("notes")).toBeLessThan(html.indexOf("Unplaced"));
    expect(render({ ...props, placing: true })).not.toContain("Unplaced");
  });

  it("says why a list could not be read and shows what it could", () => {
    const html = render({
      projects: { state: "error", message: "the database is locked" },
      sessions: { state: "ready", sessions: [session("a", "w", "Still here")] },
    });
    expect(html).toContain("Ersa can&#x27;t read your projects.");
    expect(html).toContain("the database is locked");
    expect(html).toContain("Still here");
    const failed = render({ workspaceErrors: { ersa: "no answer" } });
    expect(failed).toContain("Ersa can&#x27;t read this project&#x27;s workspaces.");
    expect(failed).toContain("no answer");
  });

  it("says what changing trust does: saving, failing, and running sessions keeping theirs", () => {
    expect(render({ trusting: { ersa: { state: "sending" } } })).toContain('role="status">Saving…</p>');
    const refused = render({ trusting: { ersa: { state: "failed", message: "no project ersa" } } });
    expect(refused).toContain("Ersa can&#x27;t change this project&#x27;s trust.");
    expect(refused).toContain("no project ersa");
    expect(render({ keepingTrust: ["ersa"] })).toContain("Running sessions keep their current settings until they are stopped.");
  });

  it("counts a collapsed heading's sessions, and collapsed and not trusted, gives its words to the shield and a tooltip", () => {
    const sessions = { state: "ready" as const, sessions: [session("a", "w-ersa", "One"), session("b", "w-ersa", "Two")] };
    const workspaces = { "w-ersa": { project_id: "ersa" } };
    const trusted = render({ sessions, workspaces, collapsed: new Set(["ersa"]) });
    expect(trusted).toContain('aria-expanded="false"');
    expect(trusted).toContain('<span class="rail-heading-count">2 sessions</span>');
    expect(trusted).not.toContain("rail-heading-shield");

    const untrusted = render({ projects: { state: "ready", projects: [project("ersa", { trusted: false })] }, sessions, workspaces, collapsed: new Set(["ersa"]) });
    // The shield, named for assistive technology, then the same count every collapsed heading shows.
    expect(untrusted).toMatch(/<span class="rail-heading-shield" style="[^"]*" role="img" aria-label="Not trusted">/);
    expect(untrusted).toContain('<span class="rail-heading-count">2 sessions</span>');
    expect(untrusted.indexOf("rail-heading-shield")).toBeLessThan(untrusted.indexOf("rail-heading-count"));
    // Its words as a tooltip, which assistive technology does not read twice.
    expect(untrusted).toMatch(/<span class="rail-heading-tip" aria-hidden="true" style="[^"]*">Not trusted<\/span>/);

    // Expanded, the shield is a plain icon beside its words, and there is no tooltip.
    const open = render({ projects: { state: "ready", projects: [project("ersa", { trusted: false })] }, sessions, workspaces });
    expect(open).toMatch(/<span class="rail-heading-shield" style="[^"]*">/);
    expect(open).toContain('<span class="rail-heading-trust">Not trusted</span>');
    expect(open).not.toContain("rail-heading-tip");
  });

  it("offers Rename as unavailable, with its reason, while Ersa can't reach Core", () => {
    const html = render({
      sessions: { state: "ready", sessions: [session("a", "w-ersa", "One")] },
      workspaces: { "w-ersa": { project_id: "ersa" } },
      onRename: async () => true,
      offline: true,
      initialTileMenu: "a",
    });
    expect(html).toMatch(/role="menuitem"[^>]*aria-disabled="true"[^>]*>.*Rename.*Available once Ersa reaches your sessions/);
  });

  it("is empty when Ersa can't reach the sessions, and loading while it first connects", () => {
    expect(render({ projects: { state: "unavailable" }, sessions: { state: "unavailable" } })).not.toContain("<p");
    expect(render({ projects: { state: "loading" }, sessions: { state: "loading" } })).toContain("Loading…");
  });
});
