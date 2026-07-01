import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "@xterm/xterm/css/xterm.css";
import "./styles.css";

import { App } from "./App";
import { installMacHostShims } from "./host/host";

const container = document.getElementById("root");
if (!container) {
  throw new Error("Desktop root element is missing.");
}

// Present the macOS WKWebView host contract (webkit.messageHandlers.* +
// cmuxAgentBridge) so reused webviews surfaces run unmodified. Fire-and-forget:
// the native push subscription resolves asynchronously and the UI does not
// block on it.
void installMacHostShims();

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
