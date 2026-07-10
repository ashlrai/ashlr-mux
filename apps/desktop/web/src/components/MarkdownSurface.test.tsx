import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { markdownSurfaceUrl } from "../session/surfaceUrl";
import { MarkdownSurface } from "./MarkdownSurface";

describe("MarkdownSurface", () => {
  test("renders a titled, sandboxed iframe pointed at the markdown shell", () => {
    const markup = renderToStaticMarkup(<MarkdownSurface panelId="surface-8" />);
    expect(markup).toContain(`src="${markdownSurfaceUrl("surface-8")}"`);
    expect(markup).toContain('class="cmux-markdown-surface"');
    expect(markup).toContain('title="Markdown preview"');
    expect(markup).toContain('sandbox="allow-scripts allow-same-origin"');
    expect(markup).toContain('data-cmux-markdown-panel-id="surface-8"');
    expect(markup).toContain('aria-label="Markdown controls"');
    expect(markup).toContain('aria-label="Markdown Zoom Out"');
    expect(markup).toContain('aria-label="Markdown Reset Zoom"');
    expect(markup).toContain('aria-label="Markdown Zoom In"');
  });

  test("a provided document is render-inert (effect-only feed, identical markup)", () => {
    const bare = renderToStaticMarkup(<MarkdownSurface panelId="surface-8" />);
    const fed = renderToStaticMarkup(
      <MarkdownSurface panelId="surface-8" filePath="C:/docs/a.md" />,
    );
    expect(fed).toBe(bare);
  });
});
