import { callNative } from "./tauri-bridge";

const container = document.getElementById("root");

if (!container) {
  throw new Error("Desktop root element is missing.");
}

container.innerHTML = `
  <main class="bootstrap-shell">
    <header class="bootstrap-header">
      <div class="brand-mark" aria-hidden="true">cm</div>
      <div>
        <p class="eyebrow">Windows bootstrap</p>
        <h1>cmux for Windows</h1>
      </div>
    </header>
    <section class="hero">
      <div class="surface surface-primary">
        <div class="surface-title">Placeholder chrome</div>
        <p class="surface-copy">
          The shared Rust core is now live under the desktop shell, so session models, control
          IPC, and agent-provider contracts can move forward without depending on AppKit.
        </p>
      </div>
      <div class="surface surface-secondary">
        <div class="surface-title">Bridge status</div>
        <p class="status-text" id="bridge-status">Connecting to the native shell...</p>
      </div>
      <div class="surface surface-secondary">
        <div class="surface-title">Core status</div>
        <p class="status-text" id="core-status">Loading M1 contract snapshot...</p>
      </div>
    </section>
    <footer class="bootstrap-footer">
      <span>Milestone M1</span>
      <span>Cross-platform core extraction</span>
      <span>WebView2-ready</span>
    </footer>
  </main>
`;

const statusNode = document.getElementById("bridge-status");
const coreStatusNode = document.getElementById("core-status");

if (!statusNode || !coreStatusNode) {
  throw new Error("Desktop status element is missing.");
}

callNative<string>("ping")
  .then((result) => {
    statusNode.textContent = `Native bridge ready: ${result}`;
  })
  .catch((error) => {
    const message = error instanceof Error ? error.message : "Native bridge unavailable";
    statusNode.textContent = `Native bridge fallback: ${message}`;
  });

type DesktopCoreStatus = {
  milestone: string;
  platform: string;
  agent_providers: string[];
  ipc_fixture_request: string;
};

callNative<DesktopCoreStatus>("desktop_core_status")
  .then((result) => {
    coreStatusNode.textContent =
      `${result.milestone} · ${result.platform} · ${result.agent_providers.join(", ")}`;
  })
  .catch((error) => {
    const message = error instanceof Error ? error.message : "Shared core unavailable";
    coreStatusNode.textContent = `Shared core fallback: ${message}`;
  });
