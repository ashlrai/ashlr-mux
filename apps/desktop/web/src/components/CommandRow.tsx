// One rendered command-palette row: a title with matched characters
// highlighted, a subtitle, an optional kind label, and a trailing
// keyboard-shortcut hint. Pure presentation — all cursor/interaction logic
// lives in `../palette/paletteSelection`, so this component fires no
// handlers (SSR-safe).

/**
 * One runnable palette command. Plain-TS mirror of the Rust
 * `CommandPaletteCommand` value fields (`crates/cmux-command-palette`),
 * minus the host-bound activation closure.
 */
export interface CommandPaletteCommand {
  /** Stable command identifier. */
  id: string;
  /** Tie-break rank; lower sorts first at equal score. */
  rank: number;
  /** Display title. */
  title: string;
  /** Display subtitle. */
  subtitle: string;
  /** Optional keyboard-shortcut hint shown trailing the row. */
  shortcut_hint: string | null;
  /** Optional kind label (for example a switcher row's surface kind). */
  kind_label: string | null;
  /** Additional search keywords. */
  keywords: string[];
  /** Whether activating the command dismisses the palette. */
  dismiss_on_run: boolean;
}

export interface CommandRowProps {
  /** The command backing this row. */
  command: CommandPaletteCommand;
  /** Title character indices to highlight (from the resolved match). */
  titleMatchIndices: readonly number[];
  /** Whether this is the highlighted row (driven by the selection reducer). */
  active: boolean;
}

/**
 * Splits a title into character spans, wrapping matched indices in `<mark>`.
 *
 * The Rust match stores a set of character indices; rendering per-character
 * keeps the SSR output a faithful, position-exact reflection of that set.
 */
function renderTitle(title: string, matchIndices: readonly number[]): React.ReactNode {
  const matched = new Set(matchIndices);
  const chars = Array.from(title);
  return chars.map((char, index) =>
    matched.has(index) ? (
      <mark key={index} className="cmux-palette-title-match">
        {char}
      </mark>
    ) : (
      <span key={index}>{char}</span>
    ),
  );
}

/** Renders a single command-palette row. */
export function CommandRow({ command, titleMatchIndices, active }: CommandRowProps): React.JSX.Element {
  return (
    <div
      className="cmux-palette-row"
      role="option"
      aria-selected={active}
      data-active={active ? "true" : "false"}
      data-command-id={command.id}
    >
      <div className="cmux-palette-row-main">
        <span className="cmux-palette-row-title">{renderTitle(command.title, titleMatchIndices)}</span>
        {command.subtitle ? (
          <span className="cmux-palette-row-subtitle">{command.subtitle}</span>
        ) : null}
      </div>
      {command.kind_label ? (
        <span className="cmux-palette-row-kind">{command.kind_label}</span>
      ) : null}
      {command.shortcut_hint ? (
        <span className="cmux-palette-row-shortcut">{command.shortcut_hint}</span>
      ) : null}
    </div>
  );
}
