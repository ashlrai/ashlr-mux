// Oracle tests for switcherIndex.ts, pinned to
// `CommandPaletteSwitcherSearchIndexer.swift:8-141`. The full-pipeline case
// mirrors the Swift fixture inputs at
// `CommandPaletteTests/CommandPaletteSearchEngineTests.swift:75-94`
// (makeSwitcherEntries, index 0); expectations are hand-derived from the
// Swift algorithm.
//
// NOTE on ordering: per Indexer.swift:53-67, ALL gated context words come
// first (directory, then branch, then port, then description families) and
// only THEN the token groups in the same family order — the families are not
// interleaved with their tokens.

import { describe, expect, test } from "bun:test";

import {
  abbreviateWithTilde,
  foldKey,
  lastPathComponent,
  splitOnMetadataDelimiters,
  standardizePathLexically,
  switcherSearchKeywords,
} from "./switcherIndex";

describe("switcherSearchKeywords — full pipeline (Swift fixture, workspace detail)", () => {
  test("base + directory + branch + ports produce the exact ordered list", () => {
    const dir = "/Users/example/dev/cmuxterm-hq/worktrees/feature-0-rename-tab";
    expect(
      switcherSearchKeywords(
        ["workspace", "switch", "go", "Workspace 0 Phoenix"],
        {
          directories: [dir],
          branches: ["feature/rename-tab-0"],
          ports: [3000, 9200],
        },
        "workspace",
      ),
    ).toEqual([
      "workspace",
      "switch",
      "go",
      "Workspace 0 Phoenix",
      // All context words first, in directory/branch/port family order.
      "directory",
      "dir",
      "cwd",
      "path",
      "branch",
      "git",
      "port",
      "ports",
      // Then the token groups: trimmed == canonical == abbreviated (no
      // homeDir) dedupes the directory to one whole-path token.
      dir,
      "feature/rename-tab-0",
      "3000",
      ":3000",
      "9200",
      ":9200",
    ]);
  });
});

describe("switcherSearchKeywords — surface detail tokenization", () => {
  test("surface directories add basename and delimiter components", () => {
    expect(
      switcherSearchKeywords([], { directories: ["/Users/x/dev/my-app.web"] }, "surface"),
    ).toEqual([
      "directory",
      "dir",
      "cwd",
      "path",
      "/Users/x/dev/my-app.web",
      "my-app.web",
      "Users",
      "x",
      "dev",
      "my",
      "app",
      "web",
    ]);
  });

  test("surface branches add delimiter components (Swift fixture branch)", () => {
    expect(
      switcherSearchKeywords([], { branches: ["feature/rename-tab-0"] }, "surface"),
    ).toEqual(["branch", "git", "feature/rename-tab-0", "feature", "rename", "tab", "0"]);
  });

  test("workspace branches keep only the whole trimmed value", () => {
    expect(
      switcherSearchKeywords([], { branches: [" feature/rename-tab-0 "] }, "workspace"),
    ).toEqual(["branch", "git", "feature/rename-tab-0"]);
  });

  test("home-relative surface directory indexes the tilde abbreviation", () => {
    expect(
      switcherSearchKeywords([], { directories: ["/Users/example/dev"] }, "surface", {
        homeDir: "/Users/example",
      }),
    ).toEqual([
      "directory",
      "dir",
      "cwd",
      "path",
      "/Users/example/dev",
      "~/dev",
      // basename "dev" then components; "dev" dedupes against the basename.
      "dev",
      "Users",
      "example",
    ]);
  });
});

describe("switcherSearchKeywords — context-word gating", () => {
  test("empty metadata yields the deduped base only", () => {
    expect(switcherSearchKeywords(["a", "b", "a"], {}, "workspace")).toEqual(["a", "b"]);
  });

  test("each family alone gates only its own context words", () => {
    expect(switcherSearchKeywords([], { directories: ["/d"] }, "workspace")).toEqual([
      "directory",
      "dir",
      "cwd",
      "path",
      "/d",
    ]);
    expect(switcherSearchKeywords([], { branches: ["main"] }, "workspace")).toEqual([
      "branch",
      "git",
      "main",
    ]);
    expect(switcherSearchKeywords([], { ports: [8080] }, "workspace")).toEqual([
      "port",
      "ports",
      "8080",
      ":8080",
    ]);
    expect(switcherSearchKeywords([], { description: "hi" }, "workspace")).toEqual([
      "description",
      "descriptions",
      "notes",
      "note",
      "hi",
    ]);
  });

  test("whitespace-only directory/branch/description gate nothing", () => {
    expect(
      switcherSearchKeywords([], { directories: ["   "], branches: ["\n\t"], description: "  " }, "surface"),
    ).toEqual([]);
  });
});

describe("switcherSearchKeywords — port bounds (Swift 1...65535 + Int typing)", () => {
  test("out-of-range and non-integer ports drop, and drop their context words", () => {
    expect(
      switcherSearchKeywords([], { ports: [0, -1, 65536, 70000, 80.5] }, "workspace"),
    ).toEqual([]);
  });

  test("boundary ports 1 and 65535 are kept", () => {
    expect(switcherSearchKeywords([], { ports: [1, 65535] }, "workspace")).toEqual([
      "port",
      "ports",
      "1",
      ":1",
      "65535",
      ":65535",
    ]);
  });
});

describe("switcherSearchKeywords — dedupe (diacritic + case insensitive, first wins)", () => {
  test("Café variants collapse to the first trimmed original", () => {
    expect(
      switcherSearchKeywords(["Café", "Café", "CAFE", " café "], {}, "workspace"),
    ).toEqual(["Café"]);
  });

  test("dedupe key folds but output keeps original casing", () => {
    expect(switcherSearchKeywords(["Phoenix", "phoenix"], {}, "workspace")).toEqual(["Phoenix"]);
  });
});

