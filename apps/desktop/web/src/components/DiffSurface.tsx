import { diffSurfaceUrl } from "../session/surfaceUrl";

export interface DiffSurfaceProps {
  /**
   * The diff session token that keys the live `cmux-diff-viewer` registry entry.
   * Absent until the diff-session mint flow is ported (needs live WebView2), so
   * the surface renders a placeholder instead of an iframe.
   */
  token?: string | null;
}

/**
 * A pane surface hosting the diff viewer (`cmux-diff-viewer://<token>/index.html`),
 * served by the Rust `cmux-diff-viewer` scheme handler (`resolve_diff_request`).
 * The viewer document is a per-session sandbox: its files are only served while
 * `token` names a live registry entry.
 *
 * There is no live token source in this headless slice (minting a session needs
 * WebView2), so with no `token` the surface shows a neutral placeholder rather
 * than navigating an iframe to a URL that would resolve to `None`. Given a token
 * it renders a sandboxed `<iframe>` (`allow-scripts` for the viewer's JS,
 * `allow-same-origin` for its own `cmux-diff-viewer` asset fetches). Mounted on
 * demand — the workspace tears it down when the pane switches away.
 */
export function DiffSurface({ token }: DiffSurfaceProps): React.JSX.Element {
  if (!token) {
    return (
      <div
        className="cmux-diff-surface-placeholder"
        style={{
          display: "flex",
          width: "100%",
          height: "100%",
          alignItems: "center",
          justifyContent: "center",
          fontSize: 12,
          color: "#6b7280",
          userSelect: "none",
        }}
      >
        No diff session.
      </div>
    );
  }
  return (
    <iframe
      title="Diff viewer"
      className="cmux-diff-surface"
      src={diffSurfaceUrl(token)}
      sandbox="allow-scripts allow-same-origin"
      style={{ width: "100%", height: "100%", border: "none" }}
    />
  );
}
