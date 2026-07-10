import { describe, expect, mock, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

mock.module("../hooks/useSession", () => ({
  useSession: () => ({
    activeLayout: null,
    workspaces: [
      {
        workspace_id: "workspace-1",
        process_title: "Workspace One",
        custom_title: "Project Alpha",
        current_directory: "C:/repo",
        layout: {
          type: "pane",
          pane: {
            pane_id: "pane-1",
            panel_ids: ["panel-1"],
            selected_panel_id: "panel-1",
          },
        },
      },
    ],
    selectedWorkspaceIndex: 0,
    openMarkdownFile: () => {},
    openFile: () => {},
    selectWorkspace: () => {},
  }),
}));

mock.module("../session/focusedPane", () => ({
  useFocusedPanelId: () => "panel-1",
}));

const {
  FileExplorerPanel,
  nextFileExplorerSelection,
  rightSidebarWidthStyle,
  selectedFileExplorerEntry,
} = await import("./FileExplorerPanel");

const entries = [
  {
    name: "src",
    path: "C:/repo/src",
    relativePath: "src",
    kind: "directory" as const,
    size: null,
  },
  {
    name: "notes.txt",
    path: "C:/repo/notes.txt",
    relativePath: "notes.txt",
    kind: "file" as const,
    size: 12,
  },
];

describe("FileExplorerPanel", () => {
  test("renders nothing while closed", () => {
    expect(
      renderToStaticMarkup(<FileExplorerPanel open={false} onClose={() => {}} />),
    ).toBe("");
  });

  test("renders the selected workspace root and controls while open", () => {
    const markup = renderToStaticMarkup(
      <FileExplorerPanel open={true} onClose={() => {}} />,
    );
    expect(markup).toContain("Files");
    expect(markup).toContain("C:/repo");
    expect(markup).toContain("Vault");
    expect(markup).toContain("Hidden");
    expect(markup).toContain("Open");
    expect(markup).toContain('aria-label="Close right sidebar"');
  });

  test("applies the right sidebar max-width setting as a width cap", () => {
    const markup = renderToStaticMarkup(
      <FileExplorerPanel
        open={true}
        rightMaxWidth={240}
        onClose={() => {}}
      />,
    );

    expect(markup).toContain('style="flex-basis:240px;max-width:240px"');
  });

  test("renders the Vault sessions surface in sessions mode", () => {
    const markup = renderToStaticMarkup(
      <FileExplorerPanel
        open={true}
        mode="sessions"
        onClose={() => {}}
      />,
    );
    expect(markup).toContain('aria-label="Right Sidebar"');
    expect(markup).toContain('aria-label="Vault sessions"');
    expect(markup).toContain("Project Alpha");
    expect(markup).toContain('aria-selected="true"');
  });

  test("renders Feed only when its persisted beta mode is enabled", () => {
    const markup = renderToStaticMarkup(
      <FileExplorerPanel
        open={true}
        mode="feed"
        feedEnabled={true}
        onClose={() => {}}
      />,
    );
    expect(markup).toContain('aria-label="Show Sidebar Feed"');
    expect(markup).toContain('aria-label="Feed"');
    expect(markup).toContain("No pending decisions");
  });
});

describe("file explorer selection helpers", () => {
  test("rightSidebarWidthStyle ignores unset/invalid values and caps the default width", () => {
    expect(rightSidebarWidthStyle(undefined)).toBeUndefined();
    expect(rightSidebarWidthStyle(0)).toBeUndefined();
    expect(rightSidebarWidthStyle(Number.NaN)).toBeUndefined();
    expect(rightSidebarWidthStyle(240)).toEqual({
      flexBasis: "240px",
      maxWidth: "240px",
    });
    expect(rightSidebarWidthStyle(480)).toEqual({
      flexBasis: "292px",
      maxWidth: "480px",
    });
  });

  test("selectedFileExplorerEntry falls back to the first entry", () => {
    expect(selectedFileExplorerEntry(entries, null)?.relativePath).toBe("src");
    expect(selectedFileExplorerEntry(entries, "missing")?.relativePath).toBe("src");
  });

  test("nextFileExplorerSelection wraps through entries", () => {
    expect(nextFileExplorerSelection(entries, null, 1)).toBe("src");
    expect(nextFileExplorerSelection(entries, null, -1)).toBe("notes.txt");
    expect(nextFileExplorerSelection(entries, "notes.txt", 1)).toBe("src");
    expect(nextFileExplorerSelection(entries, "src", -1)).toBe("notes.txt");
    expect(nextFileExplorerSelection([], null, 1)).toBeNull();
  });
});
