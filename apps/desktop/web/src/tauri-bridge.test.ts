import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import {
  NativeBridgeError,
  callNative,
  resetNativeBridgeForTests,
  subscribeToAgentEvents,
} from "./tauri-bridge";

type AgentEvent = { type: string; payload?: unknown };

type ListenCallback = (event: { payload: unknown }) => void;

type MockWindow = {
  __TAURI__?: {
    core?: {
      invoke?: <T>(command: string, payload?: Record<string, unknown>) => Promise<T>;
    };
    event?: {
      listen?: (name: string, callback: ListenCallback) => Promise<() => void>;
    };
  };
};

function installWindow(windowValue: MockWindow): void {
  (globalThis as { window?: MockWindow }).window = windowValue;
}

/**
 * A deferred promise helper so tests can drive the timing of the native
 * `listen` resolution explicitly. This is what lets us reproduce the
 * in-flight subscription race that the bridge guards against.
 */
function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
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

  describe("without a native bridge", () => {
    test("returns the browser ping fallback for the ping method", async () => {
      await expect(callNative<string>("ping")).resolves.toBe("pong (browser fallback)");
    });

    test("throws a typed NativeBridgeError for any non-ping method", async () => {
      await expect(callNative("connect")).rejects.toBeInstanceOf(NativeBridgeError);
    });

    test("the unavailable error carries the expected message and no code", async () => {
      try {
        await callNative("connect");
        throw new Error("expected callNative to reject");
      } catch (error) {
        expect(error).toBeInstanceOf(NativeBridgeError);
        expect((error as NativeBridgeError).message).toBe(
          "Tauri bridge is unavailable in the current runtime.",
        );
        expect((error as NativeBridgeError).code).toBeUndefined();
        expect((error as NativeBridgeError).name).toBe("NativeBridgeError");
      }
    });

    test("treats a window with __TAURI__ but no core.invoke as unavailable", async () => {
      installWindow({ __TAURI__: { core: {} } });
      await expect(callNative<string>("ping")).resolves.toBe("pong (browser fallback)");
      await expect(callNative("connect")).rejects.toBeInstanceOf(NativeBridgeError);
    });
  });

  describe("NativeReply<T> shapes", () => {
    function installInvoke(
      invoke: (command: string, payload?: Record<string, unknown>) => unknown,
    ): void {
      installWindow({
        __TAURI__: {
          core: {
            invoke: (async (command: string, payload?: Record<string, unknown>) =>
              invoke(command, payload)) as <T>(
              command: string,
              payload?: Record<string, unknown>,
            ) => Promise<T>,
          },
        },
      });
    }

    test("passes a bare T reply through unchanged (string)", async () => {
      installInvoke(() => "pong");
      await expect(callNative<string>("ping")).resolves.toBe("pong");
    });

    test("passes a bare object reply through unchanged when it lacks an `ok` field", async () => {
      const value = { milestone: "M1", providers: ["codex"] };
      installInvoke(() => value);
      await expect(callNative<typeof value>("desktop_core_status")).resolves.toEqual(value);
    });

    test("unwraps an { ok: true, value } envelope", async () => {
      installInvoke(() => ({ ok: true, value: { count: 3 } }));
      await expect(callNative<{ count: number }>("status")).resolves.toEqual({ count: 3 });
    });

    test("unwraps an { ok: true, value } envelope carrying a primitive", async () => {
      installInvoke(() => ({ ok: true, value: "pong" }));
      await expect(callNative<string>("ping")).resolves.toBe("pong");
    });

    test("unwraps an { ok: true, value } envelope where value is falsy", async () => {
      installInvoke(() => ({ ok: true, value: 0 }));
      await expect(callNative<number>("count")).resolves.toBe(0);

      installInvoke(() => ({ ok: true, value: null }));
      await expect(callNative<null>("nothing")).resolves.toBeNull();

      installInvoke(() => ({ ok: true, value: false }));
      await expect(callNative<boolean>("flag")).resolves.toBe(false);
    });

    test("throws NativeBridgeError on an { ok: false, error } envelope", async () => {
      installInvoke(() => ({
        ok: false,
        error: { code: "E_BRIDGE", userMessage: "native failure" },
      }));

      try {
        await callNative("connect");
        throw new Error("expected callNative to reject");
      } catch (error) {
        expect(error).toBeInstanceOf(NativeBridgeError);
        expect((error as NativeBridgeError).message).toBe("native failure");
        expect((error as NativeBridgeError).code).toBe("E_BRIDGE");
      }
    });

    test("propagates error.code even when userMessage is absent", async () => {
      installInvoke(() => ({ ok: false, error: { code: "E_NO_MESSAGE" } }));

      try {
        await callNative("connect");
        throw new Error("expected callNative to reject");
      } catch (error) {
        expect(error).toBeInstanceOf(NativeBridgeError);
        expect((error as NativeBridgeError).code).toBe("E_NO_MESSAGE");
        expect((error as NativeBridgeError).message).toBe("Native bridge request failed.");
      }
    });

    test("falls back to the default message when error is entirely missing", async () => {
      installInvoke(() => ({ ok: false }));

      try {
        await callNative("connect");
        throw new Error("expected callNative to reject");
      } catch (error) {
        expect(error).toBeInstanceOf(NativeBridgeError);
        expect((error as NativeBridgeError).message).toBe("Native bridge request failed.");
        expect((error as NativeBridgeError).code).toBeUndefined();
      }
    });

    test("falls back to the default message when userMessage is an empty string", async () => {
      installInvoke(() => ({ ok: false, error: { code: "E_EMPTY", userMessage: "" } }));

      try {
        await callNative("connect");
        throw new Error("expected callNative to reject");
      } catch (error) {
        expect((error as NativeBridgeError).message).toBe("Native bridge request failed.");
        expect((error as NativeBridgeError).code).toBe("E_EMPTY");
      }
    });

    test("passes a null reply through unchanged (no `ok` membership test crash)", async () => {
      installInvoke(() => null);
      await expect(callNative<null>("noop")).resolves.toBeNull();
    });

    test("passes non-object primitive replies (number, boolean) through unchanged", async () => {
      installInvoke(() => 42);
      await expect(callNative<number>("answer")).resolves.toBe(42);

      installInvoke(() => true);
      await expect(callNative<boolean>("truthy")).resolves.toBe(true);
    });

    test("passes an array reply through unchanged (object but no `ok` field)", async () => {
      installInvoke(() => ["codex", "claude", "opencode"]);
      await expect(callNative<string[]>("providers")).resolves.toEqual([
        "codex",
        "claude",
        "opencode",
      ]);
    });

    test("forwards the method name and params to the native invoke", async () => {
      let seenCommand: string | undefined;
      let seenPayload: Record<string, unknown> | undefined;
      installInvoke((command, payload) => {
        seenCommand = command;
        seenPayload = payload;
        return "ok";
      });

      await callNative<string>("run_agent", { provider: "codex", id: 7 });
      expect(seenCommand).toBe("run_agent");
      expect(seenPayload).toEqual({ provider: "codex", id: 7 });
    });

    test("defaults params to an empty object when omitted", async () => {
      let seenPayload: Record<string, unknown> | undefined;
      installInvoke((_command, payload) => {
        seenPayload = payload;
        return "ok";
      });

      await callNative<string>("ping");
      expect(seenPayload).toEqual({});
    });
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

  /**
   * Installs an event-listen mock that resolves the native `listen` promise
   * synchronously (microtask). Returns counters and the captured native
   * callback so the test can drive event delivery.
   */
  function installImmediateListen() {
    const state = {
      listenCalls: 0,
      unlistenCalls: 0,
      nativeCallback: null as ListenCallback | null,
    };
    installWindow({
      __TAURI__: {
        event: {
          listen: async (_name, callback) => {
            state.listenCalls += 1;
            state.nativeCallback = callback;
            return () => {
              state.unlistenCalls += 1;
            };
          },
        },
      },
    });
    return state;
  }

  test("shares a single native subscription across listeners and fans events out once", async () => {
    const state = installImmediateListen();

    const firstEvents: AgentEvent[] = [];
    const secondEvents: AgentEvent[] = [];

    const offFirst = await subscribeToAgentEvents((event) => firstEvents.push(event));
    const offSecond = await subscribeToAgentEvents((event) => secondEvents.push(event));

    expect(state.listenCalls).toBe(1);
    expect(state.nativeCallback).not.toBeNull();

    state.nativeCallback?.({
      payload: { type: "agent-output", payload: { text: "hello" } },
    });

    expect(firstEvents).toEqual([{ type: "agent-output", payload: { text: "hello" } }]);
    expect(secondEvents).toEqual([{ type: "agent-output", payload: { text: "hello" } }]);

    offFirst();
    expect(state.unlistenCalls).toBe(0);

    offSecond();
    expect(state.unlistenCalls).toBe(1);
  });

  test("re-subscribing after a full teardown re-establishes the native listener", async () => {
    const state = installImmediateListen();

    const off1 = await subscribeToAgentEvents(() => {});
    expect(state.listenCalls).toBe(1);

    // Last unsubscribe tears the native listener down.
    off1();
    expect(state.unlistenCalls).toBe(1);

    // A fresh subscribe after teardown must create a brand-new native listener.
    const received: AgentEvent[] = [];
    const off2 = await subscribeToAgentEvents((event) => received.push(event));
    expect(state.listenCalls).toBe(2);
    expect(state.unlistenCalls).toBe(1);

    state.nativeCallback?.({ payload: { type: "after-resubscribe" } });
    expect(received).toEqual([{ type: "after-resubscribe" }]);

    off2();
    expect(state.unlistenCalls).toBe(2);
  });

  test("interleaved subscribe/unsubscribe fans events to current listeners only", async () => {
    const state = installImmediateListen();

    const aEvents: AgentEvent[] = [];
    const bEvents: AgentEvent[] = [];
    const cEvents: AgentEvent[] = [];

    const offA = await subscribeToAgentEvents((e) => aEvents.push(e));
    const offB = await subscribeToAgentEvents((e) => bEvents.push(e));

    state.nativeCallback?.({ payload: { type: "evt-1" } });
    expect(aEvents).toEqual([{ type: "evt-1" }]);
    expect(bEvents).toEqual([{ type: "evt-1" }]);

    // Drop A, add C. Native listener must survive (B still present).
    offA();
    const offC = await subscribeToAgentEvents((e) => cEvents.push(e));
    expect(state.listenCalls).toBe(1);
    expect(state.unlistenCalls).toBe(0);

    state.nativeCallback?.({ payload: { type: "evt-2" } });
    expect(aEvents).toEqual([{ type: "evt-1" }]); // A no longer receives
    expect(bEvents).toEqual([{ type: "evt-1" }, { type: "evt-2" }]);
    expect(cEvents).toEqual([{ type: "evt-2" }]);

    offB();
    expect(state.unlistenCalls).toBe(0); // C still present

    offC();
    expect(state.unlistenCalls).toBe(1); // last listener gone
  });

  test("last unsubscribe tears down the native listener exactly once even if called twice", async () => {
    const state = installImmediateListen();

    const off = await subscribeToAgentEvents(() => {});
    off();
    expect(state.unlistenCalls).toBe(1);

    // Calling the same unsubscribe again must not double-tear-down.
    off();
    expect(state.unlistenCalls).toBe(1);
  });

  test("two subscribes before the native listen resolves share ONE native subscription", async () => {
    // This is the original race: if `ensureNativeEventSubscription` did not
    // memoise the in-flight promise, each early subscribe would start its own
    // native `listen`, duplicating event delivery.
    const listenDeferred = deferred<() => void>();
    let listenCalls = 0;
    let unlistenCalls = 0;
    // Hold the captured callback on an object: a bare `let` assigned only inside
    // the closure gets control-flow-narrowed to `null` at the later call site.
    const captured: { nativeCallback: ListenCallback | null } = { nativeCallback: null };

    installWindow({
      __TAURI__: {
        event: {
          listen: (_name, callback) => {
            listenCalls += 1;
            captured.nativeCallback = callback;
            return listenDeferred.promise;
          },
        },
      },
    });

    const firstEvents: AgentEvent[] = [];
    const secondEvents: AgentEvent[] = [];

    // Kick off two subscriptions BEFORE the native listen promise resolves.
    const p1 = subscribeToAgentEvents((e) => firstEvents.push(e));
    const p2 = subscribeToAgentEvents((e) => secondEvents.push(e));

    // Only one native listen should have been started.
    expect(listenCalls).toBe(1);

    // Now resolve the native listen and let both subscribe calls settle.
    listenDeferred.resolve(() => {
      unlistenCalls += 1;
    });
    const off1 = await p1;
    const off2 = await p2;

    expect(listenCalls).toBe(1);
    expect(captured.nativeCallback).not.toBeNull();

    captured.nativeCallback?.({ payload: { type: "raced-event" } });
    expect(firstEvents).toEqual([{ type: "raced-event" }]);
    expect(secondEvents).toEqual([{ type: "raced-event" }]);

    off1();
    expect(unlistenCalls).toBe(0);
    off2();
    expect(unlistenCalls).toBe(1);
  });

  test("subscribing in a browser runtime (no event.listen) still returns a usable unsubscribe", async () => {
    installWindow({});
    const events: AgentEvent[] = [];
    const off = await subscribeToAgentEvents((e) => events.push(e));
    // No native listener, but unsubscribe must be safe to call.
    expect(() => off()).not.toThrow();
    expect(events).toEqual([]);
  });
});
