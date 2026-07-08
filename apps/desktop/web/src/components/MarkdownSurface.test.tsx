import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { markdownSurfaceUrl } from "../session/surfaceUrl";
import { MarkdownSurface } from "./MarkdownSurface";

describe("MarkdownSurface", () => {
  test("renders a titled, sandboxed iframe pointed at the markdown shell", () => {
    const markup = renderToStaticMarkup(<MarkdownSurface />);
    expect(markup).toContain(`src="${markdownSurfaceUrl()}"`);
    expect(markup).toContain('class="cmux-markdown-surface"');
    expect(markup).toContain('title="Markdown preview"');
    expect(markup).toContain('sandbox="allow-scripts allow-same-origin"');
  });

  test("a provided document is render-inert (effect-only feed, identical markup)", () => {
    const bare = renderToStaticMarkup(<MarkdownSurface />);
    const fed = renderToStaticMarkup(
      <MarkdownSurface document={{ path: "C:/docs/a.md", markdown: "# hi" }} />,
    );
    expect(fed).toBe(bare);
  });
});
