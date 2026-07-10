import { invokeRaw as defaultInvokeRaw } from "../tauri-bridge";
import { errorReply, isEnvelope, type HostMessage, type NativeReply } from "./host";

/**
 * The parent↔iframe postMessage relay for `diff_comments_rpc`.
 *
 * On macOS `Sources/Panels/DiffCommentsBridge.swift` is a WKScriptMessageHandler
 * ON the diff-viewer WKWebView, so `webkit.messageHandlers.cmuxDiffComments`
 * exists natively inside the viewer document. The Windows port hosts the viewer
 * as a sandboxed cross-origin IFRAME (`cmux-diff-viewer://<token>/index.html`
 * inside `http://tauri.localhost`), so `installMacHostShims` never runs there
 * and Tauri IPC is not exposed to that origin. The frozen
 * `webviews/src/comments/bridge.ts` still requires
 * `window.webkit.messageHandlers.cmuxDiffComments.postMessage(msg) -> Promise<NativeReply>`
 * inside the iframe.
 *
 * Both halves of the relay live in this one module so the wire-protocol
 * constants can never drift:
 *
 *  - Host half ({@link installDiffCommentsRelay}) runs in the MAIN document:
 *    listens for request envelopes from the viewer iframe, forwards the raw
 *    `{ id?, method, params? }` message to the `diff_comments_rpc` Tauri
 *    command, and posts the raw `NativeReply` envelope back to the sender.
 *  - Guest half ({@link installDiffCommentsGuestShim}) runs inside the
 *    diff-viewer document. DELIVERY into the served viewer bundle is G7's job
 *    (the bundle entry imports/inlines it before `webviews/src/comments/bridge.ts`
 *    runs its `diffCommentsBridgeAvailable()` probe); it lands here so host and
 *    guest compile against the same protocol constants.
 *
 * The request payload carries no secrets — the trust gate stays Rust-side
 * (`diff.rs` token → `DiffSessionRegistry`). The host half threads the diff
 * session token and hosting panel id alongside the untouched bridge message so
 * Rust can trust-gate iframe requests and register pending review comments
 * against the correct workspace.
 */

/** `type` tag of a guest→parent relay request envelope. */
export const DIFF_COMMENTS_REQUEST_TYPE = "cmux-diff-comments-request";

/** `type` tag of a parent→guest relay reply envelope. */
export const DIFF_COMMENTS_REPLY_TYPE = "cmux-diff-comments-reply";

/** The Tauri command servicing the relay (mirrors `MAC_HOST_CHANNELS.cmuxDiffComments`). */
export const DIFF_COMMENTS_COMMAND = "diff_comments_rpc";

/** The WebView2 rewrite origin of the diff viewer (`cmux-diff-viewer://…` → this). */
export const DIFF_VIEWER_HTTP_ORIGIN = "http://cmux-diff-viewer.localhost";

/** The custom-scheme origin prefix, in case WebView2 reports the unrewritten form. */
export const DIFF_VIEWER_SCHEME_PREFIX = "cmux-diff-viewer://";

/** Guest→parent envelope: the untouched bridge.ts message plus routing data. */
export interface DiffCommentsRelayRequest {
  type: typeof DIFF_COMMENTS_REQUEST_TYPE;
  relayId: string;
  message: HostMessage;
}

/** Parent→guest envelope: the raw `NativeReply` for the matching `relayId`. */
export interface DiffCommentsRelayReply {
  type: typeof DIFF_COMMENTS_REPLY_TYPE;
  relayId: string;
  reply: NativeReply;
}

/**
 * Default origin filter: accepts both forms the viewer iframe can present —
 * the WebView2 rewrite (`http://cmux-diff-viewer.localhost`) and the raw
 * custom scheme (`cmux-diff-viewer://<token>`) — the same dual-form rationale
 * as `token_from_diff_viewer_url` in diff.rs. Which literal string a real
 * WebView2 delivers is a documented GUI-tail question.
 */
