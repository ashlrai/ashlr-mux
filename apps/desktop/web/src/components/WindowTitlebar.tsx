import type { CSSProperties } from "react";

import { Icon } from "@cmux/webviews/src/icons";

import { useWindowChrome } from "../hooks/useWindowChrome";
import {
  HEADER_CHROME_CONTROL_METRICS,
  WINDOW_CHROME_METRICS,
  windowsCaptionControlReservedInlineSize,
  WINDOWS_CAPTION_CONTROL_METRICS,
} from "../window/chromeMetrics";

export interface WindowTitlebarProps {
  sidebarCollapsed: boolean;
  fileExplorerOpen?: boolean;
  minimalMode?: boolean;
  onToggleSidebar: () => void;
  onToggleFileExplorer?: () => void;
  onOpenShortcutHelp: () => void;
  onOpenSettings: () => void;
}

export function WindowTitlebar({
  sidebarCollapsed,
  fileExplorerOpen = false,
  minimalMode = false,
  onToggleSidebar,
  onToggleFileExplorer,
  onOpenShortcutHelp,
  onOpenSettings,
}: WindowTitlebarProps): React.JSX.Element {
  const { state, minimize, toggleMaximize, close } = useWindowChrome();
  const style = {
    "--cmux-titlebar-height": `${WINDOW_CHROME_METRICS.appTitlebarHeight}px`,
    "--cmux-titlebar-button-size": `${HEADER_CHROME_CONTROL_METRICS.buttonSize}px`,
    "--cmux-titlebar-icon-size": `${HEADER_CHROME_CONTROL_METRICS.iconSize}px`,
    "--cmux-titlebar-radius": `${HEADER_CHROME_CONTROL_METRICS.cornerRadius}px`,
    "--cmux-caption-button-width": `${WINDOWS_CAPTION_CONTROL_METRICS.buttonWidth}px`,
    "--cmux-caption-controls-inline-size": `${windowsCaptionControlReservedInlineSize()}px`,
  } as CSSProperties;

  return (
    <header
      className={
        minimalMode ? "cmux-titlebar cmux-titlebar--minimal" : "cmux-titlebar"
      }
      data-minimal-mode={minimalMode ? "true" : "false"}
      style={style}
    >
      <div className="cmux-titlebar-leading" data-tauri-drag-region="false">
        <button
          type="button"
          className="cmux-sidebar-toggle"
          title={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
          aria-label={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
          aria-pressed={!sidebarCollapsed}
          onClick={onToggleSidebar}
        >
          <Icon name="bars" />
        </button>
        <div className="cmux-titlebar-brand">
          <span className="cmux-icon cmux-titlebar-brand-icon">
            <Icon name="classic" />
          </span>
          <span className="cmux-titlebar-brand-name">cmux</span>
        </div>
      </div>

      <div className="cmux-titlebar-drag-band" data-tauri-drag-region="deep">
        <span className="cmux-titlebar-window-title">{state.title}</span>
      </div>

      <div className="cmux-titlebar-trailing" data-tauri-drag-region="false">
        <button
          type="button"
          className="cmux-header-button"
          aria-label={fileExplorerOpen ? "Hide file explorer" : "Show file explorer"}
          aria-pressed={fileExplorerOpen}
          onClick={onToggleFileExplorer}
        >
          Files
        </button>
        <button
          type="button"
          className="cmux-header-button"
          aria-label="Open settings"
          onClick={onOpenSettings}
        >
          Settings
        </button>
        <button
          type="button"
          className="cmux-header-button cmux-header-button--icon"
          title="Learn shortcuts"
          aria-label="Learn shortcuts"
          onClick={onOpenShortcutHelp}
        >
          ?
        </button>
        <div className="cmux-caption-buttons">
          <button
            type="button"
            className="cmux-caption-button"
            aria-label="Minimize window"
            title="Minimize"
            onClick={minimize}
          >
            <span className="cmux-caption-glyph cmux-caption-glyph--minimize" />
          </button>
          <button
            type="button"
            className="cmux-caption-button"
            aria-label={state.isMaximized ? "Restore window" : "Maximize window"}
            title={state.isMaximized ? "Restore" : "Maximize"}
            onClick={toggleMaximize}
          >
            <span
              className={
                state.isMaximized
                  ? "cmux-caption-glyph cmux-caption-glyph--restore"
                  : "cmux-caption-glyph cmux-caption-glyph--maximize"
              }
            />
          </button>
          <button
            type="button"
            className="cmux-caption-button cmux-caption-button--close"
            aria-label="Close window"
            title="Close"
            onClick={close}
          >
            <span className="cmux-caption-glyph cmux-caption-glyph--close" />
          </button>
        </div>
      </div>
    </header>
  );
}
