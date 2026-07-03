import { describe, expect, test } from "bun:test";

import {
  applyMarkdownTheme,
  renderMarkdown,
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
