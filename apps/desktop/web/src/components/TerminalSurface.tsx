import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type ILink, type ILinkProvider } from "@xterm/xterm";
import { useEffect, useRef, useState, type DragEvent } from "react";

import { host } from "../host/host";

type TerminalOutput = { id: number; data: string };
type TerminalExit = { id: number };

export type TerminalCommand = "sendCtrlF" | "clearScreenKeepScrollback";
export type TerminalFindCommand =
  | "find"
  | "findNext"
  | "findPrevious"
  | "hideFind"
  | "useSelectionForFind";
export type TerminalTextBoxCommand =
  | "toggleTextBoxInput"
  | "focusTextBoxInput"
  | "attachTextBoxFile";

const TERMINAL_COMMAND_EVENT = "cmux:terminal-command";

export interface TerminalCommandDetail {
  panelId: string;
  command: TerminalCommand | TerminalFindCommand | TerminalTextBoxCommand;
}

export function dispatchTerminalCommand(
  panelId: string,
  command: TerminalCommand | TerminalFindCommand | TerminalTextBoxCommand,
): void {
  if (typeof window === "undefined") {
    return;
  }
  window.dispatchEvent(
    new CustomEvent<TerminalCommandDetail>(TERMINAL_COMMAND_EVENT, {
      detail: { panelId, command },
    }),
  );
}

export function findTerminalLineIndex(
  lines: string[],
  query: string,
  startIndex: number,
  direction: "next" | "previous",
): number | null {
  const needle = query.trim().toLowerCase();
  if (needle === "" || lines.length === 0) {
    return null;
  }
  const normalizedStart =
    ((Math.trunc(startIndex) % lines.length) + lines.length) % lines.length;
  for (let offset = 0; offset < lines.length; offset += 1) {
    const index =
      direction === "next"
        ? (normalizedStart + offset) % lines.length
        : (normalizedStart - offset + lines.length) % lines.length;
    if (lines[index]?.toLowerCase().includes(needle)) {
      return index;
    }
  }
  return null;
}

export interface PickedTextBoxFile {
  label?: string;
  path?: string;
  fsPath?: string;
}

interface PickedTextBoxFiles {
  files?: PickedTextBoxFile[];
}

interface TerminalDropFile {
  path?: string;
  name?: string;
}

interface TerminalDropDataTransfer {
  files?: ArrayLike<TerminalDropFile>;
  getData?: (format: string) => string;
}

export function appendAttachedFilePaths(
  draft: string,
  files: readonly PickedTextBoxFile[],
): string {
  const paths = files
    .map((file) => file.fsPath ?? file.path ?? "")
    .filter((path) => path.trim() !== "");
  if (paths.length === 0) {
    return draft;
  }
  const separator = draft === "" || draft.endsWith("\n") ? "" : "\n";
  return `${draft}${separator}${paths.join("\n")}`;
}

export function fileUrlToTerminalPath(rawUrl: string): string | null {
  const trimmed = rawUrl.trim();
  if (!/^file:/i.test(trimmed)) {
    return null;
  }
  try {
    const url = new URL(trimmed);
    if (url.protocol !== "file:") {
      return null;
    }
    const decodedPath = decodeURIComponent(url.pathname);
    if (/^\/[a-zA-Z]:\//.test(decodedPath)) {
      return decodedPath.slice(1).replace(/\//g, "\\");
    }
    if (url.hostname !== "") {
      return `\\\\${url.hostname}${decodedPath.replace(/\//g, "\\")}`;
    }
    return decodedPath;
  } catch {
    return null;
  }
}

export function terminalDropPathsFromDataTransfer(
  dataTransfer: TerminalDropDataTransfer,
): string[] {
  const paths: string[] = [];
  const addPath = (value: string | null | undefined): void => {
    const path = value?.trim();
    if (path != null && path !== "" && !paths.includes(path)) {
      paths.push(path);
    }
  };

  for (const file of Array.from(dataTransfer.files ?? [])) {
    addPath(file.path);
  }

  const uriList = dataTransfer.getData?.("text/uri-list") ?? "";
  for (const line of uriList.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("#")) {
      continue;
    }
    addPath(fileUrlToTerminalPath(trimmed));
  }

  const plain = dataTransfer.getData?.("text/plain") ?? "";
  for (const line of plain.split(/\r?\n/)) {
    addPath(fileUrlToTerminalPath(line));
  }

  return paths;
}

