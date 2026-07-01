import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { Icon } from "@cmux/webviews/src/icons";

/**
 * Phase 1 reuse proof: a genuinely-reused `cmux/webviews` component, imported
 * across the workspace, renders under the desktop app's React instance. If the
 * workspace link, the single-React-instance guarantee, or the TSX transform
 * regressed, this render would throw or emit nothing.
 */
describe("reused webviews component", () => {
  test("renders the webviews Icon to SVG markup via the shared React", () => {
    const markup = renderToStaticMarkup(<Icon name="classic" />);
    expect(markup).toContain("<svg");
    expect(markup).toContain("</svg>");
  });
});
