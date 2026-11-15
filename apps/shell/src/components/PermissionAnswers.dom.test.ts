// @vitest-environment jsdom
// A permission request's answers in a DOM: what each one decides.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { PermissionRequested } from "../protocol/agent-events.gen";
import type { Decision } from "../session/answers";
import { PermissionAnswers } from "./PermissionAnswers";

const request: PermissionRequested = {
  type: "permission.requested",
  request_id: "r1",
  tool_use_id: "t1",
  tool_name: "Bash",
  input: { command: "pnpm test" },
  suggestions: [
    { kind: "allow_rule", label: "Allow pnpm commands", payload: {} },
    { kind: "set_mode", label: "Switch to acceptEdits mode", payload: {} },
  ],
  flags: { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false },
};

afterEach(cleanup);

describe("a permission request's answers", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R13
  it("decides allow, deny, or allow with the chosen suggestion's index", () => {
    const decided: Decision[] = [];
    render(createElement(PermissionAnswers, { request, live: true, onDecide: (d: Decision) => decided.push(d) }));
    for (const name of ["Allow", "Deny", "Switch to acceptEdits mode", "Allow pnpm commands"]) {
      fireEvent.click(screen.getByRole("button", { name }));
    }
    expect(decided).toEqual(["allow", "deny", { suggestion: 1 }, { suggestion: 0 }]);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R12
  // verifies: FLOW-APPROVE-PERMISSION-R30
  it("names each suggestion by its whole label, with no tooltip standing in for it", () => {
    const label = "Always allow Bash(echo probe *) in this project's local settings";
    const long = { ...request, suggestions: [{ kind: "allow_rule" as const, label, payload: {} }] };
    render(createElement(PermissionAnswers, { request: long, live: true, onDecide: () => {} }));
    const suggestion = screen.getByRole("button", { name: label });
    expect(suggestion.textContent).toBe(label);
    expect(suggestion.hasAttribute("title")).toBe(false);
  });
});