export function terminalDropPayloadFromDataTransfer(
  dataTransfer: TerminalDropDataTransfer,
): string | null {
  const paths = terminalDropPathsFromDataTransfer(dataTransfer);
  if (paths.length === 0) {
    return null;
  }
  return paths.map(quoteTerminalPath).join(" ");
}

function quoteTerminalPath(path: string): string {
  if (!/[\s"`]/.test(path)) {
    return path;
  }
  return `"${path.replace(/(["`])/g, "`$1")}"`;
}

export function textBoxSendPayload(draft: string): string | null {
  if (draft.trim() === "") {
    return null;
  }
  return draft.endsWith("\n") ? draft : `${draft}\r`;
}

export interface TerminalUrlLink {
  text: string;
  range: ILink["range"];
}

const TERMINAL_URL_PATTERN = /https?:\/\/[^\s<>"'`]+/gi;
const TERMINAL_URL_TRAILING_PUNCTUATION = /[),.;:!?]+$/;

export function terminalUrlLinksForLine(
  bufferLineNumber: number,
  line: string,
): TerminalUrlLink[] {
  const links: TerminalUrlLink[] = [];
  for (const match of line.matchAll(TERMINAL_URL_PATTERN)) {
    const raw = match[0] ?? "";
    const index = match.index ?? 0;
    const text = raw.replace(TERMINAL_URL_TRAILING_PUNCTUATION, "");
    if (text === "") {
      continue;
    }
    links.push({
      text,
      range: {
        start: { x: index + 1, y: bufferLineNumber + 1 },
        end: { x: index + text.length, y: bufferLineNumber + 1 },
      },
    });
  }
  return links;
}

export function shouldOpenTerminalLinkInCmuxBrowser(
  event: Pick<MouseEvent, "ctrlKey" | "metaKey">,
): boolean {
  return event.ctrlKey || event.metaKey;
}

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
 *
 * The `panelId` binds this surface to its workspace: an OSC title change from
 * the shell (xterm `onTitleChange`) is fed to `session_set_process_title`, which
 * sets the owning workspace's `process_title` — the terminal top-label feed that
 * replaces the "Terminal" fallback in the sidebar/switcher with the running
 * program / directory.
 */
