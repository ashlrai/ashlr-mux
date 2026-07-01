type NativeReply<T> =
  | T
  | { ok: true; value: T }
  | { ok: false; error?: { code?: string; userMessage?: string } };

type AgentEvent = {
  type: string;
  payload?: unknown;
};

type EventListener = (event: AgentEvent) => void;

type TauriWindow = {
  core?: {
    invoke?: <T>(command: string, payload?: Record<string, unknown>) => Promise<T>;
  };
  event?: {
    listen?: (
      name: string,
      callback: (event: { payload: unknown }) => void,
    ) => Promise<() => void>;
  };
};

declare global {
  interface Window {
    __TAURI__?: TauriWindow;
  }
}

const listeners = new Set<EventListener>();
let nativeEventUnlisten: (() => void) | null = null;
let nativeEventSubscription: Promise<(() => void) | null> | null = null;

export class NativeBridgeError extends Error {
  readonly code?: string;

  constructor(message: string, code?: string) {
    super(message);
    this.name = "NativeBridgeError";
    this.code = code;
  }
}

export async function callNative<T>(
  method: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) {
    if (method === "ping") {
      return "pong (browser fallback)" as T;
    }
    throw new NativeBridgeError("Tauri bridge is unavailable in the current runtime.");
  }

  const reply = (await invoke<NativeReply<T>>(method, params)) as NativeReply<T>;
  if (typeof reply !== "object" || reply === null || !("ok" in reply)) {
    return reply as T;
  }

  if (!reply.ok) {
    throw new NativeBridgeError(
      reply.error?.userMessage || "Native bridge request failed.",
      reply.error?.code,
    );
  }

  return reply.value;
}

/**
 * Invoke a native Tauri command and return its reply verbatim, WITHOUT the
 * `NativeReply` envelope-unwrapping that {@link callNative} performs. The macOS
 * `webkit.messageHandlers.*` contract requires the raw `{ ok, value | error }`
 * envelope to reach the reused `webviews/` bridges (they unwrap it themselves),
 * so the host shim in `host/` routes through this instead of `callNative`.
 *
 * In a plain-browser runtime (no Tauri) it throws {@link NativeBridgeError}.
 */
export async function invokeRaw<T>(
  method: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) {
    throw new NativeBridgeError("Tauri bridge is unavailable in the current runtime.");
  }
  return invoke<T>(method, params);
}

/**
 * Subscribe to a native Tauri event by name. Returns an unlisten function; in a
 * plain-browser runtime (no Tauri) it resolves to a no-op so the caller can run
 * unconditionally.
 */
export async function listenNative<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  const listen = window.__TAURI__?.event?.listen;
  if (!listen) {
    return () => {};
  }
  return listen(event, (received) => handler(received.payload as T));
}

function fanOutAgentEvent(event: AgentEvent): void {
  for (const handler of listeners) {
    handler(event);
  }
}

async function ensureNativeEventSubscription(): Promise<(() => void) | null> {
  if (nativeEventUnlisten) {
    return nativeEventUnlisten;
  }

  if (nativeEventSubscription) {
    return nativeEventSubscription;
  }

  const tauriListen = window.__TAURI__?.event?.listen;
  if (!tauriListen) {
    return null;
  }

  nativeEventSubscription = tauriListen("cmux://agent-event", (event) => {
    fanOutAgentEvent(event.payload as AgentEvent);
  })
    .then((unlisten) => {
      nativeEventUnlisten = () => {
        nativeEventUnlisten = null;
        unlisten();
      };
      return nativeEventUnlisten;
    })
    .finally(() => {
      nativeEventSubscription = null;
    });

  return nativeEventSubscription;
}

export async function subscribeToAgentEvents(listener: EventListener): Promise<() => void> {
  listeners.add(listener);
  await ensureNativeEventSubscription();

  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && nativeEventUnlisten) {
      const unlisten = nativeEventUnlisten;
      nativeEventUnlisten = null;
      unlisten();
    }
  };
}

// Keeps module-level listener state isolated across bun test cases.
export function resetNativeBridgeForTests(): void {
  listeners.clear();
  nativeEventSubscription = null;
  if (nativeEventUnlisten) {
    const unlisten = nativeEventUnlisten;
    nativeEventUnlisten = null;
    unlisten();
  }
}
