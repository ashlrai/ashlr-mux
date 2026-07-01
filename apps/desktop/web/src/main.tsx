import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";

import { callNative, listenNative } from "./tauri-bridge";

const container = document.getElementById("root");

if (!container) {
  throw new Error("Desktop root element is missing.");
}

type TerminalOutput = {
  id: number;
  data: string;
};

type TerminalExit = {
  id: number;
};

/** Decode base64 (the Rust output bridge) into the raw bytes xterm expects. */
function decodeBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

const term = new Terminal({
  cursorBlink: true,
  fontFamily:
    '"Cascadia Mono", "Cascadia Code", Consolas, "Courier New", monospace',
  fontSize: 14,
  theme: {
    background: "#0b0e14",
    foreground: "#e6e6e6",
  },
});

const fitAddon = new FitAddon();
term.loadAddon(fitAddon);
term.open(container);
fitAddon.fit();

async function boot(): Promise<void> {
  const { cols, rows } = term;
  const sessionId = await callNative<number>("terminal_open", { cols, rows });

  await listenNative<TerminalOutput>("cmux://terminal-output", (payload) => {
    if (payload.id === sessionId) {
      term.write(decodeBase64(payload.data));
    }
  });

  await listenNative<TerminalExit>("cmux://terminal-exit", (payload) => {
    if (payload.id === sessionId) {
      term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
    }
  });

  term.onData((data) => {
    void callNative("terminal_write", { id: sessionId, data });
  });

  const applyResize = (): void => {
    fitAddon.fit();
    void callNative("terminal_resize", {
      id: sessionId,
      cols: term.cols,
      rows: term.rows,
    });
  };

  window.addEventListener("resize", applyResize);
  new ResizeObserver(applyResize).observe(container);

  term.focus();
}

boot().catch((error) => {
  const message = error instanceof Error ? error.message : String(error);
  term.write(`\r\n\x1b[31mFailed to start terminal: ${message}\x1b[0m\r\n`);
});
