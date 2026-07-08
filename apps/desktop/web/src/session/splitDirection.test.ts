import { describe, expect, test } from "bun:test";

import { directionalSplitPlan } from "./splitDirection";

describe("directionalSplitPlan", () => {
  test("left/up insert the new pane first; right/down append it second", () => {
    expect(directionalSplitPlan("left")).toEqual({
      orientation: "horizontal",
      insertFirst: true,
    });
    expect(directionalSplitPlan("right")).toEqual({
      orientation: "horizontal",
      insertFirst: false,
    });
    expect(directionalSplitPlan("up")).toEqual({
      orientation: "vertical",
      insertFirst: true,
    });
    expect(directionalSplitPlan("down")).toEqual({
      orientation: "vertical",
      insertFirst: false,
    });
  });
});
