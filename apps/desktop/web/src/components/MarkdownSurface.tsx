import { markdownSurfaceUrl } from "../session/surfaceUrl";

/**
 * A pane surface hosting the markdown preview shell (`cmux-md://…/shell.html`),
 * served by the Rust `cmux-md` scheme handler (`resolve_md_request`). The shell
 * self-boots its renderer (marked.js + lazy libs) and receives documents via the
 * `markdown_render` / `markdown_apply_theme` bridge (see `host/markdownBridge`).
 *
 * The document runs untrusted markdown-derived HTML, so it is isolated in a
 * sandboxed `<iframe>`: `allow-scripts` (the renderer + lazy libs need JS) plus
 * `allow-same-origin` (the shell fetches its own `cmux-md` assets and talks to
 * the injected bridge). Unlike the terminal/agent surfaces this iframe is mounted
 * on demand — it holds no irrecoverable in-flight state, so the workspace tears
 * it down when the pane switches away.
 */
export function MarkdownSurface(): React.JSX.Element {
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
