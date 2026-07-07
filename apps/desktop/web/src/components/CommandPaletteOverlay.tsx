import { useEffect, useRef } from "react";

import { useCommandPalette } from "../palette/useCommandPalette";
import { CommandPalette } from "./CommandPalette";

/**
 * The D4 command-palette overlay shell: a centered modal over a scrim, with the
 * search input and keyboard handling. The result list itself is the pure
 * {@link CommandPalette} renderer; this owns interaction and visibility (from
 * {@link useCommandPalette}).
 *
 * Open with Ctrl/Cmd+K (workspace switcher) or Ctrl/Cmd+Shift+P (commands).
 * Arrow keys move the cursor, Enter activates, Escape or a backdrop click
 * dismisses.
 */
export function CommandPaletteOverlay(): React.JSX.Element | null {
  const palette = useCommandPalette();
  const { visible, query, scope, commands, matches, selectedIndex } = palette;
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (visible) {
      inputRef.current?.focus();
    }
  }, [visible]);

  if (!visible) {
    return null;
  }

  const onKeyDown = (event: React.KeyboardEvent) => {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        palette.move(1);
        break;
      case "ArrowUp":
        event.preventDefault();
        palette.move(-1);
        break;
      case "Enter":
        event.preventDefault();
        palette.activateAt(selectedIndex);
        break;
      case "Escape":
        event.preventDefault();
        palette.close();
        break;
      default:
        break;
    }
  };

  const placeholder =
    scope === "commands" ? "Run a command…" : "Jump to a workspace… (type > for commands)";
  const emptyLabel =
    scope === "commands" ? "No matching commands" : "No matching workspaces";

  return (
    <div
      className="cmux-palette-scrim"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          palette.close();
        }
      }}
    >
      <div
        className="cmux-palette-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <input
          ref={inputRef}
          className="cmux-palette-input"
          type="text"
          spellCheck={false}
          autoComplete="off"
          placeholder={placeholder}
          value={query}
          onChange={(event) => palette.setQuery(event.target.value)}
          onKeyDown={onKeyDown}
        />
        <CommandPalette
          matches={matches}
          commands={commands}
          selectedIndex={selectedIndex}
          emptyLabel={emptyLabel}
        />
      </div>
    </div>
  );
}
