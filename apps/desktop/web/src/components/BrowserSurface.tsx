import { useEffect, useMemo, useRef, useState } from "react";

import { callNative, listenNative } from "../tauri-bridge";

export type BrowserCommand =
  | "back"
  | "forward"
  | "reload"
  | "focusAddressBar"
  | "openDefault"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset";

export type BrowserDeveloperToolsPanel =
  | "inspector"
  | "console"
  | "react"
  | "network";

const BROWSER_COMMAND_EVENT = "cmux:browser-command";
const BROWSER_NETWORK_CLEARED_EVENT = "cmux:browser-network-cleared";
const DEFAULT_BROWSER_URL = "about:blank";

export interface BrowserCommandDetail {
  panelId: string;
  command: BrowserCommand;
}

export interface BrowserNetworkClearedDetail {
  panelId: string;
}

type BrowserWebviewBounds = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type BrowserWebviewNavigatedPayload = {
  panelId: string;
  url: string;
};

export type BrowserNetworkRecord = {
  id: string;
  panelId: string;
  url: string;
  method: string;
  requestHeaders: Record<string, string>;
  requestBody?: string | null;
  requestBodyPreviewKind?: string | null;
  requestBodySize: number;
  requestBodyTruncated: boolean;
  responseStatus?: number | null;
  responseHeaders: Record<string, string>;
  responseBody?: string | null;
  responseBodyPreviewKind?: string | null;
  responseBodySize: number;
  responseBodyTruncated: boolean;
  startedAtMs: number;
  completedAtMs?: number | null;
  durationMs?: number | null;
  source: string;
  transport: string;
  proxyAttribution?: string | null;
  note?: string | null;
};

export type BrowserNetworkObserverSummary = {
  source: string;
  capturesUrl: boolean;
  capturesMethod: boolean;
  capturesRequestHeaders: boolean;
  capturesRequestBody: boolean;
  capturesResponseStatus: boolean;
  capturesResponseHeaders: boolean;
  capturesResponseBody: boolean;
  capturesTiming: boolean;
  supportsFilters: boolean;
  maxRecordsPerPanel: number;
  bodyCaptureLimitBytes?: number;
  proxyAttributionMode: string;
  note: string;
};

export type BrowserNetworkRequestsReply = {
  panelId: string;
  requests: BrowserNetworkRecord[];
  observer: BrowserNetworkObserverSummary;
  totalCount: number;
  returnedCount: number;
  filteredCount: number;
};

export type BrowserNetworkClearReply = {
  panelId: string;
  clearedCount: number;
};

export interface BrowserSurfaceProps {
  panelId: string;
  url?: string | null;
  proxyUrl?: string | null;
  zoom?: number | null;
  canGoBack?: boolean;
  canGoForward?: boolean;
  omnibarVisible?: boolean;
  focusModeActive?: boolean;
  developerToolsVisible?: boolean;
  developerToolsPanel?: BrowserDeveloperToolsPanel | string | null;
  showImportHint?: boolean;
  onBack?: () => void;
  onForward?: () => void;
  onOpenImportHint?: () => void;
  onOpenImportHintSettings?: () => void;
  onDismissImportHint?: () => void;
  onToggleOmnibar?: () => void;
  onToggleFocusMode?: () => void;
  onToggleDeveloperTools?: () => void;
  onShowDeveloperToolsPanel?: (panel: BrowserDeveloperToolsPanel) => void;
  onClearHistory?: () => void;
  onNavigate?: (url: string) => void;
  onZoomChange?: (zoom: number) => void;
  networkPreview?: BrowserNetworkRequestsReply | null;
}

export function dispatchBrowserCommand(
  panelId: string,
  command: BrowserCommand,
): void {
  if (typeof window === "undefined") {
    return;
  }
  window.dispatchEvent(
    new CustomEvent<BrowserCommandDetail>(BROWSER_COMMAND_EVENT, {
      detail: { panelId, command },
    }),
  );
}

export function dispatchBrowserNetworkCleared(panelId: string): void {
  if (typeof window === "undefined") {
    return;
  }
  window.dispatchEvent(
    new CustomEvent<BrowserNetworkClearedDetail>(BROWSER_NETWORK_CLEARED_EVENT, {
      detail: { panelId },
    }),
  );
}

function clampZoom(value: number): number {
  if (!Number.isFinite(value)) {
    return 1;
  }
  return Math.min(3, Math.max(0.25, value));
}

function normalizeBrowserInput(input: string): string {
  const trimmed = input.trim();
  if (trimmed === "") {
    return DEFAULT_BROWSER_URL;
  }
  if (
    trimmed === "about:blank" ||
    /^[a-z][a-z0-9+.-]*:/i.test(trimmed)
  ) {
    return trimmed;
  }
  if (
    trimmed.startsWith("localhost") ||
    trimmed.startsWith("127.0.0.1") ||
    trimmed.startsWith("[::1]") ||
    trimmed.includes(".")
  ) {
    return `https://${trimmed}`;
  }
  return `https://duckduckgo.com/?q=${encodeURIComponent(trimmed)}`;
}

