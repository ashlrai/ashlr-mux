import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { diffSurfaceUrl } from "../session/surfaceUrl";
import { DiffSurface } from "./DiffSurface";

describe("DiffSurface", () => {
  test("renders a placeholder (no iframe) when no token is supplied", () => {
    const markup = renderToStaticMarkup(<DiffSurface panelId="surface-1" />);
    expect(markup).toContain("cmux-diff-surface-placeholder");
    expect(markup).toContain("No diff session.");
    expect(markup).not.toContain("Start diff session");
    expect(markup).not.toContain("<iframe");
  });

  test("renders a recovery action when a starter-session callback is available", () => {
    const markup = renderToStaticMarkup(
      <DiffSurface panelId="surface-1" onCreateSession={() => {}} />,
    );
    expect(markup).toContain("cmux-diff-surface-placeholder-action");
    expect(markup).toContain("Start diff session");
  });

  test("treats an empty-string token as absent (placeholder, no iframe)", () => {
    const markup = renderToStaticMarkup(<DiffSurface panelId="surface-1" token="" />);
    expect(markup).toContain("cmux-diff-surface-placeholder");
    expect(markup).not.toContain("<iframe");
  });

  test("renders a titled, sandboxed iframe at the diff-viewer URL when given a token", () => {
    const token = "tok-abcdef0123456789";
    const markup = renderToStaticMarkup(<DiffSurface panelId="surface-1" token={token} />);
    expect(markup).toContain(`src="${diffSurfaceUrl(token)}"`);
    expect(markup).toContain('class="cmux-diff-surface"');
    expect(markup).toContain('title="Diff viewer"');
    expect(markup).toContain('sandbox="allow-scripts allow-same-origin"');
    expect(markup).not.toContain("cmux-diff-surface-placeholder");
  });

  test("uses the stored request path when the pane is bound to a non-default diff entry", () => {
    const token = "tok-abcdef0123456789";
    const markup = renderToStaticMarkup(
      <DiffSurface
        panelId="surface-1"
        token={token}
        requestPath="/review/file.patch.html"
      />,
    );
    expect(markup).toContain(
      `src="${diffSurfaceUrl(token, "/review/file.patch.html")}"`,
    );
  });
});
