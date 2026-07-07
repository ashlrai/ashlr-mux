import { describe, expect, test } from "bun:test";

import { RenderSequencingGuard } from "./renderSequencing";

describe("renderSequencing (D7)", () => {
  test("issue() returns strictly increasing ids starting at 1", () => {
    const g = new RenderSequencingGuard();
    expect(g.issue()).toBe(1);
    expect(g.issue()).toBe(2);
    expect(g.issue()).toBe(3);
  });

  test("accepts a first batch, then drops a stale sequence", () => {
    const g = new RenderSequencingGuard();
    // First batch applies.
    expect(g.applyIfCurrent(1, 1)).toBe(true);
    // Stale seq (0 < appliedSequence 1) is dropped even with a higher rv.
    expect(g.applyIfCurrent(0, 5)).toBe(false);
  });

  test("drops a stale resultsVersion (Swift line 52)", () => {
    const g = new RenderSequencingGuard();
    expect(g.applyIfCurrent(1, 1)).toBe(true);
    // seq advances but rv (0 < appliedResultsVersion 1) is stale -> drop.
    expect(g.applyIfCurrent(2, 0)).toBe(false);
  });

  test("out-of-order async arrival: issue 1,2,3; apply 3,2,1 -> only 3 applied", () => {
    const g = new RenderSequencingGuard();
    const a = g.issue(); // 1
    const b = g.issue(); // 2
    const c = g.issue(); // 3
    // Producer stamps each with the same monotonic results version here.
    expect(g.applyIfCurrent(c, 1)).toBe(true);
    expect(g.applyIfCurrent(b, 1)).toBe(false);
    expect(g.applyIfCurrent(a, 1)).toBe(false);
  });

  test("appliedResultsVersion uses max, not last-write", () => {
    const g = new RenderSequencingGuard();
    // Apply a high rv first.
    expect(g.applyIfCurrent(1, 5)).toBe(true);
    // seq advances (2 >= 1) but rv 3 < appliedResultsVersion 5 -> dropped by max.
    expect(g.applyIfCurrent(2, 3)).toBe(false);
  });

  test("equal-seq boundary (>=) accepts a re-schedule at the same seq with a higher rv", () => {
    const g = new RenderSequencingGuard();
    expect(g.applyIfCurrent(1, 1)).toBe(true);
    // Same seq (1 >= 1) with higher rv (2 >= 1) is accepted.
    expect(g.applyIfCurrent(1, 2)).toBe(true);
  });

  test("decide() is pure: does not mutate applied state", () => {
    const g = new RenderSequencingGuard();
    expect(g.applyIfCurrent(1, 1)).toBe(true);
    // Probing with decide() many times must not advance applied state.
    expect(g.decide(2, 2)).toBe(true);
    expect(g.decide(2, 2)).toBe(true);
    // A later stale batch is still gated against the original applied=1/rv=1.
    expect(g.applyIfCurrent(1, 1)).toBe(true); // equal on both axes -> accepted
    expect(g.snapshot()).toEqual({
      scheduledSequence: 0,
      appliedSequence: 1,
      appliedResultsVersion: 1,
    });
  });

  test("snapshot reflects issued + applied counters", () => {
    const g = new RenderSequencingGuard();
    g.issue();
    g.issue();
    g.applyIfCurrent(2, 4);
    expect(g.snapshot()).toEqual({
      scheduledSequence: 2,
      appliedSequence: 2,
      appliedResultsVersion: 4,
    });
  });
});
