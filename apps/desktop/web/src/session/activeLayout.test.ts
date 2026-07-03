import { describe, expect, test } from "bun:test";

import type {
  AppSessionSnapshot,
  SessionWorkspaceLayoutSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import { activeLayoutOf } from "./activeLayout";

function paneLayout(id: string): SessionWorkspaceLayoutSnapshot {
  return { type: "pane", pane: { panel_ids: [id] } };
}

function workspace(
  layout: SessionWorkspaceLayoutSnapshot | null,
  title = "ws",
): SessionWorkspaceSnapshot {
  return { process_title: title, layout };
}

function snapshot(
  windows: Array<{ selected_workspace_index?: number; workspaces: SessionWorkspaceSnapshot[] }>,
): AppSessionSnapshot {
  return {
    version: 1,
    created_at: 0,
    windows: windows.map((w) => ({
      tab_manager: {
        selected_workspace_index: w.selected_workspace_index,
        workspaces: w.workspaces,
      },
    })),
  };
}

describe("activeLayoutOf", () => {
  test("a null snapshot is null", () => {
    expect(activeLayoutOf(null)).toBeNull();
  });

  test("no windows is null", () => {
    expect(activeLayoutOf(snapshot([]))).toBeNull();
  });

  test("defaults the selected workspace index to 0 when absent", () => {
    const layout = paneLayout("a");
    const snap = snapshot([{ workspaces: [workspace(layout), workspace(paneLayout("b"))] }]);
    expect(activeLayoutOf(snap)).toBe(layout);
  });

  test("honors selected_workspace_index", () => {
    const target = paneLayout("b");
    const snap = snapshot([
      { selected_workspace_index: 1, workspaces: [workspace(paneLayout("a")), workspace(target)] },
    ]);
    expect(activeLayoutOf(snap)).toBe(target);
  });

  test("an out-of-range index falls back to the first workspace", () => {
    const first = paneLayout("a");
    const snap = snapshot([
      { selected_workspace_index: 5, workspaces: [workspace(first), workspace(paneLayout("b"))] },
    ]);
    expect(activeLayoutOf(snap)).toBe(first);
  });

  test("a null layout passes through as null", () => {
    const snap = snapshot([{ selected_workspace_index: 0, workspaces: [workspace(null)] }]);
    expect(activeLayoutOf(snap)).toBeNull();
  });

  test("picks the first window when there are several", () => {
    const firstWindowLayout = paneLayout("w0");
    const snap = snapshot([
      { workspaces: [workspace(firstWindowLayout)] },
      { workspaces: [workspace(paneLayout("w1"))] },
    ]);
    expect(activeLayoutOf(snap)).toBe(firstWindowLayout);
  });
});
