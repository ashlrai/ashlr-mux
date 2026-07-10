import { useEffect, useRef, useState } from "react";
import type { MarkdownConfig } from "@cmux/core-types";

import { host } from "../host/host";
import {
  applyMarkdownTypography,
  renderMarkdownDocument,
  resetMarkdownZoom,
  type MarkdownDocument,
  zoomMarkdownIn,
  zoomMarkdownOut,
} from "../host/markdownBridge";
import { markdownSurfaceUrl } from "../session/surfaceUrl";

/**
 * A pane surface hosting the markdown preview shell (`cmux-md://…/shell.html`),
 * served by the Rust `cmux-md` scheme handler (`resolve_md_request`). The shell
 * self-boots its renderer (marked.js + lazy libs) and receives documents via the
 * `markdown_render` / `markdown_apply_theme` bridge (see `host/markdownBridge`).
 * A provided `document` arrives through the awaited set-document → render
 * sequence (`renderMarkdownDocument`); the Rust local-image jail depends on
 * that ordering, so the two invokes must never be issued independently here.
 *
 * The document runs untrusted markdown-derived HTML, so it is isolated in a
 * sandboxed `<iframe>`: `allow-scripts` (the renderer + lazy libs need JS) plus
 * `allow-same-origin` (the shell fetches its own `cmux-md` assets and talks to
 * the injected bridge). Unlike the terminal/agent surfaces this iframe is mounted
 * on demand — it holds no irrecoverable in-flight state, so the workspace tears
 * it down when the pane switches away.
 */
export function MarkdownSurface({
  panelId,
  filePath,
  markdownConfig,
}: {
  panelId: string;
  filePath?: string;
  markdownConfig?: MarkdownConfig;
}): React.JSX.Element {
  const [ready, setReady] = useState(false);
  const [document, setDocument] = useState<MarkdownDocument | undefined>(undefined);
  // Serialize feeds: two in-flight set/render pairs could otherwise interleave
  // as set(A), set(B), render(B), render(A), leaving markdown A jailed to path B.
  const feedChain = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let cancelled = false;
    if (!filePath) {
      setDocument(undefined);
      return;
    }
    void host
      .invoke<string>("markdown_read_file", { path: filePath })
      .then((markdown) => {
        if (!cancelled) {
          setDocument({ path: filePath, markdown });
        }
      })
      .catch((error) => {
        if (!cancelled) {
          const message = error instanceof Error ? error.message : String(error);
          setDocument({
            path: filePath,
            markdown: `# Unable to open markdown file\n\n${filePath}\n\n${message}`,
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [filePath]);

  useEffect(() => {
    if (!ready) return;
    // No document → no invokes; the pre-set empty jail state (markdown.rs:29-31)
    // is the documented default.
    if (!document) return;
    const doc = document;
    feedChain.current = feedChain.current
      .then(() => renderMarkdownDocument(panelId, doc))
      // A bridge rejection must not poison the chain for the next feed.
      .catch(() => {});
  }, [document?.path, document?.markdown, panelId, ready]);

  useEffect(() => {
    setReady(false);
  }, [panelId]);

  useEffect(() => {
    if (!ready) return;
    void applyMarkdownTypography(panelId).catch((error) => {
      console.error("markdown_apply_typography failed", error);
    });
  }, [
    markdownConfig?.fontFamily,
    markdownConfig?.fontSize,
    markdownConfig?.maxWidth,
    panelId,
    ready,
  ]);

  const runZoom = (action: "in" | "out" | "reset"): void => {
    const command =
      action === "in"
        ? zoomMarkdownIn
        : action === "out"
          ? zoomMarkdownOut
          : resetMarkdownZoom;
    void command(panelId).catch((error) => {
      console.error(`markdown_zoom_${action} failed`, error);
    });
  };

  return (
    <div className="cmux-markdown-host">
      <div className="cmux-markdown-toolbar" aria-label="Markdown controls">
        <button
          type="button"
          className="cmux-markdown-tool"
          aria-label="Markdown Zoom Out"
          onClick={() => runZoom("out")}
        >
          -
        </button>
        <button
          type="button"
          className="cmux-markdown-tool cmux-markdown-tool-wide"
          aria-label="Markdown Reset Zoom"
          onClick={() => runZoom("reset")}
        >
          100%
        </button>
        <button
          type="button"
          className="cmux-markdown-tool"
          aria-label="Markdown Zoom In"
          onClick={() => runZoom("in")}
        >
          +
        </button>
      </div>
      <iframe
        title="Markdown preview"
        className="cmux-markdown-surface"
        data-cmux-markdown-panel-id={panelId}
        src={markdownSurfaceUrl(panelId)}
        sandbox="allow-scripts allow-same-origin"
        onLoad={() => setReady(true)}
        style={{ width: "100%", height: "100%", border: "none" }}
      />
    </div>
  );
}
