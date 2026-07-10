import { describe, expect, test } from "bun:test";

import {
  EmptyDiffTokenError,
  diffSurfaceHttpUrl,
  diffSurfaceUrl,
  markdownSurfaceHttpUrl,
  markdownSurfaceUrl,
  normalizeSurfaceKind,
} from "./surfaceUrl";

describe("normalizeSurfaceKind", () => {
  test("maps each recognized surface tag to itself", () => {
    expect(normalizeSurfaceKind("agent")).toBe("agent");
    expect(normalizeSurfaceKind("markdown")).toBe("markdown");
    expect(normalizeSurfaceKind("file")).toBe("file");
    expect(normalizeSurfaceKind("diff")).toBe("diff");
    expect(normalizeSurfaceKind("browser")).toBe("browser");
  });

  test("absent / null / empty fall back to terminal (macOS: absent = terminal)", () => {
    expect(normalizeSurfaceKind(undefined)).toBe("terminal");
    expect(normalizeSurfaceKind(null)).toBe("terminal");
    expect(normalizeSurfaceKind("")).toBe("terminal");
    expect(normalizeSurfaceKind("terminal")).toBe("terminal");
  });

  test("an unknown/future tag falls back to terminal, not to itself", () => {
    expect(normalizeSurfaceKind("Markdown")).toBe("terminal"); // case-sensitive
  });
});

describe("markdownSurfaceUrl", () => {
  test("custom-scheme form matches resolve_md_request's shell arm", () => {
    expect(markdownSurfaceUrl()).toBe("cmux-md://localhost/shell.html");
  });

  test("threads a panel id through the query when provided", () => {
    expect(markdownSurfaceUrl("surface-4")).toBe(
      "cmux-md://localhost/shell.html?panelId=surface-4",
    );
  });

  test("http rewrite form matches the WebView2-delivered origin", () => {
    expect(markdownSurfaceHttpUrl()).toBe("http://cmux-md.localhost/shell.html");
  });

  test("http rewrite form keeps the panel id query too", () => {
    expect(markdownSurfaceHttpUrl("pane/with space")).toBe(
      "http://cmux-md.localhost/shell.html?panelId=pane%2Fwith%20space",
    );
  });
});

describe("diffSurfaceUrl", () => {
  const token = "tok-abcdef0123456789";

  test("custom-scheme form is byte-aligned with parse_diff_viewer_uri", () => {
    expect(diffSurfaceUrl(token)).toBe(`cmux-diff-viewer://${token}/index.html`);
  });

  test("http rewrite form matches the WebView2-delivered origin", () => {
    expect(diffSurfaceHttpUrl(token)).toBe(
      `http://cmux-diff-viewer.localhost/${token}/index.html`,
    );
  });

  test("a stored request path reopens the same diff entry instead of hardcoding index", () => {
    expect(diffSurfaceUrl(token, "/review/file.patch.html")).toBe(
      `cmux-diff-viewer://${token}/review/file.patch.html`,
    );
    expect(diffSurfaceHttpUrl(token, "review/file.patch.html")).toBe(
      `http://cmux-diff-viewer.localhost/${token}/review/file.patch.html`,
    );
  });

  test("both forms reject an empty token (would parse to None on the Rust side)", () => {
    expect(() => diffSurfaceUrl("")).toThrow(EmptyDiffTokenError);
    expect(() => diffSurfaceHttpUrl("")).toThrow(EmptyDiffTokenError);
  });
});
