import { beforeEach, describe, expect, mock, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type { CommandPaletteResolvedSearchMatch } from "./CommandPalette";
import type { CommandRowProps } from "./CommandRow";

const capturedRows: CommandRowProps[] = [];

mock.module("./CommandRow", () => ({
  CommandRow: (props: CommandRowProps) => {
    capturedRows.push(props);
    return <div data-command-id={props.command.id} />;
  },
}));

const { CommandPalette, scrollSelectedPaletteRow } = await import("./CommandPalette");

function command(id: string, title: string) {
  return {
    id,
    rank: 0,
    title,
    subtitle: "",
    shortcut_hint: null,
    kind_label: null,
    keywords: [],
    dismiss_on_run: true,
  };
}

function match(command_id: string): CommandPaletteResolvedSearchMatch {
  return { command_id, score: 0, title_match_indices: [] };
}

describe("CommandPalette row interactions", () => {
  beforeEach(() => {
    capturedRows.length = 0;
  });

  test("passes stable option ids and the active row marker through to rendered rows", () => {
    renderToStaticMarkup(
      <CommandPalette
        matches={[match("workspace.a"), match("workspace.b")]}
        commands={[command("workspace.a", "A"), command("workspace.b", "B")]}
        selectedIndex={1}
        optionIdPrefix="palette-row"
      />,
    );

    expect(capturedRows).toHaveLength(2);
    expect(capturedRows[0]?.rowId).toBe("palette-row-0");
    expect(capturedRows[0]?.active).toBe(false);
    expect(capturedRows[1]?.rowId).toBe("palette-row-1");
    expect(capturedRows[1]?.active).toBe(true);
  });

  test("hover and click callbacks are wired by visible row index", () => {
    const hovered: number[] = [];
    const activated: number[] = [];

    renderToStaticMarkup(
      <CommandPalette
        matches={[match("workspace.a"), match("workspace.b")]}
        commands={[command("workspace.a", "A"), command("workspace.b", "B")]}
        selectedIndex={0}
        onHoverIndex={(index) => hovered.push(index)}
        onActivateIndex={(index) => activated.push(index)}
      />,
    );

    capturedRows[1]?.onMouseEnter?.();
    capturedRows[0]?.onClick?.();

    expect(hovered).toEqual([1]);
    expect(activated).toEqual([0]);
  });

  test("scroll helper follows the selected row with nearest alignment", () => {
    const calls: unknown[] = [];
    const row = {
      scrollIntoView: (options?: ScrollIntoViewOptions) => calls.push(options),
    };

    scrollSelectedPaletteRow([null, row, null], 1);
    scrollSelectedPaletteRow([null, row, null], 0);

    expect(calls).toEqual([{ block: "nearest" }]);
  });
});
