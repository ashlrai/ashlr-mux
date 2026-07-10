// The command-palette result list: joins resolved search matches to their
// backing commands (by `command_id`, preserving match order) and renders one
// `CommandRow` each, or an empty state. The highlighted row is chosen by the
// pure `paletteSelection` reducer — this shell holds no interaction logic
// (SSR fires no handlers).

import { useEffect, useRef } from "react";

import { CommandRow, type CommandPaletteCommand } from "./CommandRow";

/**
 * One resolved match produced by the (Rust) search orchestrator. Plain-TS
 * mirror of `CommandPaletteResolvedSearchMatch`; `title_match_indices` is a
 * JSON-friendly array in place of the Rust `HashSet<usize>`.
 */
export interface CommandPaletteResolvedSearchMatch {
  /** The matched command's identifier. */
  command_id: string;
  /** Final merged score. */
  score: number;
  /** Title character indices to highlight. */
  title_match_indices: number[];
}

export interface CommandPaletteProps {
  /** Resolved matches, already ordered by the orchestrator. */
  matches: readonly CommandPaletteResolvedSearchMatch[];
  /** Commands available to join against, looked up by `id`. */
  commands: readonly CommandPaletteCommand[];
  /** Index of the highlighted row (from the `paletteSelection` reducer). */
  selectedIndex: number;
  /** Text shown when there are no matches. */
  emptyLabel?: string;
  /** Stable id for the listbox. */
  listId?: string;
  /** Stable id prefix for each rendered option. */
  optionIdPrefix?: string;
  /** Hovering a row should update the highlighted selection. */
  onHoverIndex?: (index: number) => void;
  /** Clicking a row should activate it. */
  onActivateIndex?: (index: number) => void;
}

export interface ScrollablePaletteRow {
  scrollIntoView(options?: ScrollIntoViewOptions): void;
}

export function scrollSelectedPaletteRow(
  rows: readonly (ScrollablePaletteRow | null)[],
  selectedIndex: number,
): void {
  rows[selectedIndex]?.scrollIntoView({
    block: "nearest",
  });
}

/** Renders the command-palette result list (or its empty state). */
export function CommandPalette({
  matches,
  commands,
  selectedIndex,
  emptyLabel = "No matching commands",
  listId = "cmux-palette-results",
  optionIdPrefix = "cmux-palette-option",
  onHoverIndex,
  onActivateIndex,
}: CommandPaletteProps): React.JSX.Element {
  const byId = new Map(commands.map((command) => [command.id, command]));
  const rowRefs = useRef<Array<HTMLDivElement | null>>([]);

  // Join matches to commands, dropping any match whose command is absent.
  const rows = matches
    .map((match) => {
      const command = byId.get(match.command_id);
      return command ? { command, match } : null;
    })
    .filter((row): row is { command: CommandPaletteCommand; match: CommandPaletteResolvedSearchMatch } => row !== null);

  useEffect(() => {
    scrollSelectedPaletteRow(rowRefs.current, selectedIndex);
  }, [selectedIndex, rows.length]);

  if (rows.length === 0) {
    return (
      <div
        id={listId}
        className="cmux-palette"
        role="listbox"
        aria-label="Command palette results"
      >
        <div className="cmux-palette-empty">{emptyLabel}</div>
      </div>
    );
  }

  return (
    <div
      id={listId}
      className="cmux-palette"
      role="listbox"
      aria-label="Command palette results"
    >
      {rows.map(({ command, match }, index) => (
        <CommandRow
          key={command.id}
          command={command}
          titleMatchIndices={match.title_match_indices}
          active={index === selectedIndex}
          rowId={`${optionIdPrefix}-${index}`}
          rowRef={(node) => {
            rowRefs.current[index] = node;
          }}
          onMouseEnter={() => onHoverIndex?.(index)}
          onClick={() => onActivateIndex?.(index)}
        />
      ))}
    </div>
  );
}