export function defaultAllowDiffViewerOrigin(origin: string): boolean {
  return origin === DIFF_VIEWER_HTTP_ORIGIN || origin.startsWith(DIFF_VIEWER_SCHEME_PREFIX);
}

// Non-generic on purpose (same rationale as host.ts): the relay only needs
// `unknown` in/out, and plain test doubles cannot implement a generic call
// signature.
type InvokeRaw = (method: string, params?: Record<string, unknown>) => Promise<unknown>;

/** The slice of a MessageEvent the relay reads — a plain object in tests. */
export interface RelayMessageEvent {
  data: unknown;
  origin: string;
  source?: { postMessage(message: unknown, targetOrigin: string): void } | null;
}

/** addEventListener/removeEventListener-shaped seam standing in for `window`. */
export interface RelayWindowTarget {
  addEventListener(type: "message", listener: (event: RelayMessageEvent) => void): void;
  removeEventListener(type: "message", listener: (event: RelayMessageEvent) => void): void;
}

export interface DiffCommentsRelayOptions {
  /** Raw (non-unwrapping) Tauri invoke. Injectable for tests. */
  invokeRaw?: InvokeRaw;
  /** The window to listen on. Injectable for tests. */
  windowTarget?: RelayWindowTarget;
  /** Origin filter for incoming requests. Defaults to {@link defaultAllowDiffViewerOrigin}. */
  allowOrigin?: (origin: string) => boolean;
  /** Diff session token for the iframe whose messages this relay forwards. */
  token?: string | null;
  /** Pane id hosting the diff iframe, used for workspace-scoped pending comments. */
  panelId?: string | null;
}

function isRelayRequest(data: unknown): data is DiffCommentsRelayRequest {
  if (typeof data !== "object" || data === null) {
    return false;
  }
  const candidate = data as Partial<DiffCommentsRelayRequest>;
  return (
    candidate.type === DIFF_COMMENTS_REQUEST_TYPE &&
    typeof candidate.relayId === "string" &&
    typeof candidate.message === "object" &&
    candidate.message !== null
  );
}

function isRelayReply(data: unknown): data is DiffCommentsRelayReply {
  if (typeof data !== "object" || data === null) {
    return false;
  }
  const candidate = data as Partial<DiffCommentsRelayReply>;
  return (
    candidate.type === DIFF_COMMENTS_REPLY_TYPE &&
    typeof candidate.relayId === "string" &&
    isEnvelope(candidate.reply)
  );
}

let installedUninstall: (() => void) | null = null;

/**
 * Install the host half of the relay on the main document's window.
 *
 * SINGLETON: a second install tears down the prior listener first, so several
 * mounted DiffSurfaces (or dev HMR) never double-invoke / double-reply. For
 * accepted requests it invokes `diff_comments_rpc` with the untouched message
 * and ALWAYS posts a reply envelope back to the sender frame scoped to its
 * origin — a non-envelope result is wrapped `{ ok: true, value }`, a transport
 * rejection becomes `{ ok: false, error }` (host.ts semantics; never throws).
 * Events with malformed data, a disallowed origin, or no postMessage-capable
 * source are ignored. Returns the unlisten.
 */
export function installDiffCommentsRelay(options: DiffCommentsRelayOptions = {}): () => void {
  const invoke = options.invokeRaw ?? (defaultInvokeRaw as InvokeRaw);
  const target = options.windowTarget ?? (window as unknown as RelayWindowTarget);
  const allowOrigin = options.allowOrigin ?? defaultAllowDiffViewerOrigin;

  uninstallDiffCommentsRelay();

  const listener = (event: RelayMessageEvent): void => {
    if (!isRelayRequest(event.data)) {
      return;
    }
    if (!allowOrigin(event.origin)) {
      return;
    }
    const source = event.source;
    if (source == null || typeof source.postMessage !== "function") {
      return;
    }
    const { relayId, message } = event.data;
    void (async () => {
      let reply: NativeReply;
      try {
        const params: Record<string, unknown> = { message };
        if (typeof options.token === "string" && options.token.trim() !== "") {
          params.token = options.token;
        }
        if (typeof options.panelId === "string" && options.panelId.trim() !== "") {
          params.panelId = options.panelId;
        }
        const result = await invoke(DIFF_COMMENTS_COMMAND, params);
        reply = isEnvelope(result) ? result : { ok: true, value: result };
      } catch (error) {
        reply = errorReply(error);
      }
      const envelope: DiffCommentsRelayReply = {
        type: DIFF_COMMENTS_REPLY_TYPE,
        relayId,
        reply,
      };
      source.postMessage(envelope, event.origin);
    })();
  };

  target.addEventListener("message", listener);
  const uninstall = (): void => {
    target.removeEventListener("message", listener);
    if (installedUninstall === uninstall) {
      installedUninstall = null;
    }
  };
  installedUninstall = uninstall;
  return uninstall;
}

