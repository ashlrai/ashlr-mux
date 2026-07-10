import { useEffect } from "react";

import { installDiffCommentsRelay } from "../host/diffCommentsRelay";
import { diffSurfaceUrl } from "../session/surfaceUrl";

export interface DiffSurfaceProps {
  /** The pane id hosting this diff surface; used by the comments relay. */
  panelId: string;
  /**
   * The diff session token that keys the live `cmux-diff-viewer` registry entry.
   * Absent only for malformed/restored panes that lack a registered session, in
   * which case the surface renders a guarded placeholder instead of an iframe.
   */
  token?: string | null;
  /** The registered request path within the diff session, usually `/index.html`. */
  requestPath?: string | null;
  /** Recovery path for restored/malformed diff panes that have no live token. */
  onCreateSession?: () => void;
}

/**
 * A pane surface hosting the diff viewer (`cmux-diff-viewer://<token>/index.html`),
 * served by the Rust `cmux-diff-viewer` scheme handler (`resolve_diff_request`).
 * The viewer document is a per-session sandbox: its files are only served while
 * `token` names a live registry entry.
 *
 * With no `token` the surface shows a neutral placeholder rather than
 * navigating an iframe to a URL that would resolve to `None`. Given a token it
 * renders a sandboxed `<iframe>` (`allow-scripts` for the viewer's JS,
 * `allow-same-origin` for its own `cmux-diff-viewer` asset fetches). Mounted on
 * demand — the workspace tears it down when the pane switches away.
 */
export function DiffSurface({
  panelId,
  token,
  requestPath,
  onCreateSession,
}: DiffSurfaceProps): React.JSX.Element {
  // The iframe branch needs the parent-side diff-comments relay listening on
  // this (main) window; the placeholder installs nothing. The relay is a
  // window-level singleton (a reinstall tears down the prior listener), so
  // unmounting surface A while surface B is mounted would drop B's relay —
  // acceptable today under the one-diff-pane-at-a-time interim (same invariant
  // as the agent surface); ref-counting is deliberate non-scope.
  useEffect(() => {
    if (!token) {
      return;
    }
    return installDiffCommentsRelay({ token, panelId });
  }, [panelId, token]);

  if (!token) {
    return (
      <div className="cmux-diff-surface-placeholder">
        <div className="cmux-diff-surface-placeholder-title">No diff session.</div>
        {onCreateSession ? (
          <button
            type="button"
            className="cmux-diff-surface-placeholder-action"
            onClick={onCreateSession}
          >
            Start diff session
          </button>
        ) : null}
      </div>
    );
  }
  return (
    <iframe
      title="Diff viewer"
      className="cmux-diff-surface"
      src={diffSurfaceUrl(token, requestPath)}
      sandbox="allow-scripts allow-same-origin"
      style={{ width: "100%", height: "100%", border: "none" }}
    />
  );
}
