import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

const ATTENTION_EVENT_TYPES = new Set([
  "provider.turnComplete",
  "provider.exit",
]);

export interface AgentAttentionNotificationRequest {
  workspaceId: string;
  panelId: string;
  workspaceTitle?: string;
  panelTitle?: string;
}

function nonBlank(value: string | null | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed === undefined || trimmed === "" ? undefined : trimmed;
}

function layoutContainsPanel(
  layout: SessionWorkspaceSnapshot["layout"],
  panelId: string,
): boolean {
  if (layout == null) {
    return false;
  }
  if (layout.type === "pane") {
    return layout.pane.panel_ids.includes(panelId);
  }
  return (
    layoutContainsPanel(layout.split.first, panelId) ||
    layoutContainsPanel(layout.split.second, panelId)
  );
}

function workspaceTitle(workspace: SessionWorkspaceSnapshot): string | undefined {
  return (
    nonBlank(workspace.custom_title) ??
    nonBlank(workspace.process_title) ??
    nonBlank(workspace.workspace_id)
  );
}

function panelTitle(
  workspace: SessionWorkspaceSnapshot,
  panelId: string,
): string | undefined {
  return nonBlank(
    workspace.panel_titles?.find((entry) => entry.panel_id === panelId)
      ?.custom_title,
  );
}

export function agentAttentionPanelForEvent(
  event: unknown,
  workspaces: readonly SessionWorkspaceSnapshot[],
): string | null {
  if (typeof event !== "object" || event === null) {
    return null;
  }
  const eventObject = event as { type?: unknown; sessionId?: unknown };
  if (
    typeof eventObject.type !== "string" ||
    !ATTENTION_EVENT_TYPES.has(eventObject.type)
  ) {
    return null;
  }
  if (
    typeof eventObject.sessionId !== "string" ||
    eventObject.sessionId.trim() === ""
  ) {
    return null;
  }
  for (const workspace of workspaces) {
    for (const entry of workspace.restorable_agent_snapshots ?? []) {
      if (entry.snapshot.session_id === eventObject.sessionId) {
        return entry.panel_id;
      }
    }
  }
  return null;
}

export function agentAttentionNotificationRequest(
  panelId: string,
  workspaces: readonly SessionWorkspaceSnapshot[],
): AgentAttentionNotificationRequest | null {
  for (const workspace of workspaces) {
    if (!layoutContainsPanel(workspace.layout, panelId)) {
      continue;
    }
    const workspaceId = nonBlank(workspace.workspace_id);
    if (workspaceId === undefined) {
      return null;
    }
    return {
      workspaceId,
      panelId,
      workspaceTitle: workspaceTitle(workspace),
      panelTitle: panelTitle(workspace, panelId),
    };
  }
  return null;
}

export function shouldNotifyAgentPanel(
  panelId: string,
  activePanelId: string | undefined,
  documentHasFocus: boolean,
): boolean {
  return !(documentHasFocus && activePanelId === panelId);
}
