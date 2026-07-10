import { describe, expect, mock, test } from "bun:test";

const invokeCalls: Array<{ method: string; params: unknown }> = [];

mock.module("./host", () => ({
  host: {
    invoke: async (method: string, params?: unknown) => {
      invokeCalls.push({ method, params });
      return [
        {
          name: "src",
          path: "C:/repo/src",
          relativePath: "src",
          kind: "directory",
          size: null,
        },
      ];
    },
  },
}));

const {
  fileExplorerEntryIcon,
  fileExplorerParentPath,
  fileExplorerSizeLabel,
  isMarkdownFilePath,
  listFileExplorerDirectory,
  openFileExplorerPath,
  readFileExplorerFile,
  writeFileExplorerFile,
} = await import("./fileExplorer");

describe("listFileExplorerDirectory", () => {
  test("invokes the native file explorer list command with a request envelope", async () => {
    invokeCalls.length = 0;
    await expect(
      listFileExplorerDirectory({
        rootPath: "C:/repo",
        relativePath: "src",
        showHidden: true,
      }),
    ).resolves.toEqual([
      {
        name: "src",
        path: "C:/repo/src",
        relativePath: "src",
        kind: "directory",
        size: null,
      },
    ]);
    expect(invokeCalls).toEqual([
      {
        method: "file_explorer_list_directory",
        params: {
          request: {
            rootPath: "C:/repo",
            relativePath: "src",
            showHidden: true,
          },
        },
      },
    ]);
  });
});

describe("openFileExplorerPath", () => {
  test("invokes the native file opener with preferred editor settings", async () => {
    invokeCalls.length = 0;
    await openFileExplorerPath({
      path: "C:/repo/README.md",
      preferredEditor: "code -r",
    });

    expect(invokeCalls).toEqual([
      {
        method: "file_explorer_open_path",
        params: {
          request: {
            path: "C:/repo/README.md",
            preferredEditor: "code -r",
          },
        },
      },
    ]);
  });
});

describe("readFileExplorerFile", () => {
  test("invokes the native text file reader with a request envelope", async () => {
    invokeCalls.length = 0;
    await readFileExplorerFile({ path: "C:/repo/src/main.rs" });

    expect(invokeCalls).toEqual([
      {
        method: "file_explorer_read_file",
        params: {
          request: {
            path: "C:/repo/src/main.rs",
          },
        },
      },
    ]);
  });
});

describe("writeFileExplorerFile", () => {
  test("invokes the native text file writer with a request envelope", async () => {
    invokeCalls.length = 0;
    await writeFileExplorerFile({
      path: "C:/repo/src/main.rs",
      content: "fn main() {}\n",
    });

    expect(invokeCalls).toEqual([
      {
        method: "file_explorer_write_file",
        params: {
          request: {
            path: "C:/repo/src/main.rs",
            content: "fn main() {}\n",
          },
        },
      },
    ]);
  });
});

describe("file explorer row helpers", () => {
  test("recognizes supported Markdown extensions case-insensitively", () => {
    expect(isMarkdownFilePath("README.md")).toBe(true);
    expect(isMarkdownFilePath("notes.MDX")).toBe(true);
    expect(isMarkdownFilePath("archive.tar.gz")).toBe(false);
    expect(isMarkdownFilePath("Makefile")).toBe(false);
  });

  test("maps entry kind to stable text icons", () => {
    expect(fileExplorerEntryIcon("directory")).toBe("folder");
    expect(fileExplorerEntryIcon("file")).toBe("file");
    expect(fileExplorerEntryIcon("symlink")).toBe("link");
    expect(fileExplorerEntryIcon("other")).toBe("item");
  });

  test("derives parent paths for nested relative paths", () => {
    expect(fileExplorerParentPath("src/components/App.tsx")).toBe("src/components");
    expect(fileExplorerParentPath("src")).toBe("");
    expect(fileExplorerParentPath("")).toBeNull();
  });

  test("formats compact file sizes", () => {
    expect(fileExplorerSizeLabel(null)).toBe("");
    expect(fileExplorerSizeLabel(42)).toBe("42 B");
    expect(fileExplorerSizeLabel(1536)).toBe("1.5 KB");
    expect(fileExplorerSizeLabel(20 * 1024)).toBe("20 KB");
    expect(fileExplorerSizeLabel(2 * 1024 * 1024)).toBe("2.0 MB");
  });
});
