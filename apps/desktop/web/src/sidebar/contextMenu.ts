export type WorkspaceContextMenuAction =
  | { kind: "newWorkspace" }
  | { kind: "beginRenameWorkspace"; workspaceId: string }
  | { kind: "setWorkspacePinned"; workspaceId: string; pinned: boolean }
  | { kind: "closeWorkspace"; workspaceId: string }
  | { kind: "closeOtherWorkspaces"; workspaceId: string }
  | { kind: "setGroupCollapsed"; groupId: string; collapsed: boolean }
  | {
      kind: "closeGroupWorkspaces";
      groupId: string;
      workspaceIds: readonly string[];
    };

export type WorkspaceContextMenuTargetKey =
  | { kind: "workspace"; workspaceId: string }
  | { kind: "group"; groupId: string };

export type WorkspaceContextMenuTarget =
  | {
      kind: "workspace";
      workspaceId: string;
      title: string;
      isPinned: boolean;
      canCreateWorkspace: boolean;
      canRename: boolean;
      canPin: boolean;
      canClose: boolean;
      canCloseOthers: boolean;
    }
  | {
      kind: "group";
      groupId: string;
      title: string;
      isCollapsed: boolean;
      memberWorkspaceIds: readonly string[];
      canCreateWorkspace: boolean;
      canCollapse: boolean;
      canClose: boolean;
    };

export interface WorkspaceContextMenuItem {
  id: string;
  label: string;
  action: WorkspaceContextMenuAction;
  disabled?: boolean;
  destructive?: boolean;
  dividerBefore?: boolean;
}

export function contextMenuTargetMatches(
  key: WorkspaceContextMenuTargetKey | null,
  target: WorkspaceContextMenuTarget,
): boolean {
  if (key === null) {
    return false;
  }
  if (target.kind === "workspace") {
    return key.kind === "workspace" && key.workspaceId === target.workspaceId;
  }
  return key.kind === "group" && key.groupId === target.groupId;
}

export function contextMenuKeyForTarget(
  target: WorkspaceContextMenuTarget,
): WorkspaceContextMenuTargetKey {
  if (target.kind === "workspace") {
    return { kind: "workspace", workspaceId: target.workspaceId };
  }
  return { kind: "group", groupId: target.groupId };
}

export function buildWorkspaceContextMenuItems(
  target: WorkspaceContextMenuTarget,
): readonly WorkspaceContextMenuItem[] {
  if (target.kind === "workspace") {
    return [
      {
        id: "new-workspace",
        label: "New Workspace",
        action: { kind: "newWorkspace" },
        disabled: !target.canCreateWorkspace,
      },
      {
        id: "rename-workspace",
        label: "Rename Workspace",
        action: {
          kind: "beginRenameWorkspace",
          workspaceId: target.workspaceId,
        },
        disabled: !target.canRename,
      },
      {
        id: "toggle-workspace-pin",
        label: target.isPinned ? "Unpin Workspace" : "Pin Workspace",
        action: {
          kind: "setWorkspacePinned",
          workspaceId: target.workspaceId,
          pinned: !target.isPinned,
        },
        disabled: !target.canPin,
      },
      {
        id: "close-workspace",
        label: "Close Workspace",
        action: { kind: "closeWorkspace", workspaceId: target.workspaceId },
        disabled: !target.canClose,
        destructive: true,
        dividerBefore: true,
      },
      {
        id: "close-other-workspaces",
        label: "Close Other Workspaces",
        action: {
          kind: "closeOtherWorkspaces",
          workspaceId: target.workspaceId,
        },
        disabled: !target.canCloseOthers,
        destructive: true,
      },
    ];
  }

  return [
    {
      id: "new-workspace",
      label: "New Workspace",
      action: { kind: "newWorkspace" },
      disabled: !target.canCreateWorkspace,
    },
    {
      id: "toggle-group-collapse",
      label: target.isCollapsed ? "Expand Group" : "Collapse Group",
      action: {
        kind: "setGroupCollapsed",
        groupId: target.groupId,
        collapsed: !target.isCollapsed,
      },
      disabled: !target.canCollapse,
    },
    {
      id: "close-group",
      label: "Close Group",
      action: {
        kind: "closeGroupWorkspaces",
        groupId: target.groupId,
        workspaceIds: target.memberWorkspaceIds,
      },
      disabled: !target.canClose || target.memberWorkspaceIds.length === 0,
      destructive: true,
      dividerBefore: true,
    },
  ];
}
