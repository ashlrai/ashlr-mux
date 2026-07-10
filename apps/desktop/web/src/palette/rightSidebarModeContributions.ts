import {
  rightSidebarModeItems,
  type RightSidebarModeAvailability,
} from "../rightSidebarModes";
import type { CommandContribution } from "./commandCatalog";

export function buildRightSidebarModeContributions(
  availability: RightSidebarModeAvailability = {
    feedEnabled: false,
    dockEnabled: false,
  },
): CommandContribution[] {
  return rightSidebarModeItems(availability).map((item, index) => ({
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
