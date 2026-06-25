import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import {
  NativeBridgeError,
  callNative,
  resetNativeBridgeForTests,
  subscribeToAgentEvents,
} from "./tauri-bridge";

type MockWindow = {
  __TAURI__?: {
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
};

function installWindow(windowValue: MockWindow): void {
  (globalThis as { window?: MockWindow }).window = windowValue;
}

describe("callNative", () => {
  beforeEach(() => {
    resetNativeBridgeForTests();
    installWindow({});
  });

  afterEach(() => {
    resetNativeBridgeForTests();
    delete (globalThis as { window?: MockWindow }).window;
  });

  test("returns the browser ping fallback without a native bridge", async () => {
    await expect(callNative<string>("ping")).resolves.toBe("pong (browser fallback)");
  });

  test("throws a typed bridge error when a non-ping method runs without tauri", async () => {
    await expect(callNative("connect")).rejects.toBeInstanceOf(NativeBridgeError);
  });

  test("passes through raw native replies", async () => {
    installWindow({
      __TAURI__: {
        core: {
          invoke: async () => "pong",
        },
      },
    });

    await expect(callNative<string>("ping")).resolves.toBe("pong");
  });

  test("unwraps ok envelopes and surfaces native error metadata", async () => {
    installWindow({
      __TAURI__: {
        core: {
          invoke: async (command: string) => {
            if (command === "ping") {
              return { ok: true, value: "pong" };
            }
            return {
              ok: false,
              error: {
                code: "E_BRIDGE",
                userMessage: "native failure",
              },
            };
          },
        },
      },
    });

    await expect(callNative<string>("ping")).resolves.toBe("pong");

    try {
      await callNative("connect");
      throw new Error("expected callNative to reject");
    } catch (error) {
      expect(error).toBeInstanceOf(NativeBridgeError);
      expect((error as NativeBridgeError).message).toBe("native failure");
      expect((error as NativeBridgeError).code).toBe("E_BRIDGE");
    }
  });
});

describe("subscribeToAgentEvents", () => {
  beforeEach(() => {
    resetNativeBridgeForTests();
    installWindow({});
  });

  afterEach(() => {
    resetNativeBridgeForTests();
    delete (globalThis as { window?: MockWindow }).window;
  });

  test("shares a single native subscription across listeners and fans events out once", async () => {
    let listenCalls = 0;
    let unlistenCalls = 0;
    let nativeCallback: ((event: { payload: unknown }) => void) | null = null;

    installWindow({
      __TAURI__: {
        event: {
          listen: async (_name, callback) => {
            listenCalls += 1;
            nativeCallback = callback;
            return () => {
              unlistenCalls += 1;
            };
          },
        },
      },
    });

    const firstEvents: Array<{ type: string; payload?: unknown }> = [];
    const secondEvents: Array<{ type: string; payload?: unknown }> = [];

    const offFirst = await subscribeToAgentEvents((event) => firstEvents.push(event));
    const offSecond = await subscribeToAgentEvents((event) => secondEvents.push(event));

    expect(listenCalls).toBe(1);
    expect(nativeCallback).not.toBeNull();

    nativeCallback?.({
      payload: {
        type: "agent-output",
        payload: { text: "hello" },
      },
    });

    expect(firstEvents).toEqual([{ type: "agent-output", payload: { text: "hello" } }]);
    expect(secondEvents).toEqual([{ type: "agent-output", payload: { text: "hello" } }]);

    offFirst();
    expect(unlistenCalls).toBe(0);

    offSecond();
    expect(unlistenCalls).toBe(1);
  });
});
