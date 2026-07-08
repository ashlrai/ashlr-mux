import { describe, expect, test } from "bun:test";

import {
  applyMarkdownTheme,
  renderMarkdown,
  renderMarkdownDocument,
  setMarkdownDocument,
  type MarkdownInvoke,
} from "./markdownBridge";

/** An invoke double that records the last channel + payload it was called with. */
function makeInvoke(result: unknown = undefined) {
  const calls: Array<{ channel: string; payload?: Record<string, unknown> }> = [];
  const invoke: MarkdownInvoke = async (channel, payload) => {
    calls.push({ channel, payload });
    return result;
  };
  return { calls, invoke };
}

describe("renderMarkdown", () => {
  test("invokes markdown_render with the document under the `markdown` key", async () => {
    const { calls, invoke } = makeInvoke();
    await renderMarkdown("# hello", invoke);
    expect(calls).toEqual([{ channel: "markdown_render", payload: { markdown: "# hello" } }]);
  });
});

describe("setMarkdownDocument", () => {
  test("invokes markdown_set_document with the path under the `path` key", async () => {
    const { calls, invoke } = makeInvoke();
    await setMarkdownDocument("C:/docs/a.md", invoke);
    expect(calls).toEqual([
      { channel: "markdown_set_document", payload: { path: "C:/docs/a.md" } },
    ]);
  });
});

describe("renderMarkdownDocument", () => {
  test("sets the document, then renders, in that order", async () => {
    const { calls, invoke } = makeInvoke();
    await renderMarkdownDocument({ path: "C:/docs/a.md", markdown: "# hi" }, invoke);
    expect(calls).toEqual([
      { channel: "markdown_set_document", payload: { path: "C:/docs/a.md" } },
      { channel: "markdown_render", payload: { markdown: "# hi" } },
    ]);
  });

  test("awaits the set-document reply before dispatching the render", async () => {
    const calls: string[] = [];
    let releaseSet!: () => void;
    const setSettled = new Promise<void>((resolve) => {
      releaseSet = resolve;
    });
    const invoke: MarkdownInvoke = async (channel) => {
      calls.push(channel);
      if (channel === "markdown_set_document") await setSettled;
      return undefined;
    };
    const feed = renderMarkdownDocument({ path: "C:/docs/a.md", markdown: "# hi" }, invoke);
    // Yield so the set invoke is in flight; the render must not have fired yet.
    await Promise.resolve();
    expect(calls).toEqual(["markdown_set_document"]);
    releaseSet();
    await feed;
    expect(calls).toEqual(["markdown_set_document", "markdown_render"]);
  });

  test("a set-document rejection propagates and suppresses the render", async () => {
    const calls: string[] = [];
    const invoke: MarkdownInvoke = async (channel) => {
      calls.push(channel);
      throw new Error("bridge down");
    };
    await expect(
      renderMarkdownDocument({ path: "C:/docs/a.md", markdown: "# hi" }, invoke),
    ).rejects.toThrow("bridge down");
    expect(calls).toEqual(["markdown_set_document"]);
  });
});

describe("applyMarkdownTheme", () => {
  test("invokes markdown_apply_theme with the sRGB triple under `background`", async () => {
    const { calls, invoke } = makeInvoke();
    await applyMarkdownTheme([11, 14, 20], invoke);
    expect(calls).toEqual([
      { channel: "markdown_apply_theme", payload: { background: [11, 14, 20] } },
    ]);
  });

  test("propagates a transport rejection to the caller", async () => {
    const invoke: MarkdownInvoke = async () => {
      throw new Error("bridge down");
    };
    await expect(applyMarkdownTheme([0, 0, 0], invoke)).rejects.toThrow("bridge down");
  });
});
