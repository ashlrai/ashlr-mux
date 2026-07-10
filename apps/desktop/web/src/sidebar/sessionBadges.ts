import type {
  SessionWorkspaceLayoutSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import {
  badgesForWorkspace,
  type BadgeVisibilitySettings,
  type SidebarGitBranchState,
  type SidebarPullRequestState,
  type SidebarShellActivityState,
  type WorkspaceBadges,
} from "./badges";

export function orderedPanelIdsFromLayout(
  layout: SessionWorkspaceLayoutSnapshot | null | undefined,
): string[] {
  if (layout == null) {
    return [];
  }
  if (layout.type === "pane") {
    return [...layout.pane.panel_ids];
  }
  return [
    ...orderedPanelIdsFromLayout(layout.split.first),
    ...orderedPanelIdsFromLayout(layout.split.second),
  ];
}

function panelBranchMap(
  workspace: SessionWorkspaceSnapshot,
): Record<string, SidebarGitBranchState> {
  const branches: Record<string, SidebarGitBranchState> = {};
  for (const state of workspace.panel_git_branches ?? []) {
    if (branches[state.panel_id] === undefined) {
      branches[state.panel_id] = {
        branch: state.branch,
        isDirty: state.is_dirty,
      };
    }
  }
  return branches;
}

function panelPullRequestMap(
  workspace: SessionWorkspaceSnapshot,
): Record<string, SidebarPullRequestState> {
  const pullRequests: Record<string, SidebarPullRequestState> = {};
  for (const state of workspace.panel_pull_requests ?? []) {
    if (pullRequests[state.panel_id] === undefined) {
      pullRequests[state.panel_id] = {
        number: state.number,
        label: state.label,
        url: state.url,
        status: state.status,
        branch: state.branch,
        isStale: state.is_stale,
      };
    }
  }
  return pullRequests;
}

function panelShellActivityMap(
  workspace: SessionWorkspaceSnapshot,
): Record<string, SidebarShellActivityState> {
  const activity: Record<string, SidebarShellActivityState> = {};
  for (const state of workspace.panel_shell_activity ?? []) {
    if (activity[state.panel_id] === undefined) {
      activity[state.panel_id] = state.state;
    }
  }
  return activity;
}

function validPort(port: number | undefined): number | undefined {
  if (port === undefined || !Number.isInteger(port) || port < 1 || port > 65535) {
    return undefined;
  }
  return port;
}

function uniqueSortedPorts(ports: Array<number | undefined>): number[] {
  const seen = new Set<number>();
  for (const rawPort of ports) {
    const port = validPort(rawPort);
    if (port !== undefined) {
      seen.add(port);
    }
  }
  return [...seen].sort((a, b) => a - b);
}

function workspaceListeningPorts(workspace: SessionWorkspaceSnapshot): number[] {
  const aggregate = uniqueSortedPorts(workspace.listening_ports ?? []);
  if (aggregate.length > 0) {
    return aggregate;
  }
  return uniqueSortedPorts(
    [
      ...(workspace.agent_listening_ports ?? []),
      ...(workspace.panel_listening_ports ?? []).flatMap((entry) => entry.ports),
    ],
  );
}

export function badgesForSessionWorkspace(
  workspace: SessionWorkspaceSnapshot,
  settings?: Partial<BadgeVisibilitySettings>,
): WorkspaceBadges {
  return badgesForWorkspace({
    orderedPanelIds: orderedPanelIdsFromLayout(workspace.layout),
    panelGitBranches: panelBranchMap(workspace),
    panelPullRequests: panelPullRequestMap(workspace),
    fallbackBranch:
      workspace.git_branch === undefined
        ? undefined
        : {
            branch: workspace.git_branch.branch,
            isDirty: workspace.git_branch.is_dirty,
          },
    listeningPorts: workspaceListeningPorts(workspace),
    remote:
      workspace.remote === undefined
        ? undefined
        : {
            enabled: workspace.remote.enabled,
            state: workspace.remote.state,
            connected: workspace.remote.connected,
            transport: workspace.remote.transport,
            destination: workspace.remote.destination,
            port: workspace.remote.port,
            localProxyPort: workspace.remote.local_proxy_port,
            hasSshOptions: workspace.remote.has_ssh_options,
            detail: workspace.remote.detail,
            proxyState: workspace.remote.proxy?.state,
            proxyUrl: workspace.remote.proxy?.url,
            detectedPorts: workspace.remote.detected_ports,
            forwardedPorts: workspace.remote.forwarded_ports,
            conflictedPorts: workspace.remote.conflicted_ports,
            activeTerminalSessions: workspace.remote.active_terminal_sessions,
          },
    panelShellActivity: panelShellActivityMap(workspace),
    settings,
  });
}
