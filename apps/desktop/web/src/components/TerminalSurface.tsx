import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef } from "react";

import { host } from "../host/host";

type TerminalOutput = { id: number; data: string };
type TerminalExit = { id: number };

/** Decode base64 (the Rust output bridge) into the raw bytes xterm expects. */
function decodeBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/**
 * A live terminal surface: xterm.js + FitAddon over the ConPTY backend, wired
 * through the host bridge. Behavior parity with the Phase 0 vanilla-TS shell —
 * open a session, stream base64 output in, forward keystrokes + explicit
 * resizes out (no SIGWINCH on Windows).
 */
export function TerminalSurface(): React.JSX.Element {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }

    const term = new Terminal({
      cursorBlink: true,
      fontFamily:
        '"Cascadia Mono", "Cascadia Code", Consolas, "Courier New", monospace',
      fontSize: 14,
      theme: { background: "#0b0e14", foreground: "#e6e6e6" },
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(container);
    fitAddon.fit();

    // Effect bodies can't be async; track teardown work and cancellation so a
    // fast unmount (or StrictMode double-invoke) never leaks listeners.
    let disposed = false;
    const cleanups: Array<() => void> = [() => term.dispose()];

    async function boot(mount: HTMLDivElement): Promise<void> {
      const { cols, rows } = term;
      const sessionId = await host.invoke<number>("terminal_open", { cols, rows });
      if (disposed) {
        // Unmounted before the session opened; ask the backend to close it.
        void host.invoke("terminal_close", { id: sessionId }).catch(() => {});
        return;
      }

      cleanups.push(() => void host.invoke("terminal_close", { id: sessionId }).catch(() => {}));

      const offOutput = await host.on<TerminalOutput>(
        "cmux://terminal-output",
        (payload) => {
          if (payload.id === sessionId) {
            term.write(decodeBase64(payload.data));
          }
        },
      );
      cleanups.push(offOutput);

      const offExit = await host.on<TerminalExit>("cmux://terminal-exit", (payload) => {
        if (payload.id === sessionId) {
          term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
        }
      });
      cleanups.push(offExit);

      const dataSub = term.onData((data) => {
        void host.invoke("terminal_write", { id: sessionId, data });
      });
      cleanups.push(() => dataSub.dispose());

      const applyResize = (): void => {
        fitAddon.fit();
        void host.invoke("terminal_resize", {
          id: sessionId,
          cols: term.cols,
          rows: term.rows,
        });
      };
      window.addEventListener("resize", applyResize);
      cleanups.push(() => window.removeEventListener("resize", applyResize));

      const observer = new ResizeObserver(applyResize);
      observer.observe(mount);
      cleanups.push(() => observer.disconnect());

      term.focus();
    }

    boot(container).catch((error) => {
      const message = error instanceof Error ? error.message : String(error);
      term.write(`\r\n\x1b[31mFailed to start terminal: ${message}\x1b[0m\r\n`);
    });

    return () => {
      disposed = true;
      // Dispose in reverse order (listeners before the terminal itself).
      for (const cleanup of cleanups.reverse()) {
        cleanup();
      }
    };
  }, []);

  return <div ref={containerRef} className="cmux-terminal-surface" />;
}
