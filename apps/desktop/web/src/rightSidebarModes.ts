export type RightSidebarMode = "files" | "find" | "sessions" | "feed";

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

const CORE_RIGHT_SIDEBAR_MODE_ITEMS: readonly RightSidebarModeItem[] = [
  { mode: "files", label: "Files" },
  { mode: "find", label: "Find" },
  { mode: "sessions", label: "Vault" },
];

export interface RightSidebarModeAvailability {
  feedEnabled: boolean;
  dockEnabled: boolean;
}

export function rightSidebarModeItems(
  availability: RightSidebarModeAvailability,
): readonly RightSidebarModeItem[] {
  return availability.feedEnabled
    ? [...CORE_RIGHT_SIDEBAR_MODE_ITEMS, { mode: "feed", label: "Feed" }]
    : CORE_RIGHT_SIDEBAR_MODE_ITEMS;
}

export function isRightSidebarMode(value: unknown): value is RightSidebarMode {
  return (
    value === "files" ||
    value === "find" ||
    value === "sessions" ||
    value === "feed"
  );
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
