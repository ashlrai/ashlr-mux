import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import {
  BrowserSurface,
  browserNetworkActiveFilters,
  browserNetworkEmptyMessage,
  browserNetworkLimitValue,
  browserNetworkRequestParams,
  browserNativeWebviewSyncPlan,
  dispatchBrowserCommand,
  dispatchBrowserNetworkCleared,
} from "./BrowserSurface";

describe("BrowserSurface", () => {
  test("renders a blank start surface when no URL is bound", () => {
    const markup = renderToStaticMarkup(<BrowserSurface panelId="surface-1" />);
    expect(markup).toContain("cmux-browser-surface");
    expect(markup).toContain("cmux-browser-empty");
    expect(markup).not.toContain("<iframe");
    expect(markup).not.toContain("BrowserImportHintImportButton");
  });

  test("renders the blank-tab browser import hint when enabled", () => {
    const markup = renderToStaticMarkup(
      <BrowserSurface panelId="surface-1" showImportHint />,
    );
    expect(markup).toContain("cmux-browser-import-hint");
    expect(markup).toContain("Bring your browser data into cmux");
    expect(markup).toContain("BrowserImportHintImportButton");
    expect(markup).toContain("BrowserImportHintSettingsButton");
    expect(markup).toContain("BrowserImportHintDismissButton");
  });

  test("renders an iframe for a stored browser URL", () => {
    const markup = renderToStaticMarkup(
      <BrowserSurface panelId="surface-1" url="https://example.com" zoom={1.25} />,
    );
    expect(markup).toContain('title="Browser"');
    expect(markup).toContain('src="https://example.com"');
    expect(markup).toContain("scale(1.25)");
  });

  test("renders a native child webview slot in the Tauri desktop runtime", () => {
    const previousWindow = globalThis.window;
    try {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: {
          __TAURI__: {
            core: {
              invoke: async () => ({}),
            },
          },
        },
      });
      const markup = renderToStaticMarkup(
        <BrowserSurface panelId="surface-1" url="https://example.com" />,
      );
      expect(markup).toContain("cmux-browser-native-slot");
      expect(markup).not.toContain("<iframe");
    } finally {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: previousWindow,
      });
    }
  });

  test("recreates native child webviews when proxy binding changes", () => {
    expect(
      browserNativeWebviewSyncPlan(false, undefined, "socks5://127.0.0.1:4000"),
    ).toEqual({
      closeFirst: false,
      command: "browser_attach_webview",
    });
    expect(
      browserNativeWebviewSyncPlan(
        true,
        "socks5://127.0.0.1:4000",
        "socks5://127.0.0.1:4000",
      ),
    ).toEqual({
      closeFirst: false,
      command: "browser_update_webview",
    });
    expect(
      browserNativeWebviewSyncPlan(
        true,
        undefined,
        "socks5://127.0.0.1:4001",
      ),
    ).toEqual({
      closeFirst: true,
      command: "browser_attach_webview",
    });
    expect(
      browserNativeWebviewSyncPlan(
        true,
        "socks5://127.0.0.1:4000",
        "socks5://127.0.0.1:4001",
      ),
    ).toEqual({
      closeFirst: true,
      command: "browser_attach_webview",
    });
  });

  test("builds backend network request filters from UI fields", () => {
    expect(browserNetworkRequestParams("surface-1", "", "")).toEqual({
      panelId: "surface-1",
      limit: 50,
    });
    expect(
      browserNetworkRequestParams(
        "surface-1",
        " /favicon.ico ",
        " get ",
        25,
        " browser-network-4 ",
      ),
    ).toEqual({
      panelId: "surface-1",
      limit: 25,
      urlContains: "/favicon.ico",
      method: "GET",
      sinceId: "browser-network-4",
    });
    expect(browserNetworkLimitValue("500")).toBe(200);
    expect(browserNetworkLimitValue("-1")).toBe(50);
    expect(browserNetworkLimitValue("not a number")).toBe(50);
  });

  test("summarizes active browser network filters and empty states", () => {
    expect(browserNetworkActiveFilters("", "")).toEqual([]);
    expect(
      browserNetworkActiveFilters(
        " /api ",
        " post ",
        " browser-network-4 ",
        "25",
      ),
    ).toEqual([
      "URL: /api",
      "Method: POST",
      "After: browser-network-4",
      "Limit: 25",
    ]);
    expect(browserNetworkEmptyMessage(false)).toBe(
      "No network requests recorded for this browser pane yet.",
    );
    expect(browserNetworkEmptyMessage(true)).toBe(
      "No network requests match the active filters.",
    );
  });

  test("network clear dispatch helper is safe during server rendering", () => {
    const previousWindow = globalThis.window;
    try {
      // @ts-expect-error exercising the helper in a non-browser runtime.
      delete globalThis.window;
      expect(() => dispatchBrowserNetworkCleared("surface-1")).not.toThrow();
    } finally {
      globalThis.window = previousWindow;
    }
  });

  test("reflects persisted browser history availability in toolbar buttons", () => {
    const markup = renderToStaticMarkup(
      <BrowserSurface
        panelId="surface-1"
        url="https://example.com"
        canGoBack
        canGoForward={false}
      />,
    );
    expect(markup).toContain('aria-label="Back"');
    expect(markup).not.toMatch(/aria-label="Back" disabled=""/);
    expect(markup).toMatch(/aria-label="Forward" disabled=""/);
  });

  test("renders backend-backed browser controls in the toolbar", () => {
    const markup = renderToStaticMarkup(
      <BrowserSurface
        panelId="surface-1"
        url="https://example.com"
        zoom={1.5}
        focusModeActive
      />,
    );
    expect(markup).toContain('aria-label="Hide Browser Omnibar"');
    expect(markup).toContain('aria-label="Toggle Browser Focus Mode"');
    expect(markup).toContain('aria-pressed="true"');
    expect(markup).toContain('aria-label="Zoom Out"');
    expect(markup).toContain('aria-label="Reset Zoom"');
    expect(markup).toContain(">150%</span>");
    expect(markup).toContain('aria-label="Zoom In"');
    expect(markup).toContain('aria-label="Clear Browser History"');
  });

  test("hides the toolbar when omnibar visibility is persisted off", () => {
    const markup = renderToStaticMarkup(
      <BrowserSurface
        panelId="surface-1"
        url="https://example.com"
        omnibarVisible={false}
      />,
    );
    expect(markup).not.toContain("cmux-browser-toolbar");
    expect(markup).toContain('aria-label="Show Browser Omnibar"');
    expect(markup).toContain('src="https://example.com"');
  });

  test("renders browser focus mode and developer tools state", () => {
    const markup = renderToStaticMarkup(
      <BrowserSurface
        panelId="surface-1"
        url="https://example.com"
        focusModeActive
        developerToolsVisible
        developerToolsPanel="console"
      />,
    );
    expect(markup).toContain("Browser focus mode");
    expect(markup).toContain('aria-label="Browser Developer Tools"');
    expect(markup).toContain("JavaScript Console");
    expect(markup).toContain('aria-pressed="true"');
    expect(markup).toContain("<button");
    expect(markup).toContain(">Inspector</button>");
    expect(markup).toContain(">Console</button>");
    expect(markup).toContain(">React</button>");
    expect(markup).toContain(">Network</button>");
  });

  test("renders browser network inspection records in developer tools", () => {
    const markup = renderToStaticMarkup(
      <BrowserSurface
        panelId="surface-1"
        url="https://example.com"
        developerToolsVisible
        developerToolsPanel="network"
        networkPreview={{
          panelId: "surface-1",
          totalCount: 1,
          returnedCount: 1,
          filteredCount: 1,
          observer: {
            source: "proxy-stream-http",
            capturesUrl: true,
            capturesMethod: true,
            capturesRequestHeaders: true,
            capturesRequestBody: true,
            capturesResponseStatus: true,
            capturesResponseHeaders: true,
            capturesResponseBody: true,
            capturesTiming: true,
            supportsFilters: true,
            maxRecordsPerPanel: 200,
            bodyCaptureLimitBytes: 4096,
            proxyAttributionMode: "panel",
            note: "Cleartext proxy records include headers and bodies.",
          },
          requests: [
            {
              id: "browser-network-1",
              panelId: "surface-1",
              method: "POST",
              url: "http://example.com/submit",
              requestHeaders: { host: "example.com" },
              requestBody: "payload",
              requestBodyPreviewKind: "text",
              requestBodySize: 7,
              requestBodyTruncated: false,
              responseStatus: 202,
              responseHeaders: { "content-type": "application/json" },
              responseBody: "{\"ok\":true}",
              responseBodyPreviewKind: "text",
              responseBodySize: 11,
              responseBodyTruncated: false,
              startedAtMs: 10,
              completedAtMs: 25,
              durationMs: 15,
              source: "proxy-stream-http",
              transport: "socks5",
              proxyAttribution: "panel",
              note: null,
            },
          ],
        }}
      />,
    );
    expect(markup).toContain('aria-label="Browser Network Requests"');
    expect(markup).toContain("Cleartext proxy records include headers and bodies.");
    expect(markup).toContain("1 shown");
    expect(markup).toContain("1 matched");
    expect(markup).toContain("1 retained");
    expect(markup).toContain("URL contains");
    expect(markup).toContain("localhost, /api, favicon.ico");
    expect(markup).toContain("Method");
    expect(markup).toContain("After request id");
    expect(markup).toContain("browser-network-42");
    expect(markup).toContain("Limit");
    expect(markup).toContain("Apply filters");
    expect(markup).toContain("Clear");
    expect(markup).toContain("Clear records");
    expect(markup).toContain("response bodies");
    expect(markup).toContain("POST");
    expect(markup).toContain("http://example.com/submit");
    expect(markup).toContain("202");
    expect(markup).toContain("15ms");
    expect(markup).toContain("7B req / 11B res");
    expect(markup).toContain("1 req hdr /1 res hdr");
    expect(markup).toContain("socks5");
    expect(markup).toContain("proxy-stream-http");
    expect(markup).toContain("browser-network-1");
    expect(markup).toContain("panel proxy");
    expect(markup).toContain("4096B body preview cap");
    expect(markup).toContain("Inspect metadata/headers/body");
    expect(markup).toContain("Summary");
    expect(markup).toContain("id");
    expect(markup).toContain("status");
    expect(markup).toContain("transport");
    expect(markup).toContain("source");
    expect(markup).toContain("proxy");
    expect(markup).toContain("Request Headers");
    expect(markup).toContain("Response Headers");
    expect(markup).toContain("Timing");
    expect(markup).toContain("started");
    expect(markup).toContain("completed");
    expect(markup).toContain("duration");
    expect(markup).toContain("10ms");
    expect(markup).toContain("25ms");
    expect(markup).toContain("Request Body");
    expect(markup).toContain("Response Body");
    expect(markup).toContain("size");
    expect(markup).toContain("preview");
    expect(markup).toContain("truncated");
    expect(markup).toContain("7B");
    expect(markup).toContain("11B");
    expect(markup).toContain("no");
    expect(markup).toContain("Text preview");
    expect(markup).toContain("host");
    expect(markup).toContain("content-type");
    expect(markup).toContain("payload");
    expect(markup).toContain("ok");
  });

  test("renders opaque proxy tunnel records with backend note", () => {
    const note =
      "Opaque proxy tunnel observed after broker handshake; status 200 represents successful tunnel establishment.";
    const markup = renderToStaticMarkup(
      <BrowserSurface
        panelId="surface-1"
        url="https://secure.example"
        developerToolsVisible
        developerToolsPanel="network"
        networkPreview={{
          panelId: "surface-1",
          totalCount: 1,
          returnedCount: 1,
          filteredCount: 1,
          observer: {
            source: "proxy-stream-tunnel",
            capturesUrl: true,
            capturesMethod: true,
            capturesRequestHeaders: false,
            capturesRequestBody: true,
            capturesResponseStatus: true,
            capturesResponseHeaders: false,
            capturesResponseBody: true,
            capturesTiming: true,
            supportsFilters: true,
            maxRecordsPerPanel: 200,
            bodyCaptureLimitBytes: 4096,
            proxyAttributionMode: "panel",
            note: "Opaque proxy tunnel records capture authority and timing.",
          },
          requests: [
            {
              id: "browser-network-9",
              panelId: "surface-1",
              method: "CONNECT",
              url: "https://secure.example/",
              requestHeaders: {},
              requestBody: "<binary body: 5 bytes>",
              requestBodyPreviewKind: "binary",
              requestBodySize: 5,
              requestBodyTruncated: false,
              responseStatus: 200,
              responseHeaders: {},
              responseBody: "<binary body: 5 bytes>",
              responseBodyPreviewKind: "binary",
              responseBodySize: 5,
              responseBodyTruncated: true,
              startedAtMs: 100,
              completedAtMs: 155,
              durationMs: 55,
              source: "proxy-stream-tunnel",
              transport: "socks5",
              proxyAttribution: "panel",
              note,
            },
          ],
        }}
      />,
    );

    expect(markup).toContain("proxy-stream-tunnel");
    expect(markup).toContain("CONNECT");
    expect(markup).toContain("https://secure.example/");
    expect(markup).toContain("200");
    expect(markup).toContain("55ms");
    expect(markup).toContain("socks5");
    expect(markup).toContain("panel proxy");
    expect(markup).toContain("Record Note");
    expect(markup).toContain(note);
    expect(markup).toContain("No request headers captured.");
    expect(markup).toContain("No response headers captured.");
    expect(markup).toContain("Binary preview");
    expect(markup).toContain("5B");
    expect(markup).toContain("yes");
  });

  test("dispatchBrowserCommand is SSR-safe when no window exists", () => {
    expect(() => dispatchBrowserCommand("surface-1", "reload")).not.toThrow();
  });
});
