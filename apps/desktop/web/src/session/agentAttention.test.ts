import { describe, expect, test } from "bun:test";

import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

import {
  agentAttentionPanelForEvent,
  agentAttentionNotificationRequest,
  shouldNotifyAgentPanel,
} from "./agentAttention";

function workspace(
  panelId: string,
  sessionId: string,
): SessionWorkspaceSnapshot {
  return {
    workspace_id: `workspace-${panelId}`,
    process_title: "Workspace",
    layout: {
      type: "pane",
      pane: {
        panel_ids: [panelId],
        selected_panel_id: panelId,
      },
    },
    restorable_agent_snapshots: [
      {
        panel_id: panelId,
        snapshot: {
          kind: "codex",
          session_id: sessionId,
        },
      },
    ],
  };
}

describe("agent attention routing", () => {
  test("maps provider turn completion to the owning panel", () => {
    expect(
      agentAttentionPanelForEvent(
        { type: "provider.turnComplete", sessionId: "session-2" },
        [workspace("panel-1", "session-1"), workspace("panel-2", "session-2")],
      ),
    ).toBe("panel-2");
  });

  test("maps provider exit to the owning panel", () => {
    expect(
      agentAttentionPanelForEvent(
        { type: "provider.exit", sessionId: "session-1" },
        [workspace("panel-1", "session-1")],
      ),
    ).toBe("panel-1");
  });

  test("ignores unsupported events and unknown sessions", () => {
    const workspaces = [workspace("panel-1", "session-1")];

    expect(
      agentAttentionPanelForEvent(
        { type: "provider.output", sessionId: "session-1" },
        workspaces,
      ),
    ).toBeNull();
    expect(
      agentAttentionPanelForEvent(
        { type: "provider.turnComplete", sessionId: "missing" },
        workspaces,
      ),
    ).toBeNull();
    expect(
      agentAttentionPanelForEvent(
        { type: "provider.turnComplete", sessionId: undefined },
        workspaces,
      ),
    ).toBeNull();
  });

  test("suppresses attention only when the owning panel is focused in a focused document", () => {
    expect(shouldNotifyAgentPanel("panel-1", "panel-1", true)).toBe(false);
    expect(shouldNotifyAgentPanel("panel-1", "panel-2", true)).toBe(true);
    expect(shouldNotifyAgentPanel("panel-1", "panel-1", false)).toBe(true);
  });

  test("builds waiting-input notification request with custom panel title", () => {
    const ws = workspace("panel-1", "session-1");
    ws.process_title = "  Phoenix  ";
    ws.panel_titles = [
      {
        panel_id: "panel-1",
        custom_title: "  API logs  ",
      },
    ];

    expect(agentAttentionNotificationRequest("panel-1", [ws])).toEqual({
      workspaceId: "workspace-panel-1",
      panelId: "panel-1",
      workspaceTitle: "Phoenix",
      panelTitle: "API logs",
    });
  });

  test("does not build a delivered notification without a stable workspace id", () => {
    const ws = workspace("panel-1", "session-1");
    ws.workspace_id = undefined;

    expect(agentAttentionNotificationRequest("panel-1", [ws])).toBeNull();
  });
});