function canUseNativeBrowserWebview(): boolean {
  return typeof window !== "undefined" && window.__TAURI__?.core?.invoke !== undefined;
}

export function browserNativeWebviewSyncPlan(
  attached: boolean,
  currentProxyUrl: string | undefined,
  nextProxyUrl: string | undefined,
): {
  closeFirst: boolean;
  command: "browser_attach_webview" | "browser_update_webview";
} {
  if (attached && currentProxyUrl !== nextProxyUrl) {
    return { closeFirst: true, command: "browser_attach_webview" };
  }
  return {
    closeFirst: false,
    command: attached ? "browser_update_webview" : "browser_attach_webview",
  };
}

export function browserNetworkRequestParams(
  panelId: string,
  urlFilter: string,
  methodFilter: string,
  limit: number | string = 50,
  sinceIdFilter = "",
): {
  panelId: string;
  limit: number;
  urlContains?: string;
  method?: string;
  sinceId?: string;
} {
  const params: {
    panelId: string;
    limit: number;
    urlContains?: string;
    method?: string;
    sinceId?: string;
  } = { panelId, limit: browserNetworkLimitValue(limit) };
  const urlContains = urlFilter.trim();
  const method = methodFilter.trim().toUpperCase();
  const sinceId = sinceIdFilter.trim();
  if (urlContains) {
    params.urlContains = urlContains;
  }
  if (method) {
    params.method = method;
  }
  if (sinceId) {
    params.sinceId = sinceId;
  }
  return params;
}

export function browserNetworkLimitValue(limit: number | string): number {
  const parsed =
    typeof limit === "number" ? limit : Number.parseInt(limit.trim(), 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return 50;
  }
  return Math.min(200, Math.floor(parsed));
}

export function browserNetworkActiveFilters(
  urlFilter: string,
  methodFilter: string,
  sinceIdFilter = "",
  limit: number | string = 50,
): string[] {
  const filters: string[] = [];
  const urlContains = urlFilter.trim();
  const method = methodFilter.trim().toUpperCase();
  const sinceId = sinceIdFilter.trim();
  const normalizedLimit = browserNetworkLimitValue(limit);
  if (urlContains) {
    filters.push(`URL: ${urlContains}`);
  }
  if (method) {
    filters.push(`Method: ${method}`);
  }
  if (sinceId) {
    filters.push(`After: ${sinceId}`);
  }
  if (normalizedLimit !== 50) {
    filters.push(`Limit: ${normalizedLimit}`);
  }
  return filters;
}

export function browserNetworkEmptyMessage(hasActiveFilters: boolean): string {
  return hasActiveFilters
    ? "No network requests match the active filters."
    : "No network requests recorded for this browser pane yet.";
}

function networkBodyLabel(record: BrowserNetworkRecord): string {
  const request = `${record.requestBodySize}B req${
    record.requestBodyTruncated ? "+" : ""
  }`;
  const response = `${record.responseBodySize}B res${
    record.responseBodyTruncated ? "+" : ""
  }`;
  return `${request} / ${response}`;
}

function networkTimingLabel(record: BrowserNetworkRecord): string {
  if (typeof record.durationMs === "number") {
    return `${record.durationMs}ms`;
  }
  if (typeof record.completedAtMs === "number") {
    return `${Math.max(0, record.completedAtMs - record.startedAtMs)}ms`;
  }
  return "pending";
}

function networkTimestampLabel(value: number | null | undefined): string {
  return typeof value === "number" ? `${value}ms` : "pending";
}

function networkPreviewKindLabel(value: string | null | undefined): string {
  return value?.trim() || "unavailable";
}

function networkBooleanLabel(value: boolean): string {
  return value ? "yes" : "no";
}

function headerCount(headers: Record<string, string>): number {
  return Object.keys(headers).length;
}

function sortedHeaderEntries(
  headers: Record<string, string>,
): Array<[string, string]> {
  return Object.entries(headers).sort(([left], [right]) =>
    left.localeCompare(right),
  );
}

function networkBodyPreview(
  body: string | null | undefined,
  size: number,
  truncated: boolean,
  previewKind?: string | null,
): string {
  const normalizedKind = previewKind ?? (body == null ? "unavailable" : "text");
  if (body == null) {
    return normalizedKind === "empty" || size === 0
      ? "(empty)"
      : `Body unavailable (${size}B captured)`;
  }
  if (body.length === 0) {
    return "(empty)";
  }
  const kindLabel =
    normalizedKind === "binary"
      ? "Binary preview"
      : normalizedKind === "text"
        ? "Text preview"
        : normalizedKind === "unavailable"
          ? "Unavailable preview"
          : `${normalizedKind} preview`;
  const preview = body.length > 2048 ? `${body.slice(0, 2048)}...` : body;
  return truncated
    ? `${kindLabel}\n${preview}\n[truncated at ${body.length} chars]`
    : `${kindLabel}\n${preview}`;
}

