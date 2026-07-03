import { describe, expect, mock, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type { Layout } from "../session/splitLayout";

// The workspace pulls in the live terminal (xterm), the reused agent app, and
// the native session hook. Stub those seams so this SSR test exercises only the
// per-pane SURFACE PICK — deterministically and without heavy imports. The
// markdown/diff surfaces are the real ones (this lane owns them).
mock.module("./TerminalSurface", () => ({
  TerminalSurface: () => <div data-surface="terminal-live" />,
}));
mock.module("./AgentSessionSurface", () => ({
  AgentSessionSurface: () => <div data-surface="agent-live" />,
}));

let currentLayout: Layout | null = null;
mock.module("../hooks/useSession", () => ({
  useSession: () => ({
    snapshot: null,
    activeLayout: currentLayout,
    split: () => {},
    close: () => {},
    setDivider: () => {},
    setSurfaceKind: () => {},
  }),
}));

const { Workspace } = await import("./Workspace");

function pane(surfaceKind: string | undefined, id: string): Layout {
  return { type: "pane", pane: { panel_ids: [id], surface_kind: surfaceKind } };
}

function split(
  orientation: "horizontal" | "vertical",
  divider: number,
  first: Layout,
  second: Layout,
): Layout {
  return { type: "split", split: { orientation, divider_position: divider, first, second } };
}

/** SSR-render the workspace over a fixed layout. */
function render(layout: Layout): string {
  currentLayout = layout;
  return renderToStaticMarkup(<Workspace />);
}

function count(markup: string, needle: RegExp): number {
  return (markup.match(needle) ?? []).length;
}

describe("Workspace surface pick", () => {
  test("a terminal pane shows the (always-mounted) terminal and no other surface", () => {
    const markup = render(pane(undefined, "t"));
    expect(markup).toContain('data-surface="terminal-live"');
    // Terminal wrapper is visible for a terminal pane.
    expect(markup).toMatch(/display:flex[^"]*"><div data-surface="terminal-live"/);
    expect(markup).not.toContain('data-surface="agent-live"');
    expect(markup).not.toContain("cmux-markdown-surface");
    expect(markup).not.toContain("cmux-diff-surface");
  });

  test("an agent pane mounts the agent surface visible and hides the terminal", () => {
    const markup = render(pane("agent", "a"));
    // Both terminal + agent are mounted (sticky never-unmount), but the terminal
    // wrapper is hidden and the agent wrapper is shown.
    expect(markup).toContain('data-surface="terminal-live"');
    expect(markup).toContain('data-surface="agent-live"');
    expect(markup).toMatch(/display:none[^"]*"><div data-surface="terminal-live"/);
    expect(markup).toMatch(/display:flex[^"]*"><div data-surface="agent-live"/);
    // No markdown/diff surface for an agent pane.
    expect(markup).not.toContain("cmux-markdown-surface");
    expect(markup).not.toContain("cmux-diff-surface");
  });

  test("a markdown pane mounts the markdown iframe on demand (terminal hidden, no agent)", () => {
    const markup = render(pane("markdown", "m"));
    expect(markup).toContain("cmux-markdown-surface");
    expect(markup).toContain('src="cmux-md://localhost/shell.html"');
    // Terminal still mounted but hidden; agent NOT mounted (not a sticky agent pane).
    expect(markup).toMatch(/display:none[^"]*"><div data-surface="terminal-live"/);
    expect(markup).not.toContain('data-surface="agent-live"');
  });

  test("a diff pane mounts the diff surface on demand — placeholder while no token", () => {
    const markup = render(pane("diff", "d"));
    expect(markup).toContain("cmux-diff-surface-placeholder");
    // No live token source yet, so it is the placeholder, not an iframe.
    expect(markup).not.toContain("<iframe");
    expect(markup).not.toContain('data-surface="agent-live"');
  });

  test("each pane in a mixed tree picks its own surface independently", () => {
    const layout = split(
      "horizontal",
      0.5,
      split("vertical", 0.5, pane(undefined, "t"), pane("agent", "a")),
      split("vertical", 0.5, pane("markdown", "m"), pane("diff", "d")),
    );
    const markup = render(layout);
    // Terminal is mounted once per pane (4), agent only for the agent pane (1).
    expect(count(markup, /data-surface="terminal-live"/g)).toBe(4);
    expect(count(markup, /data-surface="agent-live"/g)).toBe(1);
    expect(count(markup, /cmux-markdown-surface/g)).toBe(1);
    expect(count(markup, /cmux-diff-surface-placeholder/g)).toBe(1);
  });
});

describe("Workspace pane controls", () => {
  test("every pane exposes agent / markdown / diff surface toggles", () => {
    const markup = render(pane(undefined, "t"));
    expect(markup).toContain('aria-label="Start agent session"');
    expect(markup).toContain('aria-label="Markdown preview"');
    expect(markup).toContain('aria-label="Diff viewer"');
  });

  test("the active surface's toggle reads as pressed and offers a switch-back", () => {
    const markup = render(pane("markdown", "m"));
    // The markdown toggle is now the "switch to terminal" affordance, pressed.
    expect(markup).toMatch(/aria-label="Switch to terminal" aria-pressed="true"/);
    // A non-active toggle (agent) is not pressed.
    expect(markup).toContain('aria-label="Start agent session" aria-pressed="false"');
  });
});
