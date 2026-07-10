import { describe, expect, test } from "bun:test";

import type {
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import { planSidebarWorkspaceDrop, sidebarReorderUsesTopLevelRows } from "./dragReorder";
import { projectSidebarItems } from "./snapshotProjection";

const IDS = {
  group: "11111111-1111-1111-1111-111111111111",
  anchor: "22222222-2222-2222-2222-222222222222",
  member: "33333333-3333-3333-3333-333333333333",
  solo: "44444444-4444-4444-4444-444444444444",
  other: "55555555-5555-5555-5555-555555555555",
} as const;

function workspace(
  id: string,
  overrides: Partial<SessionWorkspaceSnapshot> = {},
): SessionWorkspaceSnapshot {
  return { workspace_id: id, process_title: "Terminal", layout: null, ...overrides };
}

function group(
  overrides: Partial<SessionWorkspaceGroupSnapshot> = {},
): SessionWorkspaceGroupSnapshot {
  return {
    id: IDS.group,
    name: "Backend",
    is_collapsed: false,
    anchor_workspace_id: IDS.anchor,
    ...overrides,
  };
}

describe("sidebar drag reorder planning", () => {
  test("ungrouped rows plan in raw workspace index space", () => {
    const workspaces = [
      workspace(IDS.anchor),
      workspace(IDS.member),
      workspace(IDS.solo),
    ];
    const items = projectSidebarItems({ workspaces });

    expect(
      planSidebarWorkspaceDrop({
        draggedWorkspaceId: IDS.solo,
        targetWorkspaceId: IDS.anchor,
        edge: "top",
        workspaces,
        items,
      }),
    ).toEqual({ index: 2, toIndex: 0, usesTopLevelRows: false });
  });

  test("group anchors plan in top-level row space", () => {
    const workspaces = [
      workspace(IDS.anchor, { group_id: IDS.group }),
      workspace(IDS.member, { group_id: IDS.group }),
      workspace(IDS.solo),
    ];
    const items = projectSidebarItems({ workspaces, workspace_groups: [group()] });

    expect(
      planSidebarWorkspaceDrop({
        draggedWorkspaceId: IDS.anchor,
        targetWorkspaceId: IDS.solo,
        edge: "bottom",
        workspaces,
        items,
      }),
    ).toEqual({ index: 0, toIndex: 2, usesTopLevelRows: true });
  });

  test("dragging a grouped child over top-level space promotes it to top-level rows", () => {
    const workspaces = [
      workspace(IDS.anchor, { group_id: IDS.group }),
      workspace(IDS.member, { group_id: IDS.group }),
      workspace(IDS.solo),
    ];
    const items = projectSidebarItems({ workspaces, workspace_groups: [group()] });

    expect(
      sidebarReorderUsesTopLevelRows({
        draggedWorkspaceId: IDS.member,
        targetWorkspaceId: IDS.solo,
        workspaces,
        items,
      }),
    ).toBe(true);
    expect(
      planSidebarWorkspaceDrop({
        draggedWorkspaceId: IDS.member,
        targetWorkspaceId: IDS.solo,
        edge: "bottom",
        workspaces,
        items,
      }),
    ).toEqual({ index: 1, toIndex: 3, usesTopLevelRows: true });
  });

  test("dragging a grouped child within its group stays in raw workspace space", () => {
    const workspaces = [
      workspace(IDS.anchor, { group_id: IDS.group }),
      workspace(IDS.member, { group_id: IDS.group }),
      workspace(IDS.other, { group_id: IDS.group }),
      workspace(IDS.solo),
    ];
    const items = projectSidebarItems({ workspaces, workspace_groups: [group()] });

    expect(
      sidebarReorderUsesTopLevelRows({
        draggedWorkspaceId: IDS.other,
        targetWorkspaceId: IDS.member,
        workspaces,
        items,
      }),
    ).toBe(false);
    expect(
      planSidebarWorkspaceDrop({
        draggedWorkspaceId: IDS.other,
        targetWorkspaceId: IDS.member,
        edge: "top",
        workspaces,
        items,
      }),
    ).toEqual({ index: 2, toIndex: 1, usesTopLevelRows: false });
  });
});
