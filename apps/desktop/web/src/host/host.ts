import {
  callNative,
  invokeRaw as defaultInvokeRaw,
  listenNative,
} from "../tauri-bridge";

/**
 * The host bridge — the single seam between the React UI (and the reused
 * `cmux/webviews` surfaces) and the native backend.
 *
 * Two audiences, two shapes:
 *
 *  1. New Windows-shell code (terminal, workspace, settings) calls the generic
 *     {@link host} API: `host.invoke(channel, payload)` and
 *     `host.on(event, cb)`. These map straight onto Tauri commands/events and
 *     unwrap the `NativeReply` envelope for ergonomic call sites.
 *
 *  2. Reused `cmux/webviews` components (agent chat, diff comments, markdown)
 *     were written against a macOS **WKWebView** host: they call
 *     `window.webkit.messageHandlers.<name>.postMessage({ id, method, params })`
 *     and receive pushed events via `window.cmuxAgentBridge.receive(event)`. To
 *     run them unmodified we install shims that present exactly that shape,
 *     backed by Tauri — see {@link installMacHostShims}. A reused surface plugs
 *     in by swapping only its host adapter (this file), nothing else.
 */

/** The reply envelope every native RPC returns, mirroring the macOS contract. */
export type NativeReply<T = unknown> =
  | { ok: true; value: T }
  | { ok: false; error?: { code?: string; userMessage?: string } };

/** The `{ id, method, params }` message shape a WKWebView message handler receives. */
export interface HostMessage {
  id?: string;
  method: string;
  params?: Record<string, unknown>;
}

/**
 * Generic host API for new shell code. `invoke` unwraps the `NativeReply`
 * envelope (throwing `NativeBridgeError` on `{ ok: false }`); `on` subscribes to
 * a native event stream and returns an unlisten function.
 */
export const host = {
  invoke: callNative,
  on: listenNative,
};

/**
 * The macOS `webkit.messageHandlers` channel names the reused webviews call,
 * mapped to the Tauri command that services each one. The Rust command receives
 * a single `message` argument (`{ id, method, params }`) and returns a
 * `NativeReply`. The agent-session, diff-comments, and cmuxLib RPC commands are
 * now live in the desktop host; the shim keeps the reused webviews on their
 * original WKWebView-shaped contract.
 */
export const MAC_HOST_CHANNELS: Record<string, string> = {
  agentSession: "agent_session_rpc",
  cmuxDiffComments: "diff_comments_rpc",
  cmuxLib: "cmux_lib_rpc",
};

/**
 * The native→webview push channel. The Tauri backend emits agent/session/theme
 * events under this name; the shim forwards each payload to
 * `window.cmuxAgentBridge.receive`, matching how the macOS app evaluates
 * `window.cmuxAgentBridge.receive(event)` into the WKWebView.
 */
export const MAC_HOST_EVENT = "cmux://agent-event";

// Non-generic on purpose: the shim only ever needs `unknown` in/out, and a
// non-generic signature lets plain test doubles satisfy it (a concrete function
// cannot implement a generic call signature).
type InvokeRaw = (method: string, params?: Record<string, unknown>) => Promise<unknown>;
type Listen = (event: string, handler: (payload: unknown) => void) => Promise<() => void>;

export interface MacHostShimOptions {
  /** Raw (non-unwrapping) Tauri invoke. Injectable for tests. */
  invokeRaw?: InvokeRaw;
  /** Tauri event subscribe. Injectable for tests. */
  listen?: Listen;
  /** Channel → Tauri-command map override. */
  channels?: Record<string, string>;
  /** Native→webview push event name override. */
  eventName?: string;
}

/** Whether a raw invoke result already carries the `NativeReply` envelope. */
export function isEnvelope(value: unknown): value is NativeReply {
  return typeof value === "object" && value !== null && "ok" in value;
}

/** Map a transport rejection to the `{ ok: false, error }` reply shape. */
export function errorReply(error: unknown): NativeReply {
  const message = error instanceof Error ? error.message : String(error);
  const code = (error as { code?: string } | null)?.code;
  return { ok: false, error: { code, userMessage: message } };
}

interface MessageHandler {
  postMessage(message: HostMessage): Promise<NativeReply>;
}

interface WebkitBridgeWindow {
  webkit?: { messageHandlers?: Record<string, MessageHandler> };
  cmuxAgentBridge?: { receive(event: unknown): void; applyTheme?(theme: unknown): void };
}

let installedUnlisten: (() => void) | null = null;

/**
 * Install the macOS WKWebView host shims onto `window`, backed by Tauri.
 *
 * - For each channel in {@link MAC_HOST_CHANNELS} it creates
 *   `window.webkit.messageHandlers.<channel>` whose `postMessage(msg)` invokes
 *   the mapped Tauri command and resolves to the raw `NativeReply` envelope the
 *   reused bridge expects (never throwing — transport failures become
 *   `{ ok: false, error }`).
 * - It subscribes to the native push event and forwards each payload to
 *   `window.cmuxAgentBridge.receive`.
 *
 * Idempotent-ish: call {@link uninstallMacHostShims} first (HMR does this) to
 * drop a prior native subscription. Returns the resolved unlisten function.
 */
export async function installMacHostShims(
  options: MacHostShimOptions = {},
): Promise<() => void> {
  const invoke = options.invokeRaw ?? (defaultInvokeRaw as InvokeRaw);
  const listen = options.listen ?? (listenNative as Listen);
  const channels = options.channels ?? MAC_HOST_CHANNELS;
  const eventName = options.eventName ?? MAC_HOST_EVENT;

  const target = window as unknown as WebkitBridgeWindow;
  const webkit = (target.webkit ??= {});
  const handlers = (webkit.messageHandlers ??= {});

  for (const [channel, command] of Object.entries(channels)) {
    handlers[channel] = {
      async postMessage(message: HostMessage): Promise<NativeReply> {
        try {
          const reply = await invoke(command, { message });
          return isEnvelope(reply) ? reply : { ok: true, value: reply };
        } catch (error) {
          return errorReply(error);
        }
      },
    };
  }

  // Drop any prior native subscription before opening a new one so repeated
  // installs (dev HMR) don't fan the same event out twice.
  uninstallMacHostShims();

  const unlisten = await listen(eventName, (payload) => {
    target.cmuxAgentBridge?.receive(payload);
  });
  installedUnlisten = unlisten;
  return unlisten;
}

/** Tear down the native push subscription opened by {@link installMacHostShims}. */
export function uninstallMacHostShims(): void {
  if (installedUnlisten) {
    const unlisten = installedUnlisten;
    installedUnlisten = null;
    unlisten();
  }
}
