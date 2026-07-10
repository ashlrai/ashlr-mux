export type RightSidebarMode = "files" | "find" | "sessions";

export interface RightSidebarModeItem {
  mode: RightSidebarMode;
  label: string;
}

export const RIGHT_SIDEBAR_MODE_ITEMS: readonly RightSidebarModeItem[] = [
  { mode: "files", label: "Files" },
  { mode: "find", label: "Find" },
  { mode: "sessions", label: "Vault" },
];
