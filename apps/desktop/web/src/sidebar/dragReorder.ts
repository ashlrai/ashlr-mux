import type {
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import type { SidebarWorkspaceRenderItem } from "./renderItems";
import { workspaceIdKey } from "./snapshotProjection";

export type SidebarDropEdge = "top" | "bottom";

export interface SidebarWorkspaceDropPlan {
  index: number;
  toIndex: number;
  usesTopLevelRows: boolean;
}

function groupAnchorByGroupId(
  items: readonly SidebarWorkspaceRenderItem[],
): Map<string, string> {
  const anchors = new Map<string, string>();
  for (const item of items) {
    if (item.kind === "groupHeader") {
      anchors.set(item.group.id, item.group.anchorWorkspaceId);
    }
  }
  return anchors;
}

function rawIndexByWorkspaceId(
  workspaces: readonly SessionWorkspaceSnapshot[],
): Map<string, number> {
  const indices = new Map<string, number>();
  workspaces.forEach((workspace, index) => {
    const id = workspaceIdKey(workspace.workspace_id);
    if (id !== undefined && !indices.has(id)) {
      indices.set(id, index);
    }
  });
  return indices;
}

function workspaceGroupIdByWorkspaceId(
  workspaces: readonly SessionWorkspaceSnapshot[],
): Map<string, string | undefined> {
  const groups = new Map<string, string | undefined>();
  for (const workspace of workspaces) {
    const workspaceId = workspaceIdKey(workspace.workspace_id);
    if (workspaceId === undefined || groups.has(workspaceId)) {
      continue;
    }
    groups.set(workspaceId, workspaceIdKey(workspace.group_id));
  }
  return groups;
}

function sidebarTopLevelWorkspaceIds(args: {
  workspaces: readonly SessionWorkspaceSnapshot[];
  groupAnchorById: ReadonlyMap<string, string>;
  promotingWorkspaceId?: string;
}): string[] {
  const { workspaces, groupAnchorById, promotingWorkspaceId } = args;
  const ids: string[] = [];
  const emittedGroupIds = new Set<string>();

  for (const workspace of workspaces) {
    const workspaceId = workspaceIdKey(workspace.workspace_id);
    if (workspaceId === undefined) {
      continue;
    }
    const groupId = workspaceIdKey(workspace.group_id);
    const anchorWorkspaceId =
      groupId !== undefined ? groupAnchorById.get(groupId) : undefined;
    if (groupId !== undefined && anchorWorkspaceId !== undefined) {
      if (!emittedGroupIds.has(groupId)) {
        emittedGroupIds.add(groupId);
        ids.push(anchorWorkspaceId);
      }
    } else {
      ids.push(workspaceId);
    }
  }

  if (promotingWorkspaceId !== undefined && !ids.includes(promotingWorkspaceId)) {
    const promotingWorkspace = workspaces.find(
      (workspace) => workspaceIdKey(workspace.workspace_id) === promotingWorkspaceId,
    );
    const promotingGroupId = workspaceIdKey(promotingWorkspace?.group_id);
    const anchorWorkspaceId =
      promotingGroupId !== undefined ? groupAnchorById.get(promotingGroupId) : undefined;
    if (anchorWorkspaceId !== undefined) {
      const anchorIndex = ids.indexOf(anchorWorkspaceId);
      if (anchorIndex !== -1) {
        ids.splice(anchorIndex + 1, 0, promotingWorkspaceId);
      }
    }
  }

  return ids;
}

export function sidebarReorderUsesTopLevelRows(args: {
  draggedWorkspaceId: string;
  targetWorkspaceId?: string;
  workspaces: readonly SessionWorkspaceSnapshot[];
  items: readonly SidebarWorkspaceRenderItem[];
}): boolean {
  const { draggedWorkspaceId, targetWorkspaceId, workspaces, items } = args;
  const groupAnchors = new Set(
    items
      .filter(
        (item): item is Extract<SidebarWorkspaceRenderItem, { kind: "groupHeader" }> =>
          item.kind === "groupHeader",
      )
      .map((item) => item.group.anchorWorkspaceId),
  );
  if (groupAnchors.has(draggedWorkspaceId)) {
    return true;
  }
  if (targetWorkspaceId !== undefined && groupAnchors.has(targetWorkspaceId)) {
    return true;
  }

  const groupsByWorkspaceId = workspaceGroupIdByWorkspaceId(workspaces);
  const draggedGroupId = groupsByWorkspaceId.get(draggedWorkspaceId);
  if (draggedGroupId === undefined) {
    return false;
  }
  if (targetWorkspaceId === undefined) {
    return true;
  }
  return groupsByWorkspaceId.get(targetWorkspaceId) === undefined;
}

export function planSidebarWorkspaceDrop(args: {
  draggedWorkspaceId: string;
  targetWorkspaceId: string;
  edge: SidebarDropEdge;
  workspaces: readonly SessionWorkspaceSnapshot[];
  items: readonly SidebarWorkspaceRenderItem[];
}): SidebarWorkspaceDropPlan | null {
  const {
    draggedWorkspaceId,
    targetWorkspaceId,
    edge,
    workspaces,
    items,
  } = args;
  const rawIndices = rawIndexByWorkspaceId(workspaces);
  const index = rawIndices.get(draggedWorkspaceId);
  if (index === undefined) {
    return null;
  }

  const groupAnchors = groupAnchorByGroupId(items);
  const groupsByWorkspaceId = workspaceGroupIdByWorkspaceId(workspaces);
  const usesTopLevelRows = sidebarReorderUsesTopLevelRows({
    draggedWorkspaceId,
    targetWorkspaceId,
    workspaces,
    items,
  });

  if (!usesTopLevelRows) {
    const targetIndex = rawIndices.get(targetWorkspaceId);
    if (targetIndex === undefined) {
      return null;
    }
    return {
      index,
      toIndex: targetIndex + (edge === "bottom" ? 1 : 0),
      usesTopLevelRows: false,
    };
  }

  const targetGroupId = groupsByWorkspaceId.get(targetWorkspaceId);
  const topLevelTargetId =
    targetGroupId !== undefined
      ? (groupAnchors.get(targetGroupId) ?? targetWorkspaceId)
      : targetWorkspaceId;
  const topLevelIds = sidebarTopLevelWorkspaceIds({
    workspaces,
    groupAnchorById: groupAnchors,
    promotingWorkspaceId: draggedWorkspaceId,
  });
  const targetIndex = topLevelIds.indexOf(topLevelTargetId);
  if (targetIndex === -1) {
    return null;
  }
  return {
    index,
    toIndex: targetIndex + (edge === "bottom" ? 1 : 0),
    usesTopLevelRows: true,
  };
}
