import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { CoreHelloResult } from "../protocol/core-api.gen";
import { RailFoot } from "./RailFoot";

const hello = { core_version: "0.1.0", api_version: 1, host: { id: "local", name: "PC" } } as unknown as CoreHelloResult;
const draw = (view: Parameters<typeof RailFoot>[0]["view"]) => renderToStaticMarkup(createElement(RailFoot, { view, onRetry: () => {} }));

// LAY-SHELL, The foot; issue #155: assistive technology is told of a problem.
describe("what the rail's foot tells assistive technology", () => {
  it("keeps an empty status region while all is well, so a problem arriving in it is announced", () => {
    const html = draw({ phase: "connecting" });
    expect(html).toMatch(/^<div class="rail-foot-unseen" role="status"><\/div>$/);
  });

  it("puts the trying-again line and its reason in that status region", () => {
    const html = draw({ phase: "lost", reason: "The connection closed.", hello });
    expect(html).toMatch(/^<div class="rail-foot" role="status"><p [^>]*>Ersa can&#x27;t reach your sessions\. It&#x27;s trying again\.<\/p><p [^>]*>The connection closed\.<\/p><\/div>$/);
  });

  it("raises an alert once Ersa has stopped trying, with Retry inside it", () => {
    const html = draw({ phase: "failed", message: "The connection closed." });
    expect(html).toMatch(/<div class="rail-foot" role="alert"><p class="problem">Ersa can&#x27;t reach your sessions\.<\/p>.*<button[^>]*>Retry<\/button><\/div>$/);
    // The status region stays, empty.
    expect(html).toMatch(/^<div class="rail-foot-unseen" role="status"><\/div>/);
  });
});
