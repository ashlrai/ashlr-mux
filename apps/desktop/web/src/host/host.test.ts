import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import {
  MAC_HOST_CHANNELS,
  MAC_HOST_EVENT,
  installMacHostShims,
  uninstallMacHostShims,
  type HostMessage,
  type NativeReply,
} from "./host";

type MessageHandler = {
  postMessage(message: HostMessage): Promise<NativeReply>;
};

type ShimWindow = {
  webkit?: { messageHandlers?: Record<string, MessageHandler> };
  cmuxAgentBridge?: { receive(event: unknown): void };
};

const originalWindowDescriptor = Object.getOwnPropertyDescriptor(globalThis, "window");

function installWindow(value: ShimWindow = {}): ShimWindow {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    writable: true,
    value,
  });
  return value;
}

function restoreWindow(): void {
  if (originalWindowDescriptor) {
    Object.defineProperty(globalThis, "window", originalWindowDescriptor);
    return;
  }
  delete (globalThis as { window?: ShimWindow }).window;
}

describe("installMacHostShims", () => {
  beforeEach(() => {
    installWindow();
  });

  afterEach(() => {
    uninstallMacHostShims();
    restoreWindow();
  });

  function makeListen() {
    const state = {
      calls: 0,
      unlistenCalls: 0,
      event: null as string | null,
      callback: null as ((payload: unknown) => void) | null,
    };
    const listen = async (event: string, callback: (payload: unknown) => void) => {
      state.calls += 1;
      state.event = event;
      state.callback = callback;
      return () => {
        state.unlistenCalls += 1;
      };
    };
    return { state, listen };
  }

  test("installs a message handler for every macOS channel", async () => {
    const { listen } = makeListen();
    const win = installWindow();

    await installMacHostShims({ invokeRaw: async () => ({ ok: true, value: 1 }), listen });

    for (const channel of Object.keys(MAC_HOST_CHANNELS)) {
      const handler = win.webkit?.messageHandlers?.[channel];
      expect(handler).toBeDefined();
      expect(typeof handler?.postMessage).toBe("function");
    }
  });

  test("postMessage invokes the mapped Tauri command with { message } and returns the envelope verbatim", async () => {
    const { listen } = makeListen();
    const win = installWindow();
    let seenCommand: string | undefined;
    let seenArgs: Record<string, unknown> | undefined;

    await installMacHostShims({
      listen,
      invokeRaw: async (command, args) => {
        seenCommand = command;
        seenArgs = args;
        return { ok: true, value: { count: 3 } } as unknown;
      },
    });

    const message: HostMessage = { id: "c1", method: "session.snapshot", params: { id: 7 } };
    const reply = await win.webkit!.messageHandlers!.agentSession!.postMessage(message);

    expect(seenCommand).toBe(MAC_HOST_CHANNELS.agentSession);
    expect(seenArgs).toEqual({ message });
    expect(reply).toEqual({ ok: true, value: { count: 3 } });
  });

  test("wraps a bare (non-envelope) native reply as { ok: true, value }", async () => {
    const { listen } = makeListen();
    const win = installWindow();

    await installMacHostShims({ listen, invokeRaw: async () => "pong" as unknown });

    const reply = await win.webkit!.messageHandlers!.cmuxLib!.postMessage({ method: "ping" });
    expect(reply).toEqual({ ok: true, value: "pong" });
  });

  test("maps a transport rejection to an { ok: false, error } reply (never throws)", async () => {
    const { listen } = makeListen();
    const win = installWindow();

    await installMacHostShims({
      listen,
      invokeRaw: async () => {
        const err = new Error("command not found") as Error & { code?: string };
        err.code = "E_NO_CMD";
        throw err;
      },
    });

    const reply = await win.webkit!.messageHandlers!.cmuxDiffComments!.postMessage({
      method: "comments.list",
    });
    expect(reply).toEqual({
      ok: false,
      error: { code: "E_NO_CMD", userMessage: "command not found" },
    });
  });

  test("subscribes to the native push event and forwards payloads to cmuxAgentBridge.receive", async () => {
    const { state, listen } = makeListen();
    const received: unknown[] = [];
    installWindow({ cmuxAgentBridge: { receive: (event) => received.push(event) } });

    await installMacHostShims({ listen, invokeRaw: async () => ({ ok: true, value: null }) });

    expect(state.calls).toBe(1);
    expect(state.event).toBe(MAC_HOST_EVENT);

    state.callback?.({ type: "agent.output", text: "hi" });
    expect(received).toEqual([{ type: "agent.output", text: "hi" }]);
  });

  test("forwarding is a safe no-op when cmuxAgentBridge is absent", async () => {
    const { state, listen } = makeListen();
    installWindow(); // no cmuxAgentBridge

    await installMacHostShims({ listen, invokeRaw: async () => ({ ok: true, value: null }) });

    expect(() => state.callback?.({ type: "orphan" })).not.toThrow();
  });

  test("reinstalling tears down the prior native subscription exactly once", async () => {
    const { state, listen } = makeListen();
    installWindow();

    await installMacHostShims({ listen, invokeRaw: async () => ({ ok: true, value: null }) });
    await installMacHostShims({ listen, invokeRaw: async () => ({ ok: true, value: null }) });

    // Two installs, but the first subscription was torn down before the second.
    expect(state.calls).toBe(2);
    expect(state.unlistenCalls).toBe(1);

    uninstallMacHostShims();
    expect(state.unlistenCalls).toBe(2);
  });
});
