import { useEffect, useRef } from "react";

import { AgentSessionApp } from "@cmux/webviews/src/agent-session/react/main";
import { applyCodexDocumentMetadata } from "@cmux/webviews/src/agent-session/shared/theme";
import { type ComposerHost, focusComposer } from "../session/composerFocus";
// The reused chat UI's Tailwind stylesheet (CODEX_* classes + `--agent-*` token
// mapping). This is a WHOLE-DOCUMENT sheet — it sets `html, body, #root {
// background: transparent }`, which would black out our shared desktop shell (a
// transparent root lets the WebView2 window's black show through). The shell's
// `styles.css` re-asserts the root background with `!important` so this sheet can
// only style the agent surface, not clobber the shell. Proper long-term fix is to
// isolate the agent app in its own document (iframe/shadow root) — see
// docs/windows-port/ULTRACODE-RESUME.md.
import "@cmux/webviews/src/agent-session/shared/styles.css";

/**
 * A pane surface hosting the reused `webviews/agent-session` React app — the
 * canonical agent chat UI, mounted inside the desktop workspace.
 *
 * The app self-boots off the WKWebView contract the shell already installs once
 * at startup (`installMacHostShims` in `main.tsx`: `window.webkit.messageHandlers
 * .agentSession` → the `agent_session_rpc` Tauri command, and `cmux://agent-event`
 * → `window.cmuxAgentBridge.receive`). So it needs no props — it calls
 * `app.context` / `provider.list` on mount, then drives start / writeLine / stop
 * itself.
 *
 * The one thing the embedded (non-standalone) mount must do that the standalone
 * entry normally does is set the Codex document metadata (`data-codex-*` gates
 * the chat CSS); `applyCodexDocumentMetadata` is idempotent, so running it per
 * mount is safe. The real theme (light/dark + `--agent-*` vars) then arrives via
 * `app.context` and `app.theme` events.
 */
export function AgentSessionSurface(): React.JSX.Element {
  const surfaceRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    applyCodexDocumentMetadata();
  }, []);

  // Auto-focus the composer whenever this surface becomes visible, so Enter
  // sends without the user first clicking the box. The surface is never
  // unmounted — a pane toggles terminal⇄agent by flipping `display` (see the
  // flat portal in `Workspace.tsx`), and `.focus()` on a `display:none` node is
  // a no-op — so we key off the visibility transition (IntersectionObserver)
  // rather than mount. The ProseMirror editor mounts asynchronously (the agent
  // app fetches context before rendering), so we retry across a few frames until
  // it appears; `focusComposer` leaves focus alone if it already sits inside the
  // surface (e.g. an open provider/permissions menu).
  useEffect(() => {
    const surface = surfaceRef.current;
    if (!surface || typeof IntersectionObserver === "undefined") {
      return;
    }
    const MAX_ATTEMPTS = 60; // ~1s at 60fps — covers the async composer mount
    const host: ComposerHost = {
      querySelector: (selector) => surface.querySelector<HTMLElement>(selector),
      contains: (node) => surface.contains((node as Node | null) ?? null),
    };
    let rafId: number | null = null;
    let attempts = 0;
    const cancelPending = () => {
      if (rafId != null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
    };
    const tryFocus = () => {
      rafId = null;
      if (focusComposer(host, document) === "no-composer" && attempts < MAX_ATTEMPTS) {
        attempts += 1;
        rafId = requestAnimationFrame(tryFocus);
      }
    };
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        cancelPending();
        if (entry.isIntersecting) {
          attempts = 0;
          rafId = requestAnimationFrame(tryFocus);
        }
      }
    });
    observer.observe(surface);
    return () => {
      cancelPending();
      observer.disconnect();
    };
  }, []);

  return (
    <div
      ref={surfaceRef}
      className="cmux-agent-session-surface"
      style={{ width: "100%", height: "100%", overflow: "auto" }}
    >
      <AgentSessionApp />
    </div>
  );
}
