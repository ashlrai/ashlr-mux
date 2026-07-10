export type RightSidebarMode = "files" | "find" | "sessions";

export interface RightSidebarState {
  visible: boolean;
  mode: RightSidebarMode;
}

export interface RightSidebarRemotePayload {
  visible?: boolean;
  mode?: unknown;
  focus?: boolean;
}

export interface RightSidebarModeItem {
  mode: RightSidebarMode;
  label: string;
}

export const RIGHT_SIDEBAR_MODE_ITEMS: readonly RightSidebarModeItem[] = [
  { mode: "files", label: "Files" },
  { mode: "find", label: "Find" },
  { mode: "sessions", label: "Vault" },
];

export function isRightSidebarMode(value: unknown): value is RightSidebarMode {
  return value === "files" || value === "find" || value === "sessions";
}

export function rightSidebarStateFromRemote(
  current: RightSidebarState,
  payload: RightSidebarRemotePayload,
): RightSidebarState {
  return {
    visible:
      typeof payload.visible === "boolean" ? payload.visible : current.visible,
    mode: isRightSidebarMode(payload.mode) ? payload.mode : current.mode,
  };
}
