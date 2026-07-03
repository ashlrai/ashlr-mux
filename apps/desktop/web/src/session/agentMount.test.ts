import { describe, expect, test } from "bun:test";

import { stickyAgentPanes } from "./agentMount";

const panes = (...ids: string[]): ReadonlySet<string> => new Set(ids);

describe("stickyAgentPanes", () => {
  test("no agent anywhere and no prior owners → nobody owns a surface", () => {
    expect(stickyAgentPanes(panes(), panes(), panes("a", "b"))).toEqual(panes());
  });

  test("a pane that is currently an agent owns a surface", () => {
    expect(stickyAgentPanes(panes(), panes("a"), panes("a", "b"))).toEqual(panes("a"));
  });

  test("two agent panes each own their own surface (concurrent agents)", () => {
    expect(stickyAgentPanes(panes(), panes("a", "b"), panes("a", "b"))).toEqual(
      panes("a", "b"),
    );
  });

  test("toggling an agent pane to a terminal keeps it sticky (run survives)", () => {
    // Frame 1: pane "a" is the agent.
    const owners = stickyAgentPanes(panes(), panes("a"), panes("a", "b"));
    expect(owners).toEqual(panes("a"));
    // Frame 2: user toggled "a" back to a terminal — no pane is `agent` now, but
    // "a" still exists, so it must retain the (hidden) mounted agent surface.
    expect(stickyAgentPanes(owners, panes(), panes("a", "b"))).toEqual(panes("a"));
  });

  test("toggling back to agent reuses the same owner (same mounted surface)", () => {
    const hidden = stickyAgentPanes(panes("a"), panes(), panes("a", "b"));
    expect(stickyAgentPanes(hidden, panes("a"), panes("a", "b"))).toEqual(panes("a"));
  });

  test("another pane becoming an agent adds an owner without stealing", () => {
    expect(stickyAgentPanes(panes("a"), panes("b"), panes("a", "b"))).toEqual(
      panes("a", "b"),
    );
  });

  test("closing an owning pane drops only that owner", () => {
    expect(stickyAgentPanes(panes("a", "b"), panes(), panes("b"))).toEqual(panes("b"));
  });
});
