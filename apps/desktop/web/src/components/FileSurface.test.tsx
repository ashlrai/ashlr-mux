import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import {
  FileSurface,
  fileSurfaceTitle,
  shouldHandleFileSaveShortcut,
  shouldProceedWithFileReload,
} from "./FileSurface";

describe("fileSurfaceTitle", () => {
  test("uses the basename of Windows or POSIX paths", () => {
    expect(fileSurfaceTitle("C:\\repo\\src\\main.rs")).toBe("main.rs");
    expect(fileSurfaceTitle("/repo/notes.txt")).toBe("notes.txt");
  });

  test("falls back when no file is selected", () => {
    expect(fileSurfaceTitle()).toBe("No file selected");
    expect(fileSurfaceTitle("   ")).toBe("No file selected");
  });
});

describe("FileSurface", () => {
  test("shouldProceedWithFileReload only prompts when dirty", () => {
    let prompts = 0;
    const confirmDiscard = () => {
      prompts += 1;
      return false;
    };

    expect(shouldProceedWithFileReload(false, confirmDiscard)).toBe(true);
    expect(prompts).toBe(0);
    expect(shouldProceedWithFileReload(true, confirmDiscard)).toBe(false);
    expect(prompts).toBe(1);
    expect(shouldProceedWithFileReload(true, () => true)).toBe(true);
  });

  test("shouldHandleFileSaveShortcut accepts only dirty loaded Ctrl/Cmd+S saves", () => {
    const ready = {
      dirty: true,
      loading: false,
      saving: false,
      hasFile: true,
    };

    expect(shouldHandleFileSaveShortcut({ key: "s", ctrlKey: true }, ready)).toBe(true);
    expect(shouldHandleFileSaveShortcut({ key: "S", metaKey: true }, ready)).toBe(true);
    expect(shouldHandleFileSaveShortcut({ key: "s" }, ready)).toBe(false);
    expect(
      shouldHandleFileSaveShortcut({ key: "s", ctrlKey: true, shiftKey: true }, ready),
    ).toBe(false);
    expect(
      shouldHandleFileSaveShortcut({ key: "s", ctrlKey: true }, { ...ready, dirty: false }),
    ).toBe(false);
    expect(
      shouldHandleFileSaveShortcut({ key: "s", ctrlKey: true }, { ...ready, loading: true }),
    ).toBe(false);
    expect(
      shouldHandleFileSaveShortcut({ key: "s", ctrlKey: true }, { ...ready, saving: true }),
    ).toBe(false);
    expect(
      shouldHandleFileSaveShortcut({ key: "s", ctrlKey: true }, { ...ready, hasFile: false }),
    ).toBe(false);
  });

  test("renders file chrome and word-wrap state", () => {
    const markup = renderToStaticMarkup(
      <FileSurface filePath="C:/repo/notes.txt" wordWrap={true} />,
    );

    expect(markup).toContain("notes.txt");
    expect(markup).toContain("C:/repo/notes.txt");
    expect(markup).toContain("cmux-file-surface-editor is-wrapped");
    expect(markup).toContain("Save");
  });
});
