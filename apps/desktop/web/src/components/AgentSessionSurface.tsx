import { useEffect, useRef } from "react";

import { AgentSessionApp } from "@cmux/webviews/src/agent-session/react/main";
import { applyCodexDocumentMetadata } from "@cmux/webviews/src/agent-session/shared/theme";
import { type ComposerHost, focusComposer } from "../session/composerFocus";
import { host } from "../host/host";
// The reused chat UI's Tailwind stylesheet (CODEX_* classes + `--agent-*` token
// mapping). This is a WHOLE-DOCUMENT sheet — it sets `html, body, #root {
// background: transparent }`, which would black out our shared desktop shell (a
// transparent root lets the WebView2 window's black show through). The shell's
// `styles.css` re-asserts the root background with `!important` so this sheet can
// only style the agent surface, not clobber the shell. Proper long-term fix is to
// isolate the agent app in its own document (iframe/shadow root) — see
// docs/windows-port/ULTRACODE-RESUME.md.
import "@cmux/webviews/src/agent-session/shared/styles.css";

const AGENT_PORT_SCAN_DELAYS_MS = [500, 1500, 3000, 5000, 7500, 10000] as const;

let sharedAgentPortScanTimer: number | null = null;

function kickAgentPortScanBurst(): () => void {
  let cancelled = false;
  const timerIds = AGENT_PORT_SCAN_DELAYS_MS.map((delay) =>
    window.setTimeout(() => {
      if (cancelled) {
        return;
      }
      if (sharedAgentPortScanTimer != null) {
        window.clearTimeout(sharedAgentPortScanTimer);
      }
      sharedAgentPortScanTimer = window.setTimeout(() => {
        sharedAgentPortScanTimer = null;
        void host.invoke("agent_scan_listening_ports").catch(() => {});
      }, 100);
    }, delay),
  );
  return () => {
    cancelled = true;
    for (const timerId of timerIds) {
      window.clearTimeout(timerId);
    }
  };
}

/**
 * A pane surface hosting the reused `webviews/agent-session` React app — the
 * canonical agent chat UI, mounted inside the desktop workspace.
 *
 * The app self-boots off the WKWebView contract the shell already installs once
 * at startup (`installMacHostShims` in `main.tsx`: `window.webkit.messageHandlers
 * .agentSession` → the `agent_session_rpc` Tauri command, and `cmux://agent-event`
 * → `window.cmuxAgentBridge.receive`). The desktop mount contributes the pane
 * identity the standalone webview never had, so native can bind starts/writes to
 * the correct restorable panel.
 *
 * The one thing the embedded (non-standalone) mount must do that the standalone
 * entry normally does is set the Codex document metadata (`data-codex-*` gates
 * the chat CSS); `applyCodexDocumentMetadata` is idempotent, so running it per
 * mount is safe. The real theme (light/dark + `--agent-*` vars) then arrives via
 * `app.context` and `app.theme` events.
 */
export function AgentSessionSurface({
  panelId,
  workspaceId,
}: {
  panelId: string;
  workspaceId?: string;
}): React.JSX.Element {
  const surfaceRef = useRef<HTMLDivElement | null>(null);
  const nativeScope = workspaceId ? { panelId, workspaceId } : { panelId };

  useEffect(() => {
    applyCodexDocumentMetadata();
  }, []);

  useEffect(() => {
    const cancelMountBurst = kickAgentPortScanBurst();
    let disposed = false;
    let unlisten: (() => void) | null = null;
    let cancelEventBurst: (() => void) | null = null;

    void host
      .on("cmux://agent-event", () => {
        if (disposed) {
          return;
        }
        cancelEventBurst?.();
        cancelEventBurst = kickAgentPortScanBurst();
      })
      .then((off) => {
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      });

    return () => {
      disposed = true;
      cancelMountBurst();
      cancelEventBurst?.();
      unlisten?.();
    };
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
    const composerHost: ComposerHost = {
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
      if (focusComposer(composerHost, document) === "no-composer" && attempts < MAX_ATTEMPTS) {
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
      <AgentSessionApp nativeScope={nativeScope} />
    </div>
  );
}
