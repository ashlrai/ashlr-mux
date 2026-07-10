import { describe, expect, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  appendAttachedFilePaths,
  dispatchTerminalCommand,
  fileUrlToTerminalPath,
  findTerminalLineIndex,
  shouldOpenTerminalLinkInCmuxBrowser,
  TerminalSurface,
  terminalDropPayloadFromDataTransfer,
  terminalDropPathsFromDataTransfer,
  terminalUrlLinksForLine,
  textBoxSendPayload,
} from "./TerminalSurface";

describe("dispatchTerminalCommand", () => {
  test("is SSR-safe when no window exists", () => {
    expect(() =>
      dispatchTerminalCommand("surface-1", "clearScreenKeepScrollback"),
    ).not.toThrow();
  });
});

describe("TerminalSurface", () => {
  test("renders discoverable controls for command-palette terminal actions", () => {
    const markup = renderToStaticMarkup(
      createElement(TerminalSurface, { panelId: "surface-1" }),
    );
    expect(markup).toContain('aria-label="Terminal controls"');
    expect(markup).toContain('aria-label="Find in Terminal"');
    expect(markup).toContain('aria-label="Next Terminal Match"');
    expect(markup).toContain('aria-label="Terminal Text Box Input"');
    expect(markup).toContain('aria-label="Attach File to Terminal Text Box"');
    expect(markup).toContain('aria-label="Clear Terminal Screen"');
    expect(markup).toContain('aria-label="Send Ctrl-F to Terminal"');
  });

  test("renders a loading status while the backend terminal session starts", () => {
    const markup = renderToStaticMarkup(
      createElement(TerminalSurface, { panelId: "surface-1" }),
    );

    expect(markup).toContain('role="status"');
    expect(markup).toContain("Starting terminal...");
    expect(markup).toContain("cmux-terminal-loading-spinner");
  });
});

describe("findTerminalLineIndex", () => {
  test("finds next matches with wraparound", () => {
    const lines = ["alpha", "Beta one", "gamma", "beta two"];
    expect(findTerminalLineIndex(lines, "beta", 0, "next")).toBe(1);
    expect(findTerminalLineIndex(lines, "beta", 2, "next")).toBe(3);
    expect(findTerminalLineIndex(lines, "beta", 4, "next")).toBe(1);
  });

  test("finds previous matches with wraparound", () => {
    const lines = ["alpha", "Beta one", "gamma", "beta two"];
    expect(findTerminalLineIndex(lines, "beta", 2, "previous")).toBe(1);
    expect(findTerminalLineIndex(lines, "beta", 0, "previous")).toBe(3);
  });

  test("returns null for empty or missing queries", () => {
    expect(findTerminalLineIndex(["alpha"], "", 0, "next")).toBeNull();
    expect(findTerminalLineIndex(["alpha"], "beta", 0, "next")).toBeNull();
  });
});

describe("terminal browser links", () => {
  test("detects http links with xterm buffer ranges", () => {
    expect(
      terminalUrlLinksForLine(2, "open https://example.test/docs, then http://localhost:3000"),
    ).toEqual([
      {
        text: "https://example.test/docs",
        range: {
          start: { x: 6, y: 3 },
          end: { x: 30, y: 3 },
        },
      },
      {
        text: "http://localhost:3000",
        range: {
          start: { x: 38, y: 3 },
          end: { x: 58, y: 3 },
        },
      },
    ]);
  });

  test("opens terminal links in cmux only with cmd or ctrl held", () => {
    expect(shouldOpenTerminalLinkInCmuxBrowser({ ctrlKey: false, metaKey: false })).toBe(
      false,
    );
    expect(shouldOpenTerminalLinkInCmuxBrowser({ ctrlKey: true, metaKey: false })).toBe(
      true,
    );
    expect(shouldOpenTerminalLinkInCmuxBrowser({ ctrlKey: false, metaKey: true })).toBe(
      true,
    );
  });
});

describe("terminal TextBox helpers", () => {
  test("textBoxSendPayload ignores blank drafts and appends carriage return", () => {
    expect(textBoxSendPayload("   ")).toBeNull();
    expect(textBoxSendPayload("echo hello")).toBe("echo hello\r");
    expect(textBoxSendPayload("cat <<EOF\nEOF\n")).toBe("cat <<EOF\nEOF\n");
  });

  test("appendAttachedFilePaths appends filesystem paths on separate lines", () => {
    expect(
      appendAttachedFilePaths("echo start", [
        { fsPath: "C:\\tmp\\a.txt" },
        { path: "C:\\tmp\\b.png" },
      ]),
    ).toBe("echo start\nC:\\tmp\\a.txt\nC:\\tmp\\b.png");
    expect(appendAttachedFilePaths("", [{ fsPath: "" }])).toBe("");
  });
});

describe("terminal file drop helpers", () => {
  test("converts file URLs to filesystem paths", () => {
    expect(fileUrlToTerminalPath("file:///C:/Users/A%20B/demo.txt")).toBe(
      "C:\\Users\\A B\\demo.txt",
    );
    expect(fileUrlToTerminalPath("file://server/share/My%20File.txt")).toBe(
      "\\\\server\\share\\My File.txt",
    );
    expect(fileUrlToTerminalPath("https://example.com/demo.txt")).toBeNull();
  });

  test("extracts paths from files, uri-list, and plain text without duplicates", () => {
    const paths = terminalDropPathsFromDataTransfer({
      files: [{ path: "C:\\tmp\\from-file.txt" }],
      getData(format) {
        if (format === "text/uri-list") {
          return "# comment\nfile:///C:/tmp/from-uri.txt\n";
        }
        if (format === "text/plain") {
          return "file:///C:/tmp/from-uri.txt\nfile:///C:/tmp/from-plain.txt";
        }
        return "";
      },
    });

    expect(paths).toEqual([
      "C:\\tmp\\from-file.txt",
      "C:\\tmp\\from-uri.txt",
      "C:\\tmp\\from-plain.txt",
    ]);
  });

  test("builds a terminal paste payload using paths instead of file URLs", () => {
    expect(
      terminalDropPayloadFromDataTransfer({
        getData(format) {
          return format === "text/uri-list"
            ? "file:///C:/tmp/a.txt\nfile:///C:/tmp/My%20File.txt"
            : "";
        },
      }),
    ).toBe('C:\\tmp\\a.txt "C:\\tmp\\My File.txt"');
    expect(terminalDropPayloadFromDataTransfer({ files: [] })).toBeNull();
  });
});
