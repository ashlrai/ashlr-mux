import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

import {
  panelIdsInLayout,
  sessionIndexEntries,
  SessionsIndexPanel,
} from "./SessionsIndexPanel";

const pane = (panelIds: string[], selectedPanelId = panelIds[0]) => ({
  type: "pane" as const,
  pane: {
    pane_id: `pane-${panelIds.join("-")}`,
    panel_ids: panelIds,
    selected_panel_id: selectedPanelId,
  },
});

const workspace = (
  patch: Partial<SessionWorkspaceSnapshot>,
): SessionWorkspaceSnapshot => ({
  process_title: "Shell",
  layout: pane(["panel-1"]),
  ...patch,
});

describe("SessionsIndexPanel", () => {
  test("panelIdsInLayout walks nested split layouts", () => {
    expect(
      panelIdsInLayout({
        type: "split",
        split: {
          orientation: "horizontal",
          divider_position: 0.5,
          first: pane(["panel-a", "panel-b"], "panel-b"),
          second: pane(["panel-c"]),
        },
      }),
    ).toEqual(["panel-a", "panel-b", "panel-c"]);
  });

  test("sessionIndexEntries projects live workspace state", () => {
    const entries = sessionIndexEntries(
      [
        workspace({
          workspace_id: "workspace-alpha",
          custom_title: " Alpha ",
          custom_description: "investigate prod issue",
          current_directory: "C:/repo",
          is_pinned: true,
          panel_unreads: [{ panel_id: "panel-1", is_unread: true }],
          layout_mode: "canvas",
        }),
        workspace({ process_title: "Beta Shell", layout: null }),
      ],
      0,
    );

    expect(entries[0]).toMatchObject({
      id: "workspace-alpha",
      title: "Alpha",
      description: "investigate prod issue",
      directory: "C:/repo",
      panelCount: 1,
      isSelected: true,
      isPinned: true,
      hasUnread: true,
      layoutMode: "Canvas",
    });
    expect(entries[1]).toMatchObject({
      title: "Beta Shell",
      panelCount: 0,
      isSelected: false,
      layoutMode: "Splits",
    });
  });

  test("renders a selectable Vault list", () => {
    const markup = renderToStaticMarkup(
      <SessionsIndexPanel
        workspaces={[
          workspace({
            custom_title: "Docs",
            current_directory: "C:/docs",
          }),
        ]}
        selectedWorkspaceIndex={0}
        onSelectWorkspace={() => {}}
      />,
    );

    expect(markup).toContain('aria-label="Vault sessions"');
    expect(markup).toContain("Docs");
    expect(markup).toContain("C:/docs");
    expect(markup).toContain('aria-current="true"');
  });
});
