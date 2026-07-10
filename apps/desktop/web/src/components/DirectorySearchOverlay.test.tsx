import { describe, expect, test } from "bun:test";

import {
  activateDirectorySearchResult,
  relativeSearchPath,
  type DirectorySearchResult,
} from "./DirectorySearchOverlay";

describe("relativeSearchPath", () => {
  test("trims a matching workspace directory prefix", () => {
    expect(
      relativeSearchPath(
        "C:\\repo\\src\\main.rs",
        "C:\\repo",
      ),
    ).toBe("src/main.rs");
  });

  test("keeps paths outside the searched directory untouched", () => {
    expect(relativeSearchPath("/tmp/other/file.txt", "/repo")).toBe(
      "/tmp/other/file.txt",
    );
  });
});

describe("activateDirectorySearchResult", () => {
  const result = (path: string): DirectorySearchResult => ({
    path,
    lineNumber: 7,
    lineText: "needle",
  });

  test("opens Markdown matches in the focused pane", () => {
    const opened: Array<{ panelId: string; filePath: string }> = [];
    const statuses: string[] = [];

    activateDirectorySearchResult(result("C:/repo/docs/README.md"), {
      directory: "C:/repo",
      focusedPanelId: "panel-1",
      openMarkdownFile: (panelId, filePath) => opened.push({ panelId, filePath }),
      openFile: () => {
        throw new Error("should not open file");
      },
      setStatus: (status) => statuses.push(status),
    });

    expect(opened).toEqual([
      { panelId: "panel-1", filePath: "C:/repo/docs/README.md" },
    ]);
    expect(statuses).toEqual(["Opened docs/README.md in the focused pane."]);
  });

  test("reports when Markdown activation has no focused pane", () => {
    const statuses: string[] = [];

    activateDirectorySearchResult(result("C:/repo/README.md"), {
      directory: "C:/repo",
      focusedPanelId: undefined,
      openMarkdownFile: () => {
        throw new Error("should not open markdown");
      },
      openFile: () => {
        throw new Error("should not open file");
      },
      setStatus: (status) => statuses.push(status),
    });

    expect(statuses).toEqual([
      "Select a pane before opening a Markdown preview.",
    ]);
  });

  test("opens non-Markdown matches in the focused pane's file editor", () => {
    const opened: Array<{ panelId: string; filePath: string }> = [];
    const statuses: string[] = [];

    activateDirectorySearchResult(result("C:/repo/src/main.rs"), {
      directory: "C:/repo",
      focusedPanelId: "panel-1",
      openMarkdownFile: () => {
        throw new Error("should not open markdown");
      },
      openFile: (panelId, filePath) => opened.push({ panelId, filePath }),
      setStatus: (status) => statuses.push(status),
    });

    expect(opened).toEqual([
      { panelId: "panel-1", filePath: "C:/repo/src/main.rs" },
    ]);
    expect(statuses).toEqual(["Opened src/main.rs in the focused pane."]);
  });

  test("reports when file activation has no focused pane", () => {
    const statuses: string[] = [];

    activateDirectorySearchResult(result("C:/repo/src/main.rs"), {
      directory: "C:/repo",
      focusedPanelId: undefined,
      openMarkdownFile: () => {
        throw new Error("should not open markdown");
      },
      openFile: () => {
        throw new Error("should not open file");
      },
      setStatus: (status) => statuses.push(status),
    });

    expect(statuses).toEqual(["Select a pane before opening a file editor."]);
  });
});
