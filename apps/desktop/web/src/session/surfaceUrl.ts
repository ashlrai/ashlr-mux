/**
 * Pure URL builders for the pane surfaces that load a WebView2 custom-scheme
 * document (markdown preview + diff viewer), plus the surface-kind normalizer
 * the workspace branches on.
 *
 * These stay byte-aligned with the Rust scheme parsers in
 * `src-tauri/src/schemes.rs`:
 *
 *  - markdown → `cmux-md://<host>/<asset>` (served by `resolve_md_request`), and
 *  - diff     → `cmux-diff-viewer://<token>/<request-path>` (served by
 *    `resolve_diff_request` / gated by `parse_diff_viewer_uri`).
 *
 * WebView2 rewrites a custom-scheme request `foo://<host>/<path>` to
 * `http://foo.localhost/<path>` before it reaches the async scheme handler (see
 * the scheme-origin note in `schemes.rs`), so every builder here also exposes the
 * `http://<scheme>.localhost/…` rewrite variant. The *navigation* target (the
 * iframe `src`) uses the canonical custom-scheme form — WebView2 does the
 * rewrite internally; the `*HttpUrl` variants exist so the frontend can assert
 * parity with what the Rust handler actually receives.
 */

/** The surface a pane hosts. Absent/unknown `surface_kind` ⇒ a terminal shell. */
export type SurfaceKind =
  | "terminal"
  | "agent"
  | "markdown"
  | "file"
  | "diff"
  | "browser"
  | "custom-sidebar";
export type SwitchableSurfaceKind = Exclude<SurfaceKind, "terminal">;

/**
 * Normalize a raw `surface_kind` (the Rust model stores an arbitrary
 * `Option<String>`) into the closed {@link SurfaceKind} set. Faithful to the
 * macOS model where an absent kind means a terminal: any value that is not a
 * recognized surface (including `undefined`, `null`, `""`, or a future/unknown
 * tag) falls back to `"terminal"`, so old snapshots decode unchanged.
 */
export function normalizeSurfaceKind(raw?: string | null): SurfaceKind {
  switch (raw) {
    case "agent":
      return "agent";
    case "markdown":
      return "markdown";
    case "file":
      return "file";
    case "diff":
      return "diff";
    case "browser":
      return "browser";
    case "custom-sidebar":
      return "custom-sidebar";
    default:
      return "terminal";
  }
}

/**
 * Custom-scheme URL for the markdown viewer shell — the navigation target the
 * markdown iframe loads. Matches `resolve_md_request`'s `/shell.html` arm. When
 * `panelId` is provided it is threaded through the query string so the native
 * markdown bridge and local-image jail can route messages back to the owning
 * iframe.
 */
export function markdownSurfaceUrl(panelId?: string): string {
  const query = panelId ? `?panelId=${encodeURIComponent(panelId)}` : "";
  return `cmux-md://localhost/shell.html${query}`;
}

/**
 * The WebView2 rewrite variant of {@link markdownSurfaceUrl} — the form the
 * Rust `cmux-md` scheme handler actually receives (`http://cmux-md.localhost/…`).
 */
export function markdownSurfaceHttpUrl(panelId?: string): string {
  const query = panelId ? `?panelId=${encodeURIComponent(panelId)}` : "";
  return `http://cmux-md.localhost/shell.html${query}`;
}

/** Thrown when a diff surface URL is requested without a session token. */
export class EmptyDiffTokenError extends Error {
  constructor() {
    super("diffSurfaceUrl requires a non-empty diff session token");
    this.name = "EmptyDiffTokenError";
  }
}

function normalizeDiffRequestPath(requestPath?: string | null): string {
  if (requestPath == null || requestPath.trim() === "") {
    return "/index.html";
  }
  const trimmed = requestPath.trim();
  return trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
}

/**
 * Custom-scheme URL for the diff viewer index of session `token` — the
 * navigation target the diff iframe loads. Byte-aligned with
 * `parse_diff_viewer_uri` (`cmux-diff-viewer://<token>/index.html` →
 * token=`<token>`, path=`/index.html`). An empty token has no live session and
 * would parse to `None` on the Rust side, so it is rejected up front.
 */
export function diffSurfaceUrl(token: string, requestPath?: string | null): string {
  if (token === "") {
    throw new EmptyDiffTokenError();
  }
  return `cmux-diff-viewer://${token}${normalizeDiffRequestPath(requestPath)}`;
}

/**
 * The WebView2 rewrite variant of {@link diffSurfaceUrl} — the form the Rust
 * `cmux-diff-viewer` scheme handler actually receives
 * (`http://cmux-diff-viewer.localhost/<token>/index.html`).
 */
export function diffSurfaceHttpUrl(token: string, requestPath?: string | null): string {
  if (token === "") {
    throw new EmptyDiffTokenError();
  }
  return `http://cmux-diff-viewer.localhost/${token}${normalizeDiffRequestPath(requestPath)}`;
}
