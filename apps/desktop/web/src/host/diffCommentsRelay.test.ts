import { afterEach, describe, expect, test } from "bun:test";

import {
  DIFF_COMMENTS_COMMAND,
  DIFF_COMMENTS_REPLY_TYPE,
  DIFF_COMMENTS_REQUEST_TYPE,
  DIFF_VIEWER_HTTP_ORIGIN,
  defaultAllowDiffViewerOrigin,
  installDiffCommentsGuestShim,
  installDiffCommentsRelay,
  uninstallDiffCommentsRelay,
  type DiffCommentsGuestWindow,
  type RelayMessageEvent,
  type RelayWindowTarget,
} from "./diffCommentsRelay";
import type { HostMessage, NativeReply } from "./host";

/** Let the relay's async invoke→reply pipeline drain. */
function flush(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function makeWindowTarget() {
  const state = {
    listeners: [] as Array<(event: RelayMessageEvent) => void>,
    removals: 0,
  };
  const target: RelayWindowTarget = {
    addEventListener(_type, listener) {
      state.listeners.push(listener);
    },
    removeEventListener(_type, listener) {
      state.removals += 1;
      state.listeners = state.listeners.filter((entry) => entry !== listener);
    },
  };
  const dispatch = (event: RelayMessageEvent) => {
    for (const listener of [...state.listeners]) {
      listener(event);
    }
  };
  return { state, target, dispatch };
}

function makeSource() {
  const posted: Array<{ message: unknown; targetOrigin: string }> = [];
  return {
    posted,
    source: {
      postMessage(message: unknown, targetOrigin: string) {
        posted.push({ message, targetOrigin });
      },
    },
  };
}

function requestData(relayId: string, message: HostMessage): unknown {
  return { type: DIFF_COMMENTS_REQUEST_TYPE, relayId, message };
}

describe("installDiffCommentsRelay", () => {
  afterEach(() => {
    uninstallDiffCommentsRelay();
  });

  test("forwards a request to diff_comments_rpc verbatim and replies to the source at event.origin", async () => {
    const { target, dispatch } = makeWindowTarget();
    const { posted, source } = makeSource();
    let seenCommand: string | undefined;
    let seenArgs: Record<string, unknown> | undefined;

    installDiffCommentsRelay({
      windowTarget: target,
      invokeRaw: async (command, args) => {
        seenCommand = command;
        seenArgs = args;
        return { ok: true, value: { comments: [] } } as unknown;
      },
    });

    const message: HostMessage = { id: "c1", method: "comments.list", params: { repoRoot: "/r" } };
    dispatch({ data: requestData("r-1", message), origin: DIFF_VIEWER_HTTP_ORIGIN, source });
    await flush();

    expect(seenCommand).toBe(DIFF_COMMENTS_COMMAND);
    expect(seenArgs).toEqual({ message });
    expect(posted).toEqual([
      {
        message: {
          type: DIFF_COMMENTS_REPLY_TYPE,
          relayId: "r-1",
          reply: { ok: true, value: { comments: [] } },
        },
        targetOrigin: DIFF_VIEWER_HTTP_ORIGIN,
      },
    ]);
  });

  test("wraps a bare (non-envelope) invoke result as { ok: true, value }", async () => {
    const { target, dispatch } = makeWindowTarget();
    const { posted, source } = makeSource();

    installDiffCommentsRelay({ windowTarget: target, invokeRaw: async () => "pong" as unknown });

    dispatch({
      data: requestData("r-2", { method: "comments.list" }),
      origin: DIFF_VIEWER_HTTP_ORIGIN,
      source,
    });
    await flush();

    expect(posted).toHaveLength(1);
    expect((posted[0]!.message as { reply: NativeReply }).reply).toEqual({
      ok: true,
      value: "pong",
    });
  });

  test("maps an invoke rejection to an { ok: false, error } reply (never throws)", async () => {
    const { target, dispatch } = makeWindowTarget();
    const { posted, source } = makeSource();

    installDiffCommentsRelay({
      windowTarget: target,
      invokeRaw: async () => {
        const err = new Error("command not found") as Error & { code?: string };
        err.code = "E_NO_CMD";
        throw err;
      },
    });

    dispatch({
      data: requestData("r-3", { method: "comments.save" }),
      origin: DIFF_VIEWER_HTTP_ORIGIN,
      source,
    });
    await flush();

    expect(posted).toHaveLength(1);
    expect((posted[0]!.message as { reply: NativeReply }).reply).toEqual({
      ok: false,
      error: { code: "E_NO_CMD", userMessage: "command not found" },
    });
  });

  test("ignores requests from a disallowed origin: no invoke, no reply", async () => {
    const { target, dispatch } = makeWindowTarget();
    const { posted, source } = makeSource();
    let invokes = 0;

    installDiffCommentsRelay({
      windowTarget: target,
      invokeRaw: async () => {
        invokes += 1;
        return { ok: true, value: null } as unknown;
      },
    });

    dispatch({
      data: requestData("r-4", { method: "comments.list" }),
      origin: "http://tauri.localhost",
      source,
    });
    await flush();

    expect(invokes).toBe(0);
    expect(posted).toHaveLength(0);
  });

  test("ignores malformed data (wrong type tag, missing relayId, null)", async () => {
    const { target, dispatch } = makeWindowTarget();
    const { posted, source } = makeSource();
    let invokes = 0;

    installDiffCommentsRelay({
      windowTarget: target,
      invokeRaw: async () => {
        invokes += 1;
        return { ok: true, value: null } as unknown;
      },
    });

    const origin = DIFF_VIEWER_HTTP_ORIGIN;
    dispatch({ data: { type: "some-other-message", relayId: "x", message: {} }, origin, source });
    dispatch({ data: { type: DIFF_COMMENTS_REQUEST_TYPE, message: { method: "m" } }, origin, source });
    dispatch({ data: null, origin, source });
    dispatch({ data: "cmux-diff-comments-request", origin, source });
    await flush();

    expect(invokes).toBe(0);
    expect(posted).toHaveLength(0);
  });

  test("ignores a request whose event has no postMessage-capable source", async () => {
    const { target, dispatch } = makeWindowTarget();
    let invokes = 0;

    installDiffCommentsRelay({
      windowTarget: target,
      invokeRaw: async () => {
        invokes += 1;
        return { ok: true, value: null } as unknown;
      },
    });

    const data = requestData("r-5", { method: "comments.list" });
    expect(() => {
      dispatch({ data, origin: DIFF_VIEWER_HTTP_ORIGIN, source: null });
      dispatch({ data, origin: DIFF_VIEWER_HTTP_ORIGIN });
    }).not.toThrow();
    await flush();

    expect(invokes).toBe(0);
  });

  test("reinstalling tears down the prior listener exactly once", () => {
    const { state, target } = makeWindowTarget();
    const invokeRaw = async () => ({ ok: true, value: null }) as unknown;

    installDiffCommentsRelay({ windowTarget: target, invokeRaw });
    installDiffCommentsRelay({ windowTarget: target, invokeRaw });

    // Two installs, but the first listener was removed before the second landed.
    expect(state.removals).toBe(1);
    expect(state.listeners).toHaveLength(1);

    uninstallDiffCommentsRelay();
    expect(state.removals).toBe(2);
    expect(state.listeners).toHaveLength(0);

    // Idempotent: nothing left to remove.
    uninstallDiffCommentsRelay();
    expect(state.removals).toBe(2);
  });

  test("default allowOrigin accepts both diff-viewer origin forms and rejects others", () => {
    expect(defaultAllowDiffViewerOrigin("http://cmux-diff-viewer.localhost")).toBe(true);
    expect(defaultAllowDiffViewerOrigin("cmux-diff-viewer://tok-abcdef0123456789")).toBe(true);
    expect(defaultAllowDiffViewerOrigin("http://tauri.localhost")).toBe(false);
    expect(defaultAllowDiffViewerOrigin("https://example.com")).toBe(false);
  });

  test("default allowOrigin is applied when no override is given", async () => {
    const { target, dispatch } = makeWindowTarget();
    const { posted, source } = makeSource();
    let invokes = 0;

    installDiffCommentsRelay({
      windowTarget: target,
      invokeRaw: async () => {
        invokes += 1;
        return { ok: true, value: null } as unknown;
      },
    });

    const data = requestData("r-6", { method: "comments.list" });
    dispatch({ data, origin: "https://example.com", source });
    dispatch({ data, origin: "cmux-diff-viewer://tok-abcdef0123456789", source });
    await flush();

    expect(invokes).toBe(1);
    expect(posted).toHaveLength(1);
    expect(posted[0]!.targetOrigin).toBe("cmux-diff-viewer://tok-abcdef0123456789");
  });
});

function makeGuestWindow() {
  const posted: Array<{ message: unknown; targetOrigin: string }> = [];
  const listeners: Array<(event: { data: unknown }) => void> = [];
  const guestWindow: DiffCommentsGuestWindow = {
    parent: {
      postMessage(message: unknown, targetOrigin: string) {
        posted.push({ message, targetOrigin });
      },
    },
    addEventListener(_type, listener) {
      listeners.push(listener);
    },
  };
  const deliver = (data: unknown) => {
    for (const listener of [...listeners]) {
      listener({ data });
    }
  };
  return { guestWindow, posted, deliver };
}

function guestHandler(guestWindow: DiffCommentsGuestWindow) {
  return guestWindow.webkit!.messageHandlers!.cmuxDiffComments!;
}

describe("installDiffCommentsGuestShim", () => {
  test("postMessage posts a well-formed request to parent and resolves on the matching reply", async () => {
    const { guestWindow, posted, deliver } = makeGuestWindow();
    installDiffCommentsGuestShim(guestWindow);

    const message: HostMessage = { id: "g1", method: "comments.list", params: { repoRoot: "/r" } };
    const pending = guestHandler(guestWindow).postMessage(message);

    expect(posted).toHaveLength(1);
    expect(posted[0]!.targetOrigin).toBe("*");
    const request = posted[0]!.message as { type: string; relayId: string; message: HostMessage };
    expect(request.type).toBe(DIFF_COMMENTS_REQUEST_TYPE);
    expect(typeof request.relayId).toBe("string");
    expect(request.message).toEqual(message);

    const reply: NativeReply = { ok: true, value: { comments: [] } };
    deliver({ type: DIFF_COMMENTS_REPLY_TYPE, relayId: request.relayId, reply });
    expect(await pending).toEqual(reply);
  });

  test("ignores replies with a mismatched relayId", async () => {
    const { guestWindow, posted, deliver } = makeGuestWindow();
    installDiffCommentsGuestShim(guestWindow, { timeoutMs: 20 });

    const pending = guestHandler(guestWindow).postMessage({ method: "comments.list" });
    const request = posted[0]!.message as { relayId: string };

    deliver({ type: DIFF_COMMENTS_REPLY_TYPE, relayId: "someone-else", reply: { ok: true, value: 1 } });
    deliver({ type: "some-other-message", relayId: request.relayId, reply: { ok: true, value: 2 } });

    // Only the mismatches arrived, so the call falls through to the timeout.
    expect(await pending).toEqual({
      ok: false,
      error: { code: "bridge_unavailable", userMessage: "Diff comments bridge is unavailable." },
    });
  });

  test("an unanswered call resolves the bridge_unavailable envelope after the timeout", async () => {
    const { guestWindow } = makeGuestWindow();
    installDiffCommentsGuestShim(guestWindow, { timeoutMs: 5 });

    const reply = await guestHandler(guestWindow).postMessage({ method: "comments.list" });
    expect(reply).toEqual({
      ok: false,
      error: { code: "bridge_unavailable", userMessage: "Diff comments bridge is unavailable." },
    });
  });

  test("concurrent calls resolve to their own replies", async () => {
    const { guestWindow, posted, deliver } = makeGuestWindow();
    installDiffCommentsGuestShim(guestWindow);

    const handler = guestHandler(guestWindow);
    const first = handler.postMessage({ id: "a", method: "comments.list" });
    const second = handler.postMessage({ id: "b", method: "comments.delete" });

    const firstId = (posted[0]!.message as { relayId: string }).relayId;
    const secondId = (posted[1]!.message as { relayId: string }).relayId;
    expect(firstId).not.toBe(secondId);

    // Deliver out of order to prove replies are keyed, not queued.
    deliver({ type: DIFF_COMMENTS_REPLY_TYPE, relayId: secondId, reply: { ok: true, value: "second" } });
    deliver({ type: DIFF_COMMENTS_REPLY_TYPE, relayId: firstId, reply: { ok: true, value: "first" } });

    expect(await first).toEqual({ ok: true, value: "first" });
    expect(await second).toEqual({ ok: true, value: "second" });
  });
});
