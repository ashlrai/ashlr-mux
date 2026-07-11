import { beforeEach, describe, expect, mock, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import * as jsxDevRuntime from "react/jsx-dev-runtime";
import * as jsxRuntime from "react/jsx-runtime";
import type {
  CanvasConfig,
  SessionCanvasPaneSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import { focusedPaneStore } from "../session/focusedPane";
import type { Layout } from "../session/splitLayout";

// The workspace pulls in the live terminal (xterm), the reused agent app, and
// the native session hook. Stub those seams so this SSR test exercises only the
// per-pane SURFACE PICK — deterministically and without heavy imports. The
// markdown/diff surfaces are the real ones (this lane owns them).
const terminalSurfaceProps: Array<Record<string, unknown>> = [];
mock.module("./TerminalSurface", () => ({
  dispatchTerminalCommand: () => {},
  TerminalSurface: (props: Record<string, unknown>) => {
    terminalSurfaceProps.push(props);
    return (
      <div
        data-surface="terminal-live"
        data-active={props.isActive === true ? "true" : "false"}
      />
    );
  },
}));
mock.module("./AgentSessionSurface", () => ({
  AgentSessionSurface: () => <div data-surface="agent-live" />,
}));

let currentLayout: Layout | null = null;
let currentZoomedPanelId: string | undefined;
let currentUnreadPanelIds = new Set<string>();
let currentLayoutMode: string | undefined;
let currentCanvasPanes: SessionCanvasPaneSnapshot[] | undefined;
const openBrowserUrlCalls: Array<[string, string | undefined]> = [];
const focusPanelCalls: string[] = [];
function workspacesForLayout(
  layout: Layout | null,
): readonly SessionWorkspaceSnapshot[] {
  if (layout == null) {
    return [];
  }
  return [
    {
      process_title: "shell",
      current_directory: "C:\\repo",
      layout,
      layout_mode: currentLayoutMode,
      canvas_panes: currentCanvasPanes,
      zoomed_panel_id: currentZoomedPanelId,
      panel_unreads: [...currentUnreadPanelIds].map((panel_id) => ({
        panel_id,
        is_unread: true,
      })),
    },
  ];
}

mock.module("../hooks/useSession", () => ({
  useSession: () => ({
    snapshot: null,
    activeLayout: currentLayout,
    workspaces: workspacesForLayout(currentLayout),
    selectedWorkspaceIndex: 0,
    selectWorkspace: () => {},
    selectWorkspaceSurface: () => {},
    focusPanel: (panelId: string) => focusPanelCalls.push(panelId),
    newWorkspace: () => {},
    newTerminalTab: () => {},
    closeWorkspace: () => {},
    closeWorkspaces: () => {},
    reorderWorkspace: () => {},
    renameWorkspace: () => {},
    setWorkspaceDescription: () => {},
    resetWorkspaceColor: () => {},
    setWorkspaceUnread: () => {},
    renameTab: () => {},
    setPanelPinned: () => {},
    setPanelUnread: () => {},
    setWorkspacePinned: () => {},
    setGroupCollapsed: () => {},
    split: () => {},
    equalizeDividers: () => {},
    toggleSplitZoom: () => {},
    close: () => {},
    setDivider: () => {},
    setSurfaceKind: () => {},
    openMarkdownFile: () => {},
    openFile: () => {},
    openDiffViewer: () => {},
    openBrowserUrl: (panelId: string, url?: string) => {
      openBrowserUrlCalls.push([panelId, url]);
    },
    browserBack: () => {},
    browserForward: () => {},
    clearBrowserHistory: () => {},
    toggleBrowserOmnibar: () => {},
    toggleBrowserFocusMode: () => {},
    toggleBrowserDeveloperTools: () => {},
    showBrowserDeveloperTools: () => {},
    newBrowserWorkspace: () => {},
    reopenClosedBrowserTab: () => {},
    movePanelToNewWorkspace: () => {},
    splitBrowser: () => {},
    setBrowserZoom: () => {},
    setCanvasPaneFrame: () => {},
    applyCanvasAction: () => {},
  }),
}));

// This harness is SSR-only (no DOM), so pointer/focus events cannot be
// DISPATCHED. Instead, record each pane wrapper's props through a pass-through
// jsx-runtime mock (rendering is unchanged) and invoke the captured
// `onPointerDownCapture` / `onFocusCapture` closures directly — the exact
// production handlers a real capture-phase event would run.
const paneWrapperProps: Array<Record<string, unknown>> = [];
const dividerProps: Array<Record<string, unknown>> = [];
function recordPaneWrapper(props: unknown): void {
  if (typeof props === "object" && props !== null && "onPointerDownCapture" in props) {
    paneWrapperProps.push(props as Record<string, unknown>);
  }
  if (
    typeof props === "object" &&
    props !== null &&
    (props as Record<string, unknown>).role === "separator"
  ) {
    dividerProps.push(props as Record<string, unknown>);
  }
}
// Capture the REAL runtime functions before mocking: bun's mock.module can
// rebind live imports, so delegating through the namespace would recurse.
const realJsx = jsxRuntime.jsx;
const realJsxs = jsxRuntime.jsxs;
const realFragment = jsxRuntime.Fragment;
const realJsxDEV = jsxDevRuntime.jsxDEV;
const wrapJsx: typeof realJsx = (type, props, key) => {
  recordPaneWrapper(props);
  return realJsx(type, props, key);
};
const wrapJsxs: typeof realJsxs = (type, props, key) => {
  recordPaneWrapper(props);
  return realJsxs(type, props, key);
};
const wrapJsxDEV: typeof realJsxDEV = (...args) => {
  recordPaneWrapper(args[1]);
  return realJsxDEV(...args);
};
mock.module("react/jsx-runtime", () => ({
  Fragment: realFragment,
  jsx: wrapJsx,
  jsxs: wrapJsxs,
}));
mock.module("react/jsx-dev-runtime", () => ({
  Fragment: realFragment,
  jsxDEV: wrapJsxDEV,
}));

const {
  Workspace,
  browserHistoryNavigationAvailability,
  dispatchNativePanelFlash,
  dispatchNativeSurfaceRefresh,
  dispatchPanelFlash,
  dispatchPanelFlashSequence,
  isSerializableBrowserHistoryUrl,
  terminalPaneIsActive,
} = await import("./Workspace");

function pane(
  surfaceKind: string | undefined,
  id: string,
  extra?: Record<string, unknown>,
): Layout {
  return {
    type: "pane",
    pane: { panel_ids: [id], surface_kind: surfaceKind, ...extra },
  };
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
function render(
  layout: Layout,
  zoomedPanelId?: string,
  unreadPanelIds: readonly string[] = [],
  options: {
    canvasConfig?: CanvasConfig;
    layoutMode?: string;
    canvasPanes?: SessionCanvasPaneSnapshot[];
    openTerminalLinksInCmuxBrowser?: boolean;
  } = {},
): string {
  currentLayout = layout;
  currentZoomedPanelId = zoomedPanelId;
  currentUnreadPanelIds = new Set(unreadPanelIds);
  currentLayoutMode = options.layoutMode;
  currentCanvasPanes = options.canvasPanes;
  return renderToStaticMarkup(
    <Workspace
      canvasConfig={options.canvasConfig}
      openTerminalLinksInCmuxBrowser={options.openTerminalLinksInCmuxBrowser}
    />,
  );
}

function count(markup: string, needle: RegExp): number {
  return (markup.match(needle) ?? []).length;
}

describe("dispatchPanelFlash", () => {
  test("is SSR-safe when no window exists", () => {
    expect(() => dispatchPanelFlash("surface-1")).not.toThrow();
  });

  test("can dispatch a two-pulse flash sequence", () => {
    const previousWindow = globalThis.window;
    const events: string[] = [];
    try {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: {
          dispatchEvent(event: Event) {
            events.push((event as CustomEvent<{ panelId: string }>).detail.panelId);
            return true;
          },
          setTimeout(callback: () => void) {
            callback();
            return 1;
          },
        },
      });

      dispatchPanelFlashSequence("surface-1");

      expect(events).toEqual(["surface-1", "surface-1"]);
    } finally {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: previousWindow,
      });
    }
  });

  test("bridges valid native flash payloads and ignores malformed ones", () => {
    const previousWindow = globalThis.window;
    const events: string[] = [];
    try {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: {
          dispatchEvent(event: Event) {
            events.push((event as CustomEvent<{ panelId: string }>).detail.panelId);
            return true;
          },
        },
      });

      dispatchNativePanelFlash({ panelId: "surface-2" });
      dispatchNativePanelFlash({ panelId: "" });
      dispatchNativePanelFlash({});

      expect(events).toEqual(["surface-2"]);
    } finally {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: previousWindow,
      });
    }
  });
});

