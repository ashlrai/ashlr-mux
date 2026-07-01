import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type IDisposable } from "@xterm/xterm";

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

// Launcher buttons: a plain shell plus the agent providers the Rust resolver
// knows about (see cmux_agent::AgentSessionProviderId).
const LAUNCHERS: Array<{ label: string; agent?: string }> = [
  { label: "Shell" },
  { label: "Claude", agent: "claude" },
  { label: "Codex", agent: "codex" },
  { label: "OpenCode", agent: "opencode" },
];

container.innerHTML = `
  <div id="bar">
    ${LAUNCHERS.map(
      (l, i) => `<button class="launch" data-idx="${i}">${l.label}</button>`,
    ).join("")}
    <span id="session-label"></span>
  </div>
  <div id="term"></div>
`;

const termHost = document.getElementById("term");
const sessionLabel = document.getElementById("session-label");
if (!termHost || !sessionLabel) {
  throw new Error("Desktop terminal host is missing.");
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
term.open(termHost);
fitAddon.fit();

// Live-session bookkeeping so switching launchers tears down the old shell.
let currentId: number | null = null;
let unlisteners: Array<() => void> = [];
let dataDisposable: IDisposable | null = null;
let switching = false;

async function openSession(label: string, agent?: string): Promise<void> {
  if (switching) {
    return;
  }
  switching = true;
  try {
    if (currentId !== null) {
      await callNative("terminal_close", { id: currentId }).catch(() => {});
    }
    for (const unlisten of unlisteners) {
      unlisten();
    }
    unlisteners = [];
    dataDisposable?.dispose();
    dataDisposable = null;
    term.reset();

    const { cols, rows } = term;
    const id = await callNative<number>("terminal_open", { cols, rows, agent });
    currentId = id;
    sessionLabel.textContent = label;

    unlisteners.push(
      await listenNative<TerminalOutput>("cmux://terminal-output", (payload) => {
        if (payload.id === id) {
          term.write(decodeBase64(payload.data));
        }
      }),
    );
    unlisteners.push(
      await listenNative<TerminalExit>("cmux://terminal-exit", (payload) => {
        if (payload.id === id) {
          term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
        }
      }),
    );

    dataDisposable = term.onData((data) => {
      void callNative("terminal_write", { id, data });
    });

    term.focus();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    term.write(`\r\n\x1b[31mFailed to start ${label}: ${message}\x1b[0m\r\n`);
    sessionLabel.textContent = `${label} (failed)`;
  } finally {
    switching = false;
  }
}

for (const button of Array.from(
  document.querySelectorAll<HTMLButtonElement>("button.launch"),
)) {
  button.addEventListener("click", () => {
    const launcher = LAUNCHERS[Number(button.dataset.idx)];
    void openSession(launcher.label, launcher.agent);
  });
}

const applyResize = (): void => {
  fitAddon.fit();
  if (currentId !== null) {
    void callNative("terminal_resize", {
      id: currentId,
      cols: term.cols,
      rows: term.rows,
    });
  }
};

window.addEventListener("resize", applyResize);
new ResizeObserver(applyResize).observe(termHost);

// Open a plain shell on boot.
void openSession("Shell");
