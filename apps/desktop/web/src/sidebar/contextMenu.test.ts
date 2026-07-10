import { describe, expect, test } from "bun:test";

import {
  buildWorkspaceContextMenuItems,
  contextMenuKeyForTarget,
  contextMenuTargetMatches,
  type WorkspaceContextMenuTarget,
} from "./contextMenu";

describe("workspace context menu contract", () => {
  test("workspace rows expose the shared action order and pin toggle", () => {
    const target: WorkspaceContextMenuTarget = {
      kind: "workspace",
      workspaceId: "w1",
      title: "Build",
      isPinned: false,
      canCreateWorkspace: true,
      canRename: true,
      canPin: true,
      canClose: true,
      canCloseOthers: true,
    };

    const items = buildWorkspaceContextMenuItems(target);

    expect(items.map((item) => item.label)).toEqual([
      "New Workspace",
      "Rename Workspace",
      "Pin Workspace",
      "Close Workspace",
      "Close Other Workspaces",
    ]);
    expect(items[2]?.action).toEqual({
      kind: "setWorkspacePinned",
      workspaceId: "w1",
      pinned: true,
    });
    expect(items[3]?.destructive).toBe(true);
    expect(items[3]?.dividerBefore).toBe(true);
  });

  test("pinned workspace rows flip to the unpin action", () => {
    const items = buildWorkspaceContextMenuItems({
      kind: "workspace",
      workspaceId: "w1",
      title: "Build",
      isPinned: true,
      canCreateWorkspace: true,
      canRename: true,
      canPin: true,
      canClose: true,
      canCloseOthers: true,
    });

    expect(items[2]).toMatchObject({
      label: "Unpin Workspace",
      action: { kind: "setWorkspacePinned", workspaceId: "w1", pinned: false },
    });
  });

  test("disabled capabilities stay visible but inert", () => {
    const items = buildWorkspaceContextMenuItems({
      kind: "workspace",
      workspaceId: "w1",
      title: "Only",
      isPinned: false,
      canCreateWorkspace: true,
      canRename: false,
      canPin: false,
      canClose: false,
      canCloseOthers: false,
    });

    expect(items.filter((item) => item.disabled).map((item) => item.id)).toEqual([
      "rename-workspace",
      "toggle-workspace-pin",
      "close-workspace",
      "close-other-workspaces",
    ]);
  });

  test("group menus expose collapse and close-group actions", () => {
    const items = buildWorkspaceContextMenuItems({
      kind: "group",
      groupId: "g1",
      title: "Backend",
      isCollapsed: false,
      memberWorkspaceIds: ["w1", "w2"],
      canCreateWorkspace: true,
      canCollapse: true,
      canClose: true,
    });

    expect(items.map((item) => item.label)).toEqual([
      "New Workspace",
      "Collapse Group",
      "Close Group",
    ]);
    expect(items[1]?.action).toEqual({
      kind: "setGroupCollapsed",
      groupId: "g1",
      collapsed: true,
    });
    expect(items[2]?.action).toEqual({
      kind: "closeGroupWorkspaces",
      groupId: "g1",
      workspaceIds: ["w1", "w2"],
    });
  });

  test("target keys distinguish workspace and group identities", () => {
    const workspace: WorkspaceContextMenuTarget = {
      kind: "workspace",
      workspaceId: "same",
      title: "Same",
      isPinned: false,
      canCreateWorkspace: true,
      canRename: true,
      canPin: true,
      canClose: true,
      canCloseOthers: true,
    };
    const group: WorkspaceContextMenuTarget = {
      kind: "group",
      groupId: "same",
      title: "Same",
      isCollapsed: true,
      memberWorkspaceIds: ["same"],
      canCreateWorkspace: true,
      canCollapse: true,
      canClose: true,
    };

    expect(contextMenuKeyForTarget(workspace)).toEqual({
      kind: "workspace",
      workspaceId: "same",
    });
    expect(contextMenuTargetMatches(contextMenuKeyForTarget(workspace), group))
      .toBe(false);
    expect(contextMenuTargetMatches(contextMenuKeyForTarget(group), group)).toBe(
      true,
    );
  });
});