function measuredBounds(element: HTMLElement): BrowserWebviewBounds {
  const rect = element.getBoundingClientRect();
  const surface = element.closest<HTMLElement>(".cmux-browser-surface");
  const toolbar = surface?.querySelector<HTMLElement>(".cmux-browser-toolbar");
  const devtools = surface?.querySelector<HTMLElement>(".cmux-browser-devtools");
  const toolbarBottom = toolbar?.getBoundingClientRect().bottom;
  const devtoolsTop = devtools?.getBoundingClientRect().top;
  const top = Math.max(rect.top, toolbarBottom ?? rect.top);
  const bottom = Math.min(rect.bottom, devtoolsTop ?? rect.bottom);
  return {
    x: rect.left,
    y: top,
    width: rect.width,
    height: Math.max(1, bottom - top),
  };
}

export function BrowserSurface({
  panelId,
  url,
  proxyUrl,
  zoom,
  canGoBack = false,
  canGoForward = false,
  omnibarVisible = true,
  focusModeActive = false,
  developerToolsVisible = false,
  developerToolsPanel = "inspector",
  showImportHint = false,
  onBack,
  onForward,
  onOpenImportHint,
  onOpenImportHintSettings,
  onDismissImportHint,
  onToggleOmnibar,
  onToggleFocusMode,
  onToggleDeveloperTools,
  onShowDeveloperToolsPanel,
  onClearHistory,
  onNavigate,
  onZoomChange,
  networkPreview,
}: BrowserSurfaceProps): React.JSX.Element {
  const currentUrl = url?.trim() || DEFAULT_BROWSER_URL;
  const currentZoom = clampZoom(zoom ?? 1);
  const [draft, setDraft] = useState(currentUrl);
  const [frameKey, setFrameKey] = useState(0);
  const [networkReply, setNetworkReply] =
    useState<BrowserNetworkRequestsReply | null>(networkPreview ?? null);
  const [networkLoading, setNetworkLoading] = useState(false);
  const [networkError, setNetworkError] = useState<string | null>(null);
  const [networkRefreshNonce, setNetworkRefreshNonce] = useState(0);
  const [networkUrlFilter, setNetworkUrlFilter] = useState("");
  const [networkMethodFilter, setNetworkMethodFilter] = useState("");
  const [networkSinceIdFilter, setNetworkSinceIdFilter] = useState("");
  const [networkLimitInput, setNetworkLimitInput] = useState("50");
  const nativeSlotRef = useRef<HTMLDivElement | null>(null);
  const nativeAttachedRef = useRef(false);
  const nativeProxyUrlRef = useRef<string | undefined>(undefined);

  useEffect(() => {
    setDraft(currentUrl);
  }, [currentUrl]);

  const isBlank = currentUrl === DEFAULT_BROWSER_URL;
  const scaledFrameStyle = useMemo<React.CSSProperties>(
    () => ({
      width: `${100 / currentZoom}%`,
      height: `${100 / currentZoom}%`,
      transform: `scale(${currentZoom})`,
      transformOrigin: "0 0",
      border: "none",
      background: "white",
    }),
    [currentZoom],
  );

  const navigate = (next: string): void => {
    const normalized = normalizeBrowserInput(next);
    onNavigate?.(normalized);
  };
  const clearNetworkRequests = (): void => {
    setNetworkLoading(true);
    setNetworkError(null);
    void callNative<BrowserNetworkClearReply>("browser_clear_network_requests", {
      panelId,
    })
      .then(() => {
        dispatchBrowserNetworkCleared(panelId);
      })
      .catch((error) => {
        setNetworkError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => setNetworkLoading(false));
  };
  const useNativeWebview = canUseNativeBrowserWebview();
  const normalizedDevToolsPanel =
    developerToolsPanel === "console" ||
    developerToolsPanel === "react" ||
    developerToolsPanel === "network"
      ? developerToolsPanel
      : "inspector";
  const networkActiveFilters = browserNetworkActiveFilters(
    networkUrlFilter,
    networkMethodFilter,
    networkSinceIdFilter,
    networkLimitInput,
  );
  const hasNetworkActiveFilters = networkActiveFilters.length > 0;

  useEffect(() => {
    if (networkPreview !== undefined) {
      setNetworkReply(networkPreview);
    }
  }, [networkPreview]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }
    const onCleared = (event: Event): void => {
      const detail = (event as CustomEvent<BrowserNetworkClearedDetail>).detail;
      if (detail?.panelId !== panelId) {
        return;
      }
      setNetworkReply((reply) =>
        reply
          ? {
              ...reply,
              requests: [],
              totalCount: 0,
              returnedCount: 0,
              filteredCount: 0,
            }
          : reply,
      );
      setNetworkRefreshNonce((value) => value + 1);
    };
    window.addEventListener(BROWSER_NETWORK_CLEARED_EVENT, onCleared);
    return () => window.removeEventListener(BROWSER_NETWORK_CLEARED_EVENT, onCleared);
  }, [panelId]);

  useEffect(() => {
    if (!developerToolsVisible || normalizedDevToolsPanel !== "network") {
      return;
    }
    let disposed = false;
    setNetworkLoading(true);
    setNetworkError(null);
    void callNative<BrowserNetworkRequestsReply>(
      "browser_network_requests",
      browserNetworkRequestParams(
        panelId,
        networkUrlFilter,
        networkMethodFilter,
        networkLimitInput,
        networkSinceIdFilter,
      ),
    )
      .then((reply) => {
        if (!disposed) {
          setNetworkReply(reply);
        }
      })
      .catch((error) => {
        if (!disposed) {
          setNetworkError(error instanceof Error ? error.message : String(error));
        }
      })
      .finally(() => {
        if (!disposed) {
          setNetworkLoading(false);
        }
      });
    return () => {
      disposed = true;
    };
  }, [
    developerToolsVisible,
    networkLimitInput,
    networkMethodFilter,
    networkRefreshNonce,
    networkSinceIdFilter,
    networkUrlFilter,
    normalizedDevToolsPanel,
    panelId,
  ]);

  useEffect(() => {
    if (!useNativeWebview) {
      return;
    }
    return () => {
      if (!nativeAttachedRef.current) {
        return;
      }
      nativeAttachedRef.current = false;
      nativeProxyUrlRef.current = undefined;
      void callNative("browser_close_webview", { panelId }).catch(() => {});
    };
  }, [panelId, useNativeWebview]);

  useEffect(() => {
    if (!useNativeWebview || !isBlank || !nativeAttachedRef.current) {
      return;
    }
    nativeAttachedRef.current = false;
    nativeProxyUrlRef.current = undefined;
    void callNative("browser_close_webview", { panelId }).catch(() => {});
  }, [isBlank, panelId, useNativeWebview]);

  useEffect(() => {
    if (!useNativeWebview) {
      return;
    }
    let cancelled = false;
    const slot = nativeSlotRef.current;
    if (!slot || (!nativeAttachedRef.current && isBlank)) {
      return;
    }

    const syncNativeWebview = (): void => {
      if (cancelled || !nativeSlotRef.current) {
        return;
      }
      const nextProxyUrl = proxyUrl?.trim() || undefined;
      const plan = browserNativeWebviewSyncPlan(
        nativeAttachedRef.current,
        nativeProxyUrlRef.current,
        nextProxyUrl,
      );
      void (async () => {
        if (plan.closeFirst) {
          await callNative("browser_close_webview", { panelId }).catch(() => {});
          nativeAttachedRef.current = false;
          nativeProxyUrlRef.current = undefined;
        }
        if (cancelled || !nativeSlotRef.current) {
          return;
        }
        await callNative(plan.command, {
          panelId,
          url: currentUrl,
          proxyUrl: nextProxyUrl,
          bounds: measuredBounds(nativeSlotRef.current),
          visible: !isBlank,
          zoom: currentZoom,
        });
        nativeAttachedRef.current = true;
        nativeProxyUrlRef.current = nextProxyUrl;
        if (developerToolsVisible) {
          void callNative("browser_webview_command", {
            panelId,
            command: "openDevtools",
          }).catch(() => {});
        }
      })().catch(() => {});
    };

    syncNativeWebview();
    const resizeObserver = new ResizeObserver(syncNativeWebview);
    resizeObserver.observe(slot);
    window.addEventListener("resize", syncNativeWebview);
    window.addEventListener("scroll", syncNativeWebview, true);
    return () => {
      cancelled = true;
      resizeObserver.disconnect();
      window.removeEventListener("resize", syncNativeWebview);
      window.removeEventListener("scroll", syncNativeWebview, true);
    };
  }, [
    currentUrl,
    currentZoom,
    developerToolsVisible,
    isBlank,
    panelId,
    proxyUrl,
    useNativeWebview,
  ]);

  useEffect(() => {
    if (!useNativeWebview || !nativeAttachedRef.current) {
      return;
    }
    void callNative("browser_webview_command", {
      panelId,
      command: developerToolsVisible ? "openDevtools" : "closeDevtools",
    }).catch(() => {});
  }, [developerToolsVisible, panelId, useNativeWebview]);

  useEffect(() => {
    if (!useNativeWebview) {
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listenNative<BrowserWebviewNavigatedPayload>(
      "cmux://browser-webview-navigated",
      (payload) => {
        if (
          disposed ||
          payload.panelId !== panelId ||
          payload.url === currentUrl ||
          payload.url === DEFAULT_BROWSER_URL
        ) {
          return;
        }
        onNavigate?.(payload.url);
      },
    ).then((next) => {
      if (disposed) {
        next();
        return;
      }
      unlisten = next;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [currentUrl, onNavigate, panelId, useNativeWebview]);

  useEffect(() => {
    const onCommand = (event: Event): void => {
      const detail = (event as CustomEvent<BrowserCommandDetail>).detail;
      if (detail?.panelId !== panelId) {
        return;
      }
      switch (detail.command) {
        case "back":
          onBack?.();
          break;
        case "forward":
          onForward?.();
          break;
        case "reload":
          setFrameKey((prev) => prev + 1);
          if (useNativeWebview) {
            void callNative("browser_webview_command", {
              panelId,
              command: "reload",
            }).catch(() => {});
          }
          break;
        case "focusAddressBar":
          document
            .querySelector<HTMLInputElement>(
              `[data-cmux-browser-address="${CSS.escape(panelId)}"]`,
            )
            ?.focus();
          break;
        case "openDefault":
          if (useNativeWebview) {
            void callNative("browser_webview_command", {
              panelId,
              command: "focus",
            }).catch(() => {});
            break;
          }
          if (!isBlank) {
            window.open(currentUrl, "_blank", "noopener,noreferrer");
          }
          break;
        case "zoomIn":
          onZoomChange?.(clampZoom(currentZoom + 0.1));
          break;
        case "zoomOut":
          onZoomChange?.(clampZoom(currentZoom - 0.1));
          break;
        case "zoomReset":
          onZoomChange?.(1);
          break;
      }
    };
    window.addEventListener(BROWSER_COMMAND_EVENT, onCommand);
    return () => window.removeEventListener(BROWSER_COMMAND_EVENT, onCommand);
  }, [
    currentUrl,
    currentZoom,
    isBlank,
    onBack,
    onForward,
    onZoomChange,
    panelId,
    useNativeWebview,
  ]);

  return (
    <div
      className={[
        "cmux-browser-surface",
        focusModeActive ? "cmux-browser-surface-focus-mode" : "",
        developerToolsVisible ? "cmux-browser-surface-with-devtools" : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {focusModeActive ? (
        <div className="cmux-browser-focus-badge">Browser focus mode</div>
      ) : null}
      {omnibarVisible ? (
        <form
          className="cmux-browser-toolbar"
          onSubmit={(event) => {
            event.preventDefault();
            navigate(draft);
          }}
        >
          <button
            type="button"
            className="cmux-browser-tool"
            aria-label="Back"
            disabled={!canGoBack}
            onClick={() => dispatchBrowserCommand(panelId, "back")}
          >
            <span aria-hidden="true">&lt;</span>
          </button>
          <button
            type="button"
            className="cmux-browser-tool"
            aria-label="Forward"
            disabled={!canGoForward}
            onClick={() => dispatchBrowserCommand(panelId, "forward")}
          >
            <span aria-hidden="true">&gt;</span>
          </button>
          <button
            type="button"
            className="cmux-browser-tool"
            aria-label="Reload"
            onClick={() => dispatchBrowserCommand(panelId, "reload")}
          >
            <span aria-hidden="true">R</span>
          </button>
          <input
            data-cmux-browser-address={panelId}
            className="cmux-browser-address"
            value={draft}
            spellCheck={false}
            onChange={(event) => setDraft(event.currentTarget.value)}
            onFocus={(event) => event.currentTarget.select()}
          />
          <button type="submit" className="cmux-browser-go">
            Go
          </button>
          <button
            type="button"
            className="cmux-browser-tool"
            aria-label="Hide Browser Omnibar"
            onClick={onToggleOmnibar}
          >
            <span aria-hidden="true">-</span>
          </button>
          <button
            type="button"
            className="cmux-browser-tool"
            aria-label="Toggle Browser Focus Mode"
            aria-pressed={focusModeActive}
            onClick={onToggleFocusMode}
          >
            <span aria-hidden="true">F</span>
          </button>
          <button
            type="button"
            className="cmux-browser-tool"
            aria-label="Zoom Out"
            onClick={() => dispatchBrowserCommand(panelId, "zoomOut")}
          >
            <span aria-hidden="true">-</span>
          </button>
          <button
            type="button"
            className="cmux-browser-tool cmux-browser-tool-wide"
            aria-label="Reset Zoom"
            onClick={() => dispatchBrowserCommand(panelId, "zoomReset")}
          >
            <span aria-hidden="true">{Math.round(currentZoom * 100)}%</span>
          </button>
          <button
            type="button"
            className="cmux-browser-tool"
            aria-label="Zoom In"
            onClick={() => dispatchBrowserCommand(panelId, "zoomIn")}
          >
            <span aria-hidden="true">+</span>
          </button>
          <button
            type="button"
            className="cmux-browser-tool"
            aria-label="Clear Browser History"
            onClick={onClearHistory}
          >
            <span aria-hidden="true">H</span>
          </button>
          <button
            type="button"
            className="cmux-browser-tool"
            aria-label="Toggle Developer Tools"
            aria-pressed={developerToolsVisible}
            onClick={onToggleDeveloperTools}
          >
            <span aria-hidden="true">D</span>
          </button>
        </form>
      ) : (
        <button
          type="button"
          className="cmux-browser-omnibar-peek"
          aria-label="Show Browser Omnibar"
          onClick={onToggleOmnibar}
        >
          Show address bar
        </button>
      )}
      <div className="cmux-browser-frame-wrap">
        {isBlank ? (
          <div className="cmux-browser-empty">
            <input
              className="cmux-browser-empty-input"
              value={draft === DEFAULT_BROWSER_URL ? "" : draft}
              placeholder="Search or enter URL"
              spellCheck={false}
              onChange={(event) => setDraft(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  navigate(draft);
                }
              }}
            />
            {showImportHint ? (
              <div className="cmux-browser-import-hint">
                <div>
                  <div className="cmux-browser-import-hint-title">
                    Bring your browser data into cmux
                  </div>
                  <div className="cmux-browser-import-hint-copy">
                    Import bookmarks, history, and cookies from detected browser
                    profiles.
                  </div>
                </div>
                <div className="cmux-browser-import-hint-actions">
                  <button
                    type="button"
                    aria-label="BrowserImportHintImportButton"
                    onClick={onOpenImportHint}
                  >
                    Import
                  </button>
                  <button
                    type="button"
                    aria-label="BrowserImportHintSettingsButton"
                    onClick={onOpenImportHintSettings}
                  >
                    Settings
                  </button>
                  <button
                    type="button"
                    aria-label="BrowserImportHintDismissButton"
                    onClick={onDismissImportHint}
                  >
                    Dismiss
                  </button>
                </div>
              </div>
            ) : null}
          </div>
        ) : (
          useNativeWebview ? (
            <div
              ref={nativeSlotRef}
              className="cmux-browser-native-slot"
              aria-label="Browser"
            >
              <div className="cmux-browser-native-placeholder">
                Native browser surface
              </div>
            </div>
          ) : (
            <iframe
              key={`${currentUrl}:${frameKey}`}
              title="Browser"
              className="cmux-browser-frame"
              src={currentUrl}
              style={scaledFrameStyle}
            />
          )
        )}
      </div>
      {developerToolsVisible ? (
        <div className="cmux-browser-devtools" aria-label="Browser Developer Tools">
          <div className="cmux-browser-devtools-tabs">
            <button
              type="button"
              aria-pressed={normalizedDevToolsPanel === "inspector"}
              className={
                normalizedDevToolsPanel === "inspector"
                  ? "cmux-browser-devtools-tab cmux-browser-devtools-tab-active"
                  : "cmux-browser-devtools-tab"
              }
              onClick={() => onShowDeveloperToolsPanel?.("inspector")}
            >
              Inspector
            </button>
            <button
              type="button"
              aria-pressed={normalizedDevToolsPanel === "console"}
              className={
                normalizedDevToolsPanel === "console"
                  ? "cmux-browser-devtools-tab cmux-browser-devtools-tab-active"
                  : "cmux-browser-devtools-tab"
              }
              onClick={() => onShowDeveloperToolsPanel?.("console")}
            >
              Console
            </button>
            <button
              type="button"
              aria-pressed={normalizedDevToolsPanel === "react"}
              className={
                normalizedDevToolsPanel === "react"
                  ? "cmux-browser-devtools-tab cmux-browser-devtools-tab-active"
                  : "cmux-browser-devtools-tab"
              }
              onClick={() => onShowDeveloperToolsPanel?.("react")}
            >
              React
            </button>
            <button
              type="button"
              aria-pressed={normalizedDevToolsPanel === "network"}
              className={
                normalizedDevToolsPanel === "network"
                  ? "cmux-browser-devtools-tab cmux-browser-devtools-tab-active"
                  : "cmux-browser-devtools-tab"
              }
              onClick={() => onShowDeveloperToolsPanel?.("network")}
            >
              Network
            </button>
            <button
              type="button"
              className="cmux-browser-devtools-close"
              aria-label="Hide Browser Developer Tools"
              onClick={onToggleDeveloperTools}
            >
              Close
            </button>
          </div>
          {normalizedDevToolsPanel === "network" ? (
            <div
              className="cmux-browser-devtools-body cmux-browser-network-panel"
              aria-label="Browser Network Requests"
            >
              <div className="cmux-browser-network-header">
                <div>
                  <div className="cmux-browser-devtools-title">Network</div>
                  <div className="cmux-browser-devtools-copy">
                    {networkReply?.observer.note ??
                      "Network records load from the browser pane observer when available."}
                  </div>
                </div>
                <div className="cmux-browser-network-actions">
                  <button
                    type="button"
                    className="cmux-browser-network-refresh"
                    onClick={() => setNetworkRefreshNonce((value) => value + 1)}
                  >
                    {networkLoading ? "Refreshing..." : "Refresh"}
                  </button>
                  <button
                    type="button"
                    className="cmux-browser-network-refresh"
                    onClick={clearNetworkRequests}
                    disabled={networkLoading}
                  >
                    Clear records
                  </button>
                </div>
              </div>
              <form
                className="cmux-browser-network-filters"
                onSubmit={(event) => {
                  event.preventDefault();
                  setNetworkRefreshNonce((value) => value + 1);
                }}
              >
                <label>
                  URL contains
                  <input
                    value={networkUrlFilter}
                    placeholder="localhost, /api, favicon.ico"
                    spellCheck={false}
                    onChange={(event) =>
                      setNetworkUrlFilter(event.currentTarget.value)
                    }
                  />
                </label>
                <label>
                  Method
                  <input
                    value={networkMethodFilter}
                    placeholder="GET"
                    spellCheck={false}
                    onChange={(event) =>
                      setNetworkMethodFilter(event.currentTarget.value)
                    }
                  />
                </label>
                <label>
                  After request id
                  <input
                    value={networkSinceIdFilter}
                    placeholder="browser-network-42"
                    spellCheck={false}
                    onChange={(event) =>
                      setNetworkSinceIdFilter(event.currentTarget.value)
                    }
                  />
                </label>
                <label>
                  Limit
                  <input
                    value={networkLimitInput}
                    inputMode="numeric"
                    placeholder="50"
                    spellCheck={false}
                    onChange={(event) =>
                      setNetworkLimitInput(event.currentTarget.value)
                    }
                  />
                </label>
                <button type="submit">Apply filters</button>
                <button
                  type="button"
                  onClick={() => {
                    setNetworkUrlFilter("");
                    setNetworkMethodFilter("");
                    setNetworkSinceIdFilter("");
                    setNetworkLimitInput("50");
                    setNetworkRefreshNonce((value) => value + 1);
                  }}
                >
                  Clear
                </button>
              </form>
              {networkError ? (
                <div className="cmux-browser-network-empty">{networkError}</div>
              ) : null}
              {!networkError && !networkReply && networkLoading ? (
                <div className="cmux-browser-network-empty">
                  Loading network requests...
                </div>
              ) : null}
              {!networkError && networkReply ? (
                <>
                  <div className="cmux-browser-network-summary">
                    <span>{networkReply.returnedCount} shown</span>
                    <span>{networkReply.filteredCount} matched</span>
                    <span>{networkReply.totalCount} retained</span>
                    {networkActiveFilters.map((filter) => (
                      <span
                        className="cmux-browser-network-filter-chip"
                        key={filter}
                      >
                        {filter}
                      </span>
                    ))}
                    <span>{networkReply.observer.source}</span>
                    <span>
                      {networkReply.observer.proxyAttributionMode === "none"
                        ? "no proxy attribution"
                        : `${networkReply.observer.proxyAttributionMode} proxy`}
                    </span>
                    <span>
                      {networkReply.observer.capturesResponseBody
                        ? "response bodies"
                        : "metadata only"}
                    </span>
                    {typeof networkReply.observer.bodyCaptureLimitBytes === "number" ? (
                      <span>
                        {networkReply.observer.bodyCaptureLimitBytes}B body preview cap
                      </span>
                    ) : null}
                  </div>
                  {networkReply.requests.length === 0 ? (
                    <div className="cmux-browser-network-empty">
                      {browserNetworkEmptyMessage(hasNetworkActiveFilters)}
                    </div>
                  ) : (
                    <div className="cmux-browser-network-list">
                      {networkReply.requests.map((record) => (
                        <div className="cmux-browser-network-row" key={record.id}>
                          <div className="cmux-browser-network-main">
                            <span className="cmux-browser-network-method">
                              {record.method}
                            </span>
                            <span className="cmux-browser-network-url">
                              {record.url}
                            </span>
                          </div>
                          <div className="cmux-browser-network-meta">
                            <span>
                              {record.responseStatus ?? "pending"}
                            </span>
                            <span>{networkTimingLabel(record)}</span>
                            <span>{networkBodyLabel(record)}</span>
                            <span>
                              {headerCount(record.requestHeaders)} req hdr /
                              {headerCount(record.responseHeaders)} res hdr
                            </span>
                            <span>{record.transport}</span>
                            <span>{record.source}</span>
                            <span>{record.id}</span>
                            {record.proxyAttribution ? (
                              <span>{record.proxyAttribution} proxy</span>
                            ) : null}
                          </div>
                          <details className="cmux-browser-network-detail">
                            <summary>Inspect metadata/headers/body</summary>
                            <div className="cmux-browser-network-detail-grid">
                              <section className="cmux-browser-network-detail-section">
                                <h4>Summary</h4>
                                <dl className="cmux-browser-network-headers">
                                  <div>
                                    <dt>id</dt>
                                    <dd>{record.id}</dd>
                                  </div>
                                  <div>
                                    <dt>method</dt>
                                    <dd>{record.method}</dd>
                                  </div>
                                  <div>
                                    <dt>status</dt>
                                    <dd>{record.responseStatus ?? "pending"}</dd>
                                  </div>
                                  <div>
                                    <dt>transport</dt>
                                    <dd>{record.transport}</dd>
                                  </div>
                                  <div>
                                    <dt>source</dt>
                                    <dd>{record.source}</dd>
                                  </div>
                                  <div>
                                    <dt>proxy</dt>
                                    <dd>{record.proxyAttribution ?? "none"}</dd>
                                  </div>
                                </dl>
                              </section>
                              {record.note ? (
                                <section className="cmux-browser-network-detail-section cmux-browser-network-detail-section-wide">
                                  <h4>Record Note</h4>
                                  <div className="cmux-browser-network-note">
                                    {record.note}
                                  </div>
                                </section>
                              ) : null}
                              <section className="cmux-browser-network-detail-section">
                                <h4>Request Headers</h4>
                                {sortedHeaderEntries(record.requestHeaders).length > 0 ? (
                                  <dl className="cmux-browser-network-headers">
                                    {sortedHeaderEntries(record.requestHeaders).map(
                                      ([name, value]) => (
                                        <div key={name}>
                                          <dt>{name}</dt>
                                          <dd>{value}</dd>
                                        </div>
                                      ),
                                    )}
                                  </dl>
                                ) : (
                                  <div className="cmux-browser-network-muted">
                                    No request headers captured.
                                  </div>
                                )}
                              </section>
                              <section className="cmux-browser-network-detail-section">
                                <h4>Response Headers</h4>
                                {sortedHeaderEntries(record.responseHeaders).length > 0 ? (
                                  <dl className="cmux-browser-network-headers">
                                    {sortedHeaderEntries(record.responseHeaders).map(
                                      ([name, value]) => (
                                        <div key={name}>
                                          <dt>{name}</dt>
                                          <dd>{value}</dd>
                                        </div>
                                      ),
                                    )}
                                  </dl>
                                ) : (
                                  <div className="cmux-browser-network-muted">
                                    No response headers captured.
                                  </div>
                                )}
                              </section>
                              <section className="cmux-browser-network-detail-section">
                                <h4>Timing</h4>
                                <dl className="cmux-browser-network-headers">
                                  <div>
                                    <dt>started</dt>
                                    <dd>{networkTimestampLabel(record.startedAtMs)}</dd>
                                  </div>
                                  <div>
                                    <dt>completed</dt>
                                    <dd>{networkTimestampLabel(record.completedAtMs)}</dd>
                                  </div>
                                  <div>
                                    <dt>duration</dt>
                                    <dd>{networkTimingLabel(record)}</dd>
                                  </div>
                                </dl>
                              </section>
                              <section className="cmux-browser-network-detail-section">
                                <h4>Request Body</h4>
                                <dl className="cmux-browser-network-headers cmux-browser-network-body-meta">
                                  <div>
                                    <dt>size</dt>
                                    <dd>{record.requestBodySize}B</dd>
                                  </div>
                                  <div>
                                    <dt>preview</dt>
                                    <dd>
                                      {networkPreviewKindLabel(
                                        record.requestBodyPreviewKind,
                                      )}
                                    </dd>
                                  </div>
                                  <div>
                                    <dt>truncated</dt>
                                    <dd>
                                      {networkBooleanLabel(
                                        record.requestBodyTruncated,
                                      )}
                                    </dd>
                                  </div>
                                </dl>
                                <pre className="cmux-browser-network-body">
                                  {networkBodyPreview(
                                    record.requestBody,
                                    record.requestBodySize,
                                    record.requestBodyTruncated,
                                    record.requestBodyPreviewKind,
                                  )}
                                </pre>
                              </section>
                              <section className="cmux-browser-network-detail-section">
                                <h4>Response Body</h4>
                                <dl className="cmux-browser-network-headers cmux-browser-network-body-meta">
                                  <div>
                                    <dt>size</dt>
                                    <dd>{record.responseBodySize}B</dd>
                                  </div>
                                  <div>
                                    <dt>preview</dt>
                                    <dd>
                                      {networkPreviewKindLabel(
                                        record.responseBodyPreviewKind,
                                      )}
                                    </dd>
                                  </div>
                                  <div>
                                    <dt>truncated</dt>
                                    <dd>
                                      {networkBooleanLabel(
                                        record.responseBodyTruncated,
                                      )}
                                    </dd>
                                  </div>
                                </dl>
                                <pre className="cmux-browser-network-body">
                                  {networkBodyPreview(
                                    record.responseBody,
                                    record.responseBodySize,
                                    record.responseBodyTruncated,
                                    record.responseBodyPreviewKind,
                                  )}
                                </pre>
                              </section>
                            </div>
                          </details>
                        </div>
                      ))}
                    </div>
                  )}
                </>
              ) : null}
            </div>
          ) : (
            <div className="cmux-browser-devtools-body">
              <div className="cmux-browser-devtools-title">
                {normalizedDevToolsPanel === "console"
                  ? "JavaScript Console"
                  : normalizedDevToolsPanel === "react"
                    ? "React Grab"
                    : "Inspector"}
              </div>
              <div className="cmux-browser-devtools-copy">
                {normalizedDevToolsPanel === "console"
                  ? "Console lane is active for this browser pane. Full protocol-backed evaluation is still gated by the embedded WebView bridge."
                  : normalizedDevToolsPanel === "react"
                    ? "React Grab lane is active for this browser pane. It is persisted with the pane and ready for the inspector bridge."
                    : "Developer tools are visible for this browser pane. Inspector state is persisted with the session."}
              </div>
            </div>
          )}
        </div>
      ) : null}
    </div>
  );
}
