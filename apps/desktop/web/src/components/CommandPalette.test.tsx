import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import {
  paletteSelectionReducer,
  initialSelection,
} from "../palette/paletteSelection";
import { CommandPalette, type CommandPaletteResolvedSearchMatch } from "./CommandPalette";
import type { CommandPaletteCommand } from "./CommandRow";

function command(overrides: Partial<CommandPaletteCommand> & { id: string; title: string }): CommandPaletteCommand {
  return {
    rank: 0,
    subtitle: "",
    shortcut_hint: null,
    kind_label: null,
    keywords: [],
    dismiss_on_run: true,
    ...overrides,
  };
}

const rename = command({
  id: "command.rename",
  title: "Rename Tab",
  subtitle: "Tab",
  shortcut_hint: "⌘R",
});
const split = command({
  id: "command.split",
  title: "Split Pane",
  subtitle: "Layout",
  kind_label: "terminal",
});

const match = (
  command_id: string,
  title_match_indices: number[],
  score = 0,
): CommandPaletteResolvedSearchMatch => ({ command_id, score, title_match_indices });

describe("CommandPalette", () => {
  test("highlights matched title characters as <mark> spans", () => {
    const markup = renderToStaticMarkup(
      <CommandPalette
        matches={[match("command.rename", [0, 1])]}
        commands={[rename]}
        selectedIndex={0}
      />,
    );
    // "Rename" — indices 0,1 → R, e wrapped; unmatched chars are plain spans.
    expect(markup).toContain('<mark class="cmux-palette-title-match">R</mark>');
    expect(markup).toContain('<mark class="cmux-palette-title-match">e</mark>');
    expect(markup).toContain("<span>n</span>");
  });

  test("renders the trailing shortcut hint and subtitle", () => {
    const markup = renderToStaticMarkup(
      <CommandPalette matches={[match("command.rename", [])]} commands={[rename]} selectedIndex={0} />,
    );
    expect(markup).toContain("cmux-palette-row-shortcut");
    expect(markup).toContain("⌘R");
    expect(markup).toContain("cmux-palette-row-subtitle");
    expect(markup).toContain("Tab");
  });

  test("renders the kind label when present", () => {
    const markup = renderToStaticMarkup(
      <CommandPalette matches={[match("command.split", [])]} commands={[split]} selectedIndex={0} />,
    );
    expect(markup).toContain("cmux-palette-row-kind");
    expect(markup).toContain("terminal");
  });

  test("marks exactly the active row, driven by the selection reducer", () => {
    // Reducer moves the cursor down from the top over a 2-row list.
    const selection = paletteSelectionReducer(initialSelection(2), { type: "moveDown" });
    const markup = renderToStaticMarkup(
      <CommandPalette
        matches={[match("command.rename", []), match("command.split", [])]}
        commands={[rename, split]}
        selectedIndex={selection.index}
      />,
    );
    // Row order follows the matches array: rename (row 0), split (row 1).
    // Attribute order in the emitted tag: aria-selected, data-active, data-command-id.
    expect(markup).toMatch(/data-active="false"[^>]*data-command-id="command\.rename"/);
    expect(markup).toMatch(/data-active="true"[^>]*data-command-id="command\.split"/);
    // aria-selected mirrors the active marker for the highlighted row.
    expect(markup).toMatch(/aria-selected="true"[^>]*data-active="true"[^>]*data-command-id="command\.split"/);
  });

  test("preserves match order and drops matches with no backing command", () => {
    const markup = renderToStaticMarkup(
      <CommandPalette
        matches={[match("command.split", []), match("missing", []), match("command.rename", [])]}
        commands={[rename, split]}
        selectedIndex={0}
      />,
    );
    const splitAt = markup.indexOf('data-command-id="command.split"');
    const renameAt = markup.indexOf('data-command-id="command.rename"');
    expect(splitAt).toBeGreaterThanOrEqual(0);
    expect(renameAt).toBeGreaterThan(splitAt); // split rendered before rename
    expect(markup).not.toContain('data-command-id="missing"');
  });

  test("shows the empty state when there are no rows to render", () => {
    const markup = renderToStaticMarkup(
      <CommandPalette matches={[]} commands={[rename]} selectedIndex={0} emptyLabel="Nothing here" />,
    );
    expect(markup).toContain("cmux-palette-empty");
    expect(markup).toContain("Nothing here");
    expect(markup).not.toContain("cmux-palette-row");
  });
});
