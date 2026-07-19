export interface PaneDiffSession {
  token: string;
  requestPath?: string;
}

export interface PaneBrowserState {
  url?: string;
  proxyUrl?: string;
  zoom?: number;
  canGoBack: boolean;
  canGoForward: boolean;
  omnibarVisible: boolean;
  focusModeActive: boolean;
  developerToolsVisible: boolean;
  developerToolsPanel?: string;
}

export interface PaneTerminalStartup {
  cwd?: string;
  initialCommand?: string;
  initialInput?: string;
  environment?: Record<string, string>;
}