export function TerminalSurface({
  panelId,
  cwd,
  initialCommand,
  initialInput,
  environment,
  isActive = false,
  onOpenLinkInBrowser,
}: {
  panelId: string;
  cwd?: string;
  initialCommand?: string;
  initialInput?: string;
  environment?: Record<string, string>;
  isActive?: boolean;
  onOpenLinkInBrowser?: (url: string) => void;
}): React.JSX.Element {
  const containerRef = useRef<HTMLDivElement | null>(null);
  // Keep the latest panelId reachable from the title handler without making the
  // terminal-lifecycle effect depend on it (a panelId change must never remount
  // the surface — that would kill the ConPTY shell).
  const panelIdRef = useRef(panelId);
  panelIdRef.current = panelId;
  const cwdRef = useRef(cwd);
  cwdRef.current = cwd;
  const initialCommandRef = useRef(initialCommand);
  initialCommandRef.current = initialCommand;
  const initialInputRef = useRef(initialInput);
  initialInputRef.current = initialInput;
  const environmentRef = useRef(environment);
  environmentRef.current = environment;
  const onOpenLinkInBrowserRef = useRef(onOpenLinkInBrowser);
  onOpenLinkInBrowserRef.current = onOpenLinkInBrowser;
  const terminalRef = useRef<Terminal | null>(null);
  const sessionIdRef = useRef<number | null>(null);
  const findInputRef = useRef<HTMLInputElement | null>(null);
  const textBoxRef = useRef<HTMLTextAreaElement | null>(null);
  const [findVisible, setFindVisible] = useState(false);
  const [findDraft, setFindDraft] = useState("");
  const findDraftRef = useRef(findDraft);
  findDraftRef.current = findDraft;
  const [findStatus, setFindStatus] = useState("");
  const lastFindLineRef = useRef(0);
  const [textBoxVisible, setTextBoxVisible] = useState(false);
  const [textBoxDraft, setTextBoxDraft] = useState("");
  const textBoxDraftRef = useRef(textBoxDraft);
  textBoxDraftRef.current = textBoxDraft;
  const [textBoxStatus, setTextBoxStatus] = useState("");
  const [terminalStarting, setTerminalStarting] = useState(true);

  const runFind = (direction: "next" | "previous", query = findDraftRef.current): void => {
    const term = terminalRef.current;
    if (!term) {
      return;
    }
    const buffer = term.buffer.active;
    const lines = Array.from({ length: buffer.length }, (_, index) =>
      buffer.getLine(index)?.translateToString(true) ?? "",
    );
    const startLine =
      direction === "next" ? lastFindLineRef.current + 1 : lastFindLineRef.current - 1;
    const match = findTerminalLineIndex(lines, query, startLine, direction);
    if (match === null) {
      setFindStatus(query.trim() === "" ? "" : "No results");
      return;
    }
    lastFindLineRef.current = match;
    term.scrollToLine(match);
    setFindStatus(`Line ${match + 1}`);
    term.focus();
  };

  const showFind = (select = true): void => {
    setFindVisible(true);
    window.setTimeout(() => {
      const input = findInputRef.current;
      input?.focus();
      if (select) {
        input?.select();
      }
    }, 0);
  };

  const focusTextBox = (): void => {
    setTextBoxVisible(true);
    window.setTimeout(() => textBoxRef.current?.focus(), 0);
  };

  const attachTextBoxFiles = (): void => {
    focusTextBox();
    void host
      .invoke<PickedTextBoxFiles>("pick_textbox_files")
      .then((reply) => {
        const files = reply.files ?? [];
        setTextBoxDraft((current) => appendAttachedFilePaths(current, files));
        setTextBoxStatus(
          files.length === 0
            ? "No files attached"
            : `Attached ${files.length} file${files.length === 1 ? "" : "s"}`,
        );
      })
      .catch((error) => {
        console.error("pick_textbox_files failed", error);
        setTextBoxStatus("Unable to attach files");
      });
  };

  const sendTextBoxDraft = (draft = textBoxDraftRef.current): void => {
    const payload = textBoxSendPayload(draft);
    if (payload === null) {
      setTextBoxStatus("Nothing to send");
      return;
    }
    const sessionId = sessionIdRef.current;
    if (sessionId === null) {
      setTextBoxStatus("Terminal not ready");
      return;
    }
    void host.invoke("terminal_write", { id: sessionId, data: payload });
    setTextBoxDraft("");
    setTextBoxStatus("Sent");
    terminalRef.current?.focus();
  };

  const handleTerminalDrop = (event: DragEvent<HTMLDivElement>): void => {
    const payload = terminalDropPayloadFromDataTransfer(event.dataTransfer);
    if (payload === null) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    const sessionId = sessionIdRef.current;
    if (sessionId !== null) {
      void host.invoke("terminal_write", { id: sessionId, data: payload });
    }
    terminalRef.current?.focus();
  };

  useEffect(() => {
    if (!isActive || findVisible || textBoxVisible) {
      return;
    }
    const timer = window.setTimeout(() => {
      terminalRef.current?.focus();
    }, 0);
    return () => window.clearTimeout(timer);
  }, [findVisible, isActive, textBoxVisible]);

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
    terminalRef.current = term;
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(container);
    fitAddon.fit();

    // Effect bodies can't be async; track teardown work and cancellation so a
    // fast unmount (or StrictMode double-invoke) never leaks listeners.
    let disposed = false;
    const cleanups: Array<() => void> = [() => term.dispose()];

    if (onOpenLinkInBrowserRef.current !== undefined) {
      const linkProvider: ILinkProvider = {
        provideLinks(bufferLineNumber, callback) {
          const line =
            term.buffer.active
              .getLine(bufferLineNumber)
              ?.translateToString(true) ?? "";
          const links = terminalUrlLinksForLine(bufferLineNumber, line).map(
            (link): ILink => ({
              ...link,
              decorations: { pointerCursor: true, underline: true },
              activate(event, text) {
                if (!shouldOpenTerminalLinkInCmuxBrowser(event)) {
                  return;
                }
                event.preventDefault();
                event.stopPropagation();
                onOpenLinkInBrowserRef.current?.(text);
              },
            }),
          );
          callback(links.length === 0 ? undefined : links);
        },
      };
      const linkProviderDisposable = term.registerLinkProvider(linkProvider);
      cleanups.push(() => linkProviderDisposable.dispose());
    }

    // Feed the shell's OSC title into the owning workspace's process_title. The
    // subscription is independent of the async session boot, so register it now;
    // dedupe so a repeated title never emits a redundant session-changed.
    let lastSentTitle = "";
    const titleSub = term.onTitleChange((title) => {
      const next = title.trim();
      if (next === "" || next === lastSentTitle) {
        return;
      }
      lastSentTitle = next;
      void host
        .invoke("session_set_process_title", { panelId: panelIdRef.current, title: next })
        .catch(() => {});
    });
    cleanups.push(() => titleSub.dispose());

    async function boot(mount: HTMLDivElement): Promise<void> {
      const { cols, rows } = term;
      const sessionId = await host.invoke<number>("terminal_open", {
        cols,
        rows,
        panelId: panelIdRef.current,
        cwd: cwdRef.current,
        initialCommand: initialCommandRef.current,
        initialInput: initialInputRef.current,
        environment: environmentRef.current,
      });
      sessionIdRef.current = sessionId;
      if (disposed) {
        // Unmounted before the session opened; ask the backend to close it.
        void host.invoke("terminal_close", { id: sessionId }).catch(() => {});
        return;
      }
      setTerminalStarting(false);

      cleanups.push(() => void host.invoke("terminal_close", { id: sessionId }).catch(() => {}));

      const portScanOffsetsMs = [500, 1500, 3000, 5000, 7500, 10000];
      const portScanTimers = new Set<number>();
      let portScanBurstActive = false;
      let portScanPending = false;
      const runPortScan = (): void => {
        void host.invoke("terminal_scan_listening_ports", { id: sessionId }).catch(() => {});
      };
      const clearPortScanTimers = (): void => {
        for (const timer of portScanTimers) {
          window.clearTimeout(timer);
        }
        portScanTimers.clear();
      };
      cleanups.push(clearPortScanTimers);
      const requestPortScanBurst = (): void => {
        if (disposed) {
          return;
        }
        if (portScanBurstActive) {
          portScanPending = true;
          return;
        }
        portScanBurstActive = true;
        portScanPending = false;
        portScanOffsetsMs.forEach((offset, index) => {
          const timer = window.setTimeout(() => {
            portScanTimers.delete(timer);
            runPortScan();
            if (index === portScanOffsetsMs.length - 1) {
              portScanBurstActive = false;
              if (portScanPending) {
                requestPortScanBurst();
              }
            }
          }, offset);
          portScanTimers.add(timer);
        });
      };
      requestPortScanBurst();

      const offOutput = await host.on<TerminalOutput>(
        "cmux://terminal-output",
        (payload) => {
          if (payload.id === sessionId) {
            term.write(decodeBase64(payload.data));
            requestPortScanBurst();
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

      const onTerminalCommand = (event: Event): void => {
        const detail = (event as CustomEvent<TerminalCommandDetail>).detail;
        if (detail?.panelId !== panelIdRef.current) {
          return;
        }
        switch (detail.command) {
          case "sendCtrlF":
            term.focus();
            void host.invoke("terminal_write", { id: sessionId, data: "\x06" });
            break;
          case "clearScreenKeepScrollback":
            term.focus();
            term.write("\x1b[H\x1b[2J");
            break;
          case "find":
            showFind();
            break;
          case "findNext":
            setFindVisible(true);
            runFind("next");
            break;
          case "findPrevious":
            setFindVisible(true);
            runFind("previous");
            break;
          case "hideFind":
            setFindVisible(false);
            setFindStatus("");
            term.focus();
            break;
          case "useSelectionForFind": {
            const selection = term.getSelection().trim();
            if (selection !== "") {
              setFindDraft(selection);
              findDraftRef.current = selection;
              setFindVisible(true);
              runFind("next", selection);
            } else {
              showFind(false);
            }
            break;
          }
          case "toggleTextBoxInput":
            setTextBoxVisible((visible) => {
              const next = !visible;
              if (next) {
                window.setTimeout(() => textBoxRef.current?.focus(), 0);
              } else {
                term.focus();
              }
              return next;
            });
            break;
          case "focusTextBoxInput":
            focusTextBox();
            break;
          case "attachTextBoxFile":
            attachTextBoxFiles();
            break;
        }
      };
      window.addEventListener(TERMINAL_COMMAND_EVENT, onTerminalCommand);
      cleanups.push(() =>
        window.removeEventListener(TERMINAL_COMMAND_EVENT, onTerminalCommand),
      );

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
      setTerminalStarting(false);
      const message = error instanceof Error ? error.message : String(error);
      term.write(`\r\n\x1b[31mFailed to start terminal: ${message}\x1b[0m\r\n`);
    });

    return () => {
      disposed = true;
      terminalRef.current = null;
      sessionIdRef.current = null;
      // Dispose in reverse order (listeners before the terminal itself).
      for (const cleanup of cleanups.reverse()) {
        cleanup();
      }
    };
  }, []);

  return (
    <div
      className="cmux-terminal-shell"
      onDragOver={(event) => {
        if (terminalDropPayloadFromDataTransfer(event.dataTransfer) !== null) {
          event.preventDefault();
        }
      }}
      onDrop={handleTerminalDrop}
    >
      <div className="cmux-terminal-toolbar" aria-label="Terminal controls">
        <button
          type="button"
          className="cmux-terminal-tool"
          aria-label="Find in Terminal"
          onClick={() => dispatchTerminalCommand(panelId, "find")}
        >
          Find
        </button>
        <button
          type="button"
          className="cmux-terminal-tool"
          aria-label="Next Terminal Match"
          onClick={() => dispatchTerminalCommand(panelId, "findNext")}
        >
          Next
        </button>
        <button
          type="button"
          className="cmux-terminal-tool"
          aria-label="Terminal Text Box Input"
          onClick={() => dispatchTerminalCommand(panelId, "toggleTextBoxInput")}
        >
          Text
        </button>
        <button
          type="button"
          className="cmux-terminal-tool"
          aria-label="Attach File to Terminal Text Box"
          onClick={() => dispatchTerminalCommand(panelId, "attachTextBoxFile")}
        >
          Attach
        </button>
        <button
          type="button"
          className="cmux-terminal-tool"
          aria-label="Clear Terminal Screen"
          onClick={() =>
            dispatchTerminalCommand(panelId, "clearScreenKeepScrollback")
          }
        >
          Clear
        </button>
        <button
          type="button"
          className="cmux-terminal-tool"
          aria-label="Send Ctrl-F to Terminal"
          onClick={() => dispatchTerminalCommand(panelId, "sendCtrlF")}
        >
          Ctrl-F
        </button>
      </div>
      {textBoxVisible ? (
        <form
          className="cmux-terminal-textbox"
          onSubmit={(event) => {
            event.preventDefault();
            sendTextBoxDraft();
          }}
        >
          <textarea
            ref={textBoxRef}
            className="cmux-terminal-textbox-input"
            value={textBoxDraft}
            placeholder="Type terminal input. Ctrl+Enter sends."
            spellCheck={false}
            onChange={(event) => {
              setTextBoxDraft(event.currentTarget.value);
              setTextBoxStatus("");
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
                event.preventDefault();
                sendTextBoxDraft(textBoxDraft);
              }
            }}
          />
          <div className="cmux-terminal-textbox-actions">
            <span className="cmux-terminal-textbox-status">{textBoxStatus}</span>
            <button
              type="button"
              onClick={() => {
                attachTextBoxFiles();
              }}
            >
              Attach
            </button>
            <button
              type="button"
              onClick={() => {
                sendTextBoxDraft(textBoxDraft);
              }}
            >
              Send
            </button>
            <button
              type="button"
              onClick={() => {
                setTextBoxVisible(false);
                setTextBoxStatus("");
                terminalRef.current?.focus();
              }}
            >
              Close
            </button>
          </div>
        </form>
      ) : null}
      {findVisible ? (
        <form
          className="cmux-terminal-find"
          onSubmit={(event) => {
            event.preventDefault();
            runFind("next");
          }}
        >
          <input
            ref={findInputRef}
            className="cmux-terminal-find-input"
            value={findDraft}
            placeholder="Find in terminal"
            spellCheck={false}
            onChange={(event) => {
              setFindDraft(event.currentTarget.value);
              setFindStatus("");
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                setFindVisible(false);
                setFindStatus("");
                terminalRef.current?.focus();
              }
            }}
          />
          <button type="button" onClick={() => runFind("previous")}>
            Previous
          </button>
          <button type="submit">Next</button>
          <span className="cmux-terminal-find-status">{findStatus}</span>
          <button
            type="button"
            aria-label="Hide Find Bar"
            onClick={() => {
              setFindVisible(false);
              setFindStatus("");
              terminalRef.current?.focus();
            }}
          >
            Close
          </button>
        </form>
      ) : null}
      {terminalStarting ? (
        <div
          className="cmux-terminal-loading"
          role="status"
          aria-live="polite"
        >
          <span className="cmux-terminal-loading-spinner" aria-hidden="true" />
          <span>Starting terminal...</span>
        </div>
      ) : null}
      <div ref={containerRef} className="cmux-terminal-surface" />
    </div>
  );
}