/** Tear down the relay listener opened by {@link installDiffCommentsRelay}. */
export function uninstallDiffCommentsRelay(): void {
  if (installedUninstall) {
    const uninstall = installedUninstall;
    installedUninstall = null;
    uninstall();
  }
}

interface GuestMessageHandler {
  postMessage(message: HostMessage): Promise<NativeReply>;
}

/** The slice of the viewer document's `window` the guest shim touches. */
export interface DiffCommentsGuestWindow {
  parent: { postMessage(message: unknown, targetOrigin: string): void };
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
  webkit?: { messageHandlers?: Record<string, GuestMessageHandler> };
}

export interface DiffCommentsGuestShimOptions {
  /** Per-call reply deadline before the bridge_unavailable fallback resolves. */
  timeoutMs?: number;
}

/**
 * Timeout fallback reply. Defensive divergence from canonical: the macOS
 * WKScriptMessageHandlerWithReply always replies, but here a missing parent
 * relay would otherwise hang bridge.ts's promise forever.
 */
function bridgeUnavailableReply(): NativeReply {
  return {
    ok: false,
    error: { code: "bridge_unavailable", userMessage: "Diff comments bridge is unavailable." },
  };
}

/**
 * Install the guest half inside the diff-viewer document (or a window-shaped
 * test double): presents the WKWebView `webkit.messageHandlers.cmuxDiffComments`
 * shape the frozen bridge.ts probes for. Each `postMessage` allocates a
 * `relayId`, posts the request envelope to `parent` (target `"*"` — the guest
 * cannot know the dev-vs-prod host origin; the payload carries no secrets),
 * and resolves the matching reply from one shared `"message"` listener keyed
 * by `relayId`. An unanswered call resolves the bridge_unavailable envelope
 * after `timeoutMs` (default 10s) instead of hanging.
 */
export function installDiffCommentsGuestShim(
  guestWindow: DiffCommentsGuestWindow,
  options: DiffCommentsGuestShimOptions = {},
): void {
  const timeoutMs = options.timeoutMs ?? 10_000;
  const pending = new Map<string, (reply: NativeReply) => void>();
  let nextRelayId = 0;

  guestWindow.addEventListener("message", (event) => {
    if (!isRelayReply(event.data)) {
      return;
    }
    const settle = pending.get(event.data.relayId);
    if (!settle) {
      return;
    }
    pending.delete(event.data.relayId);
    settle(event.data.reply);
  });

  const webkit = (guestWindow.webkit ??= {});
  const handlers = (webkit.messageHandlers ??= {});
  handlers.cmuxDiffComments = {
    postMessage(message: HostMessage): Promise<NativeReply> {
      nextRelayId += 1;
      const relayId = `diff-comments-${nextRelayId}`;
      return new Promise((resolve) => {
        const timer = setTimeout(() => {
          if (pending.delete(relayId)) {
            resolve(bridgeUnavailableReply());
          }
        }, timeoutMs);
        pending.set(relayId, (reply) => {
          clearTimeout(timer);
          resolve(reply);
        });
        const request: DiffCommentsRelayRequest = {
          type: DIFF_COMMENTS_REQUEST_TYPE,
          relayId,
          message,
        };
        guestWindow.parent.postMessage(request, "*");
      });
    },
  };
}
