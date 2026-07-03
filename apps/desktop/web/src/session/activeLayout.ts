import type {
  AppSessionSnapshot,
  SessionWorkspaceLayoutSnapshot,
} from "@cmux/core-types";

/**
 * The layout tree of the selected workspace of the first window, or `null`.
 *
 * Pure and React-free so it unit-tests headlessly; `useSession` re-imports it.
 * Selection mirrors the macOS session model: the first window's tab manager,
 * its `selected_workspace_index` (defaulting to 0), clamped back to the first
 * workspace when out of range, then that workspace's `layout` (which may be
 * `null` while a workspace is still materializing).
 */
export function activeLayoutOf(
  snapshot: AppSessionSnapshot | null,
): SessionWorkspaceLayoutSnapshot | null {
  const firstWindow = snapshot?.windows[0];
  if (!firstWindow) {
    return null;
  }
  const tabs = firstWindow.tab_manager;
  const index = tabs.selected_workspace_index ?? 0;
  const workspace = tabs.workspaces[index] ?? tabs.workspaces[0];
  return workspace?.layout ?? null;
}
