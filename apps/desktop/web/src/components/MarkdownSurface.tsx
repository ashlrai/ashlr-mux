import { useEffect, useRef } from "react";

import {
  renderMarkdownDocument,
  type MarkdownDocument,
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
  document,
}: {
  document?: MarkdownDocument;
}): React.JSX.Element {
  // Serialize feeds: two in-flight set/render pairs could otherwise interleave
  // as set(A), set(B), render(B), render(A), leaving markdown A jailed to path B.
  const feedChain = useRef<Promise<void>>(Promise.resolve());
  useEffect(() => {
    // No document → no invokes; the pre-set empty jail state (markdown.rs:29-31)
    // is the documented default.
    if (!document) return;
    const doc = document;
    feedChain.current = feedChain.current
      .then(() => renderMarkdownDocument(doc))
      // A bridge rejection must not poison the chain for the next feed.
      .catch(() => {});
  }, [document?.path, document?.markdown]);

  return (
    <iframe
      title="Markdown preview"
      className="cmux-markdown-surface"
      src={markdownSurfaceUrl()}
      sandbox="allow-scripts allow-same-origin"
      style={{ width: "100%", height: "100%", border: "none" }}
    />
  );
}
