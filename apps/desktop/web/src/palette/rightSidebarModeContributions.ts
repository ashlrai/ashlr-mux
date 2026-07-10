import {
  RIGHT_SIDEBAR_MODE_ITEMS,
} from "../rightSidebarModes";
import type { CommandContribution } from "./commandCatalog";

export function buildRightSidebarModeContributions(): CommandContribution[] {
  return RIGHT_SIDEBAR_MODE_ITEMS.map((item, index) => ({
    commandId: `palette.rightSidebar.${item.mode}`,
    title: () => `Show Sidebar ${item.label}`,
    subtitle: () => "Right Sidebar",
    shortcutHint: `⌃${index + 1}`,
    keywords: [
      "right",
      "sidebar",
      item.mode,
      item.label.toLowerCase(),
      ...(item.mode === "sessions" ? ["sessions", "vault"] : []),
    ],
    dismissOnRun: true,
    when: (ctx) => ctx.hasWorkspace === true,
    enablement: (ctx) => ctx.hasWorkspace === true,
    intent: { kind: "rightSidebarMode", mode: item.mode },
  }));
}
