import { useEffect } from "react";

import { AgentSessionApp } from "@cmux/webviews/src/agent-session/react/main";
import { applyCodexDocumentMetadata } from "@cmux/webviews/src/agent-session/shared/theme";
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
  useEffect(() => {
    applyCodexDocumentMetadata();
  }, []);

  return (
    <div className="cmux-agent-session-surface" style={{ width: "100%", height: "100%", overflow: "auto" }}>
      <AgentSessionApp />
    </div>
  );
}
