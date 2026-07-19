import { describe, expect, test } from "bun:test";

describe("sidebar geometry", () => {
  test("uses the canonical 240 point default width", async () => {
    const css = await Bun.file(new URL("../styles.css", import.meta.url)).text();
    const rule = css.match(/\.cmux-sidebar\s*\{([^}]*)\}/)?.[1];

    expect(rule).toBeDefined();
    expect(rule).toMatch(/\bwidth:\s*240px\s*;/);
  });
});
