import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { FeedPanel } from "./FeedPanel";

describe("FeedPanel", () => {
  test("renders the canonical actionable empty state", () => {
    const markup = renderToStaticMarkup(<FeedPanel />);
    expect(markup).toContain('aria-label="Feed"');
    expect(markup).toContain("Actionable");
    expect(markup).toContain("All Activity");
    expect(markup).toContain("No pending decisions");
    expect(markup).toContain(
      "Permission, plan, and question requests from AI agents will appear here.",
    );
  });
});