describe("dispatchNativeSurfaceRefresh", () => {
  test("dispatches one resize event for mounted surface refits", () => {
    const previousWindow = globalThis.window;
    const events: string[] = [];
    try {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: {
          dispatchEvent(event: Event) {
            events.push(event.type);
            return true;
          },
        },
      });
      dispatchNativeSurfaceRefresh();
      expect(events).toEqual(["resize"]);
    } finally {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: previousWindow,
      });
    }
  });
});

describe("browser history availability", () => {
  test("uses the session-history sanitizer rules", () => {
    expect(isSerializableBrowserHistoryUrl(" https://example.com ")).toBe(true);
    expect(isSerializableBrowserHistoryUrl("about:blank")).toBe(false);
    expect(isSerializableBrowserHistoryUrl("cmux-diff-viewer://tok/index.html")).toBe(false);
    expect(
      isSerializableBrowserHistoryUrl(
        "http://cmux-remote-image.localhost/?url=https%3A%2F%2Fexample.com%2Fa.png",
      ),
    ).toBe(false);
    expect(isSerializableBrowserHistoryUrl("not a url")).toBe(false);

    expect(
      browserHistoryNavigationAvailability(
        ["about:blank", "cmux-diff-viewer://tok/index.html"],
        ["https://forward.example"],
      ),
    ).toEqual({ canGoBack: false, canGoForward: true });
  });
});