describe("switcherSearchKeywords — description tokens", () => {
  test("trims, collapses whitespace runs, and splits components", () => {
    expect(
      switcherSearchKeywords([], { description: "  a\tb\n\nc  " }, "workspace"),
    ).toEqual([
      "description",
      "descriptions",
      "notes",
      "note",
      "a\tb\n\nc", // trimmed original (inner whitespace untouched)
      "a b c", // ICU \s+ runs collapsed to single spaces
      "a",
      "b",
      "c",
    ]);
  });

  test("U+0085 NEL trims at the edges; U+00A0 collapses via ICU \\p{Z}", () => {
    // NEL is in Swift's whitespacesAndNewlines TRIM set but NOT in ICU's \s
    // class (it is Cc, not \p{Z}); NBSP (U+00A0) IS \p{Z} and collapses.
    expect(
      switcherSearchKeywords([], { description: "x y" }, "workspace"),
    ).toEqual([
      "description",
      "descriptions",
      "notes",
      "note",
      "x y", // trimmed original: NEL stripped from ends, NBSP kept inside
      "x y", // NBSP run collapsed to a single space
      "x",
      "y",
    ]);
  });

  test("interior U+000B (VT) survives \u2014 ICU \\s excludes it (Cc, not \\p{Z})", () => {
    // Swift oracle: `\\s+` is ICU `\s` = [\t\n\f\r\p{Z}]; U+000B is Cc and
    // NOT matched, so an INTERIOR VT is preserved. Edge VTs would be trimmed
    // by `.whitespacesAndNewlines`, but this one is interior: the description
    // stays the single token "a\u000Bb". A wrongly-included VT would collapse
    // it to "a b" and split into ["a", "b"].
    expect(
      switcherSearchKeywords([], { description: "ab" }, "workspace"),
    ).toEqual([
      "description",
      "descriptions",
      "notes",
      "note",
      "ab",
    ]);
  });

  test("interior U+0085 (NEL) survives \u2014 ICU \\s excludes it (Cc, not \\p{Z})", () => {
    // Same class as VT: NEL is Cc, outside ICU `\s`, so an interior NEL is
    // preserved rather than collapsed to a space.
    expect(
      switcherSearchKeywords([], { description: "ab" }, "workspace"),
    ).toEqual([
      "description",
      "descriptions",
      "notes",
      "note",
      "ab",
    ]);
  });
});

describe("path helpers", () => {
  test("standardizePathLexically — collapse, dot-removal, absolute-only ..", () => {
    expect(standardizePathLexically("/a//b/./c/")).toBe("/a/b/c");
    expect(standardizePathLexically("/a/b/../c")).toBe("/a/c");
    expect(standardizePathLexically("a/../b")).toBe("a/../b"); // relative: .. kept
    expect(standardizePathLexically("/..")).toBe("/"); // root's parent is root
    expect(standardizePathLexically("/")).toBe("/");
    expect(standardizePathLexically("")).toBe("");
  });

  test("standardizePathLexically — tilde expansion", () => {
    expect(standardizePathLexically("~", "/Users/example")).toBe("/Users/example");
    expect(standardizePathLexically("~/dev", "/Users/example")).toBe("/Users/example/dev");
    expect(standardizePathLexically("~")).toBe("~"); // no homeDir → unchanged
    expect(standardizePathLexically("~bob/x", "/Users/example")).toBe("~bob/x"); // unknown user
  });

  test("standardizePathLexically — Windows-shaped paths pass through", () => {
    expect(standardizePathLexically("C:\\Users\\me\\dev")).toBe("C:\\Users\\me\\dev");
  });

  test("abbreviateWithTilde — component-boundary abbreviation only", () => {
    expect(abbreviateWithTilde("/Users/example", "/Users/example")).toBe("~");
    expect(abbreviateWithTilde("/Users/example/dev", "/Users/example")).toBe("~/dev");
    expect(abbreviateWithTilde("/Users/example2", "/Users/example")).toBe("/Users/example2");
    expect(abbreviateWithTilde("/Users/exam", "/Users/example")).toBe("/Users/exam");
    expect(abbreviateWithTilde("/Users/example/dev")).toBe("/Users/example/dev"); // no homeDir
  });

  test("lastPathComponent — trailing slashes stripped, root stays root", () => {
    expect(lastPathComponent("/a/b/")).toBe("b");
    expect(lastPathComponent("/a/b")).toBe("b");
    expect(lastPathComponent("/")).toBe("/");
    expect(lastPathComponent("plain")).toBe("plain");
  });

  test("directory standardization falls back to the trimmed original when empty", () => {
    // "." standardizes to "" lexically; canonical falls back to "." per the
    // Swift `standardized.isEmpty` guard (Indexer.swift:78).
    expect(switcherSearchKeywords([], { directories: ["."] }, "workspace")).toEqual([
      "directory",
      "dir",
      "cwd",
      "path",
      ".",
    ]);
  });
});

describe("small helpers", () => {
  test("splitOnMetadataDelimiters splits on all 7 delimiters and drops empties", () => {
    expect(splitOnMetadataDelimiters("a/b\\c.d:e_f-g h")).toEqual([
      "a",
      "b",
      "c",
      "d",
      "e",
      "f",
      "g",
      "h",
    ]);
    expect(splitOnMetadataDelimiters("//--__")).toEqual([]);
  });

  test("foldKey strips diacritics and lowercases", () => {
    expect(foldKey("Café")).toBe("cafe");
    expect(foldKey("CAFE")).toBe("cafe");
    expect(foldKey("ÅΩ")).toBe(foldKey("åω"));
  });
});
