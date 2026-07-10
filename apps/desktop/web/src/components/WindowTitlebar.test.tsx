import { describe, expect, mock, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

let currentState = {
  isMaximized: false,
  title: "Workspace Alpha",
};

mock.module("../hooks/useWindowChrome", () => ({
  useWindowChrome: () => ({
    state: currentState,
    minimize: () => {},
    toggleMaximize: () => {},
    close: () => {},
  }),
}));

const { WindowTitlebar } = await import("./WindowTitlebar");

describe("WindowTitlebar", () => {
  test("renders the native title, drag band, and caption controls", () => {
    currentState = { isMaximized: false, title: "Workspace Alpha" };
    const markup = renderToStaticMarkup(
      <WindowTitlebar
        sidebarCollapsed={false}
        onToggleSidebar={() => {}}
        onToggleFileExplorer={() => {}}
        onOpenShortcutHelp={() => {}}
        onOpenSettings={() => {}}
      />,
    );

    expect(markup).toContain("Workspace Alpha");
    expect(markup).toContain('data-tauri-drag-region="deep"');
    expect(markup).toContain('aria-label="Minimize window"');
    expect(markup).toContain('aria-label="Maximize window"');
    expect(markup).toContain('aria-label="Close window"');
    expect(markup).toContain('aria-label="Hide sidebar"');
    expect(markup).toContain('aria-label="Show file explorer"');
    expect(markup).toContain('aria-label="Learn shortcuts"');
    expect(markup).toContain('title="Learn shortcuts"');
  });

  test("maximized state flips the caption button label to restore", () => {
    currentState = { isMaximized: true, title: "Workspace Beta" };
    const markup = renderToStaticMarkup(
      <WindowTitlebar
        sidebarCollapsed={true}
        fileExplorerOpen={true}
        onToggleSidebar={() => {}}
        onToggleFileExplorer={() => {}}
        onOpenShortcutHelp={() => {}}
        onOpenSettings={() => {}}
      />,
    );

    expect(markup).toContain('aria-label="Restore window"');
    expect(markup).toContain('aria-label="Show sidebar"');
    expect(markup).toContain('aria-label="Hide file explorer"');
  });

  test("minimal mode marks the titlebar for hover/focus reveal", () => {
    currentState = { isMaximized: false, title: "Minimal Workspace" };
    const markup = renderToStaticMarkup(
      <WindowTitlebar
        minimalMode
        sidebarCollapsed={false}
        onToggleSidebar={() => {}}
        onToggleFileExplorer={() => {}}
        onOpenShortcutHelp={() => {}}
        onOpenSettings={() => {}}
      />,
    );

    expect(markup).toContain("cmux-titlebar--minimal");
    expect(markup).toContain('data-minimal-mode="true"');
    expect(markup).toContain("--cmux-caption-controls-inline-size:138px");
    expect(markup).toContain("Minimal Workspace");
  });

  test("renders shortcut help beside settings", () => {
    currentState = { isMaximized: false, title: "Shortcut Help" };
    const markup = renderToStaticMarkup(
      <WindowTitlebar
        sidebarCollapsed={false}
        onToggleSidebar={() => {}}
        onToggleFileExplorer={() => {}}
        onOpenShortcutHelp={() => {}}
        onOpenSettings={() => {}}
      />,
    );

    expect(markup.indexOf('aria-label="Open settings"')).toBeLessThan(
      markup.indexOf('aria-label="Learn shortcuts"'),
    );
    expect(markup.indexOf('aria-label="Learn shortcuts"')).toBeLessThan(
      markup.indexOf('aria-label="Minimize window"'),
    );
    expect(markup).toContain("cmux-header-button--icon");
  });
});