describe("Workspace surface pick", () => {
  test("a terminal pane shows the (always-mounted) terminal and no other surface", () => {
    const markup = render(pane(undefined, "t"));
    expect(markup).toContain('data-surface="terminal-live"');
    // Terminal wrapper is visible for a terminal pane.
    expect(markup).toMatch(/display:flex[^"]*"><div data-surface="terminal-live"/);
    expect(markup).not.toContain('data-surface="agent-live"');
    expect(markup).not.toContain("cmux-markdown-surface");
    expect(markup).not.toContain("cmux-file-surface");
    expect(markup).not.toContain("cmux-diff-surface");
    expect(markup).not.toContain("cmux-custom-sidebar-surface");
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
    expect(markup).not.toContain("cmux-file-surface");
    expect(markup).not.toContain("cmux-diff-surface");
  });

  test("a markdown pane mounts the markdown iframe on demand (terminal hidden, no agent)", () => {
    const markup = render(pane("markdown", "m"));
    expect(markup).toContain("cmux-markdown-surface");
    expect(markup).toContain('data-cmux-markdown-panel-id="m"');
    expect(markup).toContain('src="cmux-md://localhost/shell.html?panelId=m"');
    // Terminal still mounted but hidden; agent NOT mounted (not a sticky agent pane).
    expect(markup).toMatch(/display:none[^"]*"><div data-surface="terminal-live"/);
    expect(markup).not.toContain('data-surface="agent-live"');
  });

  test("a file pane mounts the file editor with its stored path", () => {
    const markup = render(
      pane("file", "f", {
        file_path: "C:\\repo\\notes.txt",
      }),
    );
    expect(markup).toContain("cmux-file-surface");
    expect(markup).toContain("notes.txt");
    expect(markup).toContain("C:\\repo\\notes.txt");
    expect(markup).toMatch(/display:none[^"]*"><div data-surface="terminal-live"/);
    expect(markup).not.toContain('data-surface="agent-live"');
  });

  test("a custom sidebar pane mounts the custom sidebar preview with its source path", () => {
    const markup = render(
      pane("custom-sidebar", "s", {
        file_path: "C:\\Users\\User\\.config\\cmux\\sidebars\\review.swift",
      }),
    );
    expect(markup).toContain("cmux-custom-sidebar-surface");
    expect(markup).toContain("review.swift");
    expect(markup).toContain("Custom sidebar preview");
    expect(markup).toMatch(/display:none[^"]*"><div data-surface="terminal-live"/);
    expect(markup).not.toContain('data-surface="agent-live"');
  });

  test("a diff pane mounts the diff surface on demand — placeholder while no token", () => {
    const markup = render(pane("diff", "d"));
    expect(markup).toContain("cmux-diff-surface-placeholder");
    // A malformed/restored diff pane without a token stays guarded.
    expect(markup).not.toContain("<iframe");
    expect(markup).not.toContain('data-surface="agent-live"');
  });

  test("a diff pane with a registered token mounts the diff iframe", () => {
    const markup = render(
      pane("diff", "d", {
        diff_viewer_token: "tok-abcdef0123456789",
        diff_viewer_request_path: "/review/index.html",
      }),
    );
    expect(markup).toContain("cmux-diff-surface");
    expect(markup).toContain(
      'src="cmux-diff-viewer://tok-abcdef0123456789/review/index.html"',
    );
    expect(markup).not.toContain("cmux-diff-surface-placeholder");
  });

  test("a browser pane mounts the browser surface with its stored URL", () => {
    const markup = render(
      pane("browser", "b", {
        browser_url: "https://example.com",
        browser_page_zoom: 1,
      }),
    );
    expect(markup).toContain("cmux-browser-surface");
    expect(markup).toContain('src="https://example.com"');
    expect(markup).toMatch(/display:none[^"]*"><div data-surface="terminal-live"/);
    expect(markup).not.toContain('data-surface="agent-live"');
  });

  test("each pane in a mixed tree picks its own surface independently", () => {
    const layout = split(
      "horizontal",
      0.5,
      split("vertical", 0.5, pane(undefined, "t"), pane("agent", "a")),
      split("vertical", 0.5, pane("file", "f", { file_path: "C:/repo/a.txt" }), pane("browser", "b")),
    );
    const markup = render(layout);
    // Terminal is mounted once per pane (4), agent only for the agent pane (1).
    expect(count(markup, /data-surface="terminal-live"/g)).toBe(4);
    expect(count(markup, /data-surface="agent-live"/g)).toBe(1);
    expect(count(markup, /class="cmux-file-surface"/g)).toBe(1);
    expect(count(markup, /cmux-browser-surface/g)).toBe(1);
  });

  test("a zoomed workspace renders only the zoomed pane full-frame", () => {
    const layout = split(
      "horizontal",
      0.5,
      pane(undefined, "left"),
      pane("browser", "right", { browser_url: "https://example.com" }),
    );
    paneWrapperProps.length = 0;
    const markup = render(layout, "right");
    const style = paneWrapperProps[0]?.style as Record<string, unknown>;
    expect(count(markup, /data-surface="terminal-live"/g)).toBe(1);
    expect(markup).toContain("cmux-browser-surface");
    expect(paneWrapperProps.length).toBe(1);
    expect(style.left).toBe("calc(0% + 0px)");
    expect(style.top).toBe("calc(0% + 0px)");
    expect(style.width).toBe("calc(100% - 0px)");
    expect(style.height).toBe("calc(100% - 0px)");
  });

  test("an unread pane renders the unread indicator", () => {
    const markup = render(pane(undefined, "t"), undefined, ["t"]);
    expect(markup).toContain("cmux-pane-unread-indicator");
  });

  test("canvas layout mode uses persisted canvas pane frames and hides split dividers", () => {
    paneWrapperProps.length = 0;
    dividerProps.length = 0;
    const layout = split(
      "horizontal",
      0.5,
      pane(undefined, "left"),
      pane(undefined, "right"),
    );
    const markup = render(layout, undefined, [], {
      layoutMode: "canvas",
      canvasPanes: [
        { panel_id: "left", x: 0, y: 0, width: 600, height: 800 },
        { panel_id: "right", x: 600, y: 200, width: 600, height: 400 },
      ],
    });
    const rightStyle = paneWrapperProps[1]?.style as Record<string, unknown>;
    expect(markup).toContain("cmux-workspace-portal--canvas");
    expect(markup).toContain("cmux-canvas-toolbar");
    expect(markup).toContain("Overview");
    expect(markup).toContain("Reveal");
    expect(markup).toContain("Tidy");
    expect(count(markup, /cmux-canvas-pane-grip/g)).toBe(2);
    expect(count(markup, /cmux-canvas-pane-resize /g)).toBe(16);
    expect(markup).not.toContain('role="separator"');
    expect(dividerProps.length).toBe(0);
    expect(rightStyle.left).toBe("600px");
    expect(rightStyle.top).toBe("200px");
    expect(rightStyle.width).toBe("600px");
    expect(rightStyle.height).toBe("400px");
  });
});

describe("Workspace focused-pane tracking", () => {
  // The store is a module singleton shared with the app — reset between cases.
  beforeEach(() => {
    focusedPaneStore.clear();
    paneWrapperProps.length = 0;
    terminalSurfaceProps.length = 0;
    openBrowserUrlCalls.length = 0;
    focusPanelCalls.length = 0;
  });

  test("pointer-down (capture) on a pane wrapper focuses that pane", () => {
    render(split("horizontal", 0.5, pane(undefined, "a"), pane(undefined, "b")));
    // One recorded wrapper per pane, in tree (render) order.
    expect(paneWrapperProps.length).toBe(2);
    (paneWrapperProps[1]?.onPointerDownCapture as () => void)();
    expect(focusedPaneStore.get()).toBe("b");
    expect(focusPanelCalls).toEqual(["b"]);
  });

  test("focus (capture) on a pane wrapper focuses that pane", () => {
    render(split("horizontal", 0.5, pane(undefined, "a"), pane(undefined, "b")));
    (paneWrapperProps[0]?.onFocusCapture as () => void)();
    expect(focusedPaneStore.get()).toBe("a");
    expect(focusPanelCalls).toEqual(["a"]);
  });

  test("focus restoration activates only the visible focused terminal", () => {
    expect(terminalPaneIsActive("terminal", "right", "right")).toBe(true);
    expect(terminalPaneIsActive("terminal", "left", "right")).toBe(false);
    expect(terminalPaneIsActive("browser", "left", "left")).toBe(false);
  });

  test("cmd-click terminal links are routed to the pane browser surface", () => {
    render(pane(undefined, "term"));

    const openLink = terminalSurfaceProps[0]?.onOpenLinkInBrowser;
    expect(typeof openLink).toBe("function");
    (openLink as (url: string) => void)("https://example.test/docs");

    expect(openBrowserUrlCalls).toEqual([
      ["term", "https://example.test/docs"],
    ]);
  });

  test("terminal browser link routing follows the browser setting", () => {
    render(pane(undefined, "term"), undefined, [], {
      openTerminalLinksInCmuxBrowser: false,
    });

    expect(terminalSurfaceProps[0]?.onOpenLinkInBrowser).toBeUndefined();
  });
});

describe("Workspace divider controls", () => {
  beforeEach(() => {
    dividerProps.length = 0;
  });

  test("flat-portal dividers are keyboard-focusable and expose their ratio", () => {
    const markup = render(
      split("horizontal", 0.6, pane(undefined, "left"), pane(undefined, "right")),
    );
    expect(markup).toContain('role="separator"');
    expect(markup).toContain('aria-orientation="vertical"');
    expect(markup).toContain('aria-valuenow="60"');
    expect(markup).toContain('aria-valuemin="10"');
    expect(markup).toContain('aria-valuemax="90"');
    expect(markup).toContain('tabindex="0"');
    expect(dividerProps.length).toBe(1);
    expect(typeof dividerProps[0]?.onKeyDown).toBe("function");
  });

  test("vertical flat-portal dividers expose horizontal separator orientation", () => {
    const markup = render(
      split("vertical", 0.4, pane(undefined, "top"), pane(undefined, "bottom")),
    );
    expect(markup).toContain('aria-orientation="horizontal"');
    expect(markup).toContain('aria-valuenow="40"');
  });
});

describe("Workspace pane controls", () => {
  test("every pane exposes agent / markdown / browser / diff surface toggles", () => {
    const markup = render(pane(undefined, "t"));
    expect(markup).toContain('aria-label="Start agent session"');
    expect(markup).toContain('aria-label="Markdown preview"');
    expect(markup).toContain('aria-label="Browser"');
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
