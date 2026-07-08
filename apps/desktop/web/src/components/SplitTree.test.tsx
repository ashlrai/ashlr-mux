import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type { Layout, Pane, SplitPath } from "../session/splitLayout";
import { SplitTree } from "./SplitTree";

function pane(id: string): Layout {
  return { type: "pane", pane: { panel_ids: [id] } };
}

const renderPane = (p: Pane, path: SplitPath): React.JSX.Element => (
  <span data-pane={p.panel_ids[0]} data-depth={path.length} />
);

describe("SplitTree", () => {
  test("renders a single pane with no dividers", () => {
    const markup = renderToStaticMarkup(<SplitTree layout={pane("solo")} renderPane={renderPane} />);
    expect(markup).toContain('data-pane="solo"');
    expect(markup).not.toContain("cmux-split-divider");
  });

  test("renders both children and a divider for a split", () => {
    const layout: Layout = {
      type: "split",
      split: { orientation: "horizontal", divider_position: 0.5, first: pane("left"), second: pane("right") },
    };
    const markup = renderToStaticMarkup(<SplitTree layout={layout} renderPane={renderPane} />);
    expect(markup).toContain('data-pane="left"');
    expect(markup).toContain('data-pane="right"');
    expect(markup).toContain("cmux-split-divider");
    expect(markup).toContain('aria-orientation="vertical"'); // horizontal split → vertical divider
  });

  test("the divider is keyboard-focusable and exposes its ratio to AT", () => {
    const layout: Layout = {
      type: "split",
      split: { orientation: "horizontal", divider_position: 0.6, first: pane("left"), second: pane("right") },
    };
    const markup = renderToStaticMarkup(<SplitTree layout={layout} renderPane={renderPane} />);
    expect(markup).toContain('tabindex="0"');
    expect(markup).toContain('aria-valuenow="60"');
    expect(markup).toContain('aria-valuemin="10"');
    expect(markup).toContain('aria-valuemax="90"');
  });

  test("a single pane exposes no divider focus/value attributes", () => {
    const markup = renderToStaticMarkup(<SplitTree layout={pane("solo")} renderPane={renderPane} />);
    expect(markup).not.toContain('tabindex="0"');
    expect(markup).not.toContain("aria-valuenow");
    expect(markup).not.toContain("aria-valuemin");
    expect(markup).not.toContain("aria-valuemax");
  });

  test("walks a nested tree and passes each pane its path depth", () => {
    const layout: Layout = {
      type: "split",
      split: {
        orientation: "vertical",
        divider_position: 0.6,
        first: pane("top"),
        second: {
          type: "split",
          split: { orientation: "horizontal", divider_position: 0.5, first: pane("bl"), second: pane("br") },
        },
      },
    };
    const markup = renderToStaticMarkup(<SplitTree layout={layout} renderPane={renderPane} />);
    // top is one level deep; bl/br are two levels deep.
    expect(markup).toContain('data-pane="top"');
    expect(markup).toMatch(/data-pane="top"[^>]*data-depth="1"/);
    expect(markup).toMatch(/data-pane="bl"[^>]*data-depth="2"/);
    expect(markup).toMatch(/data-pane="br"[^>]*data-depth="2"/);
  });
});
