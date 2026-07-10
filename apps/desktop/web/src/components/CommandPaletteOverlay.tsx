import { useEffect, useRef } from "react";

import {
  useCommandPalette,
  type CommandPaletteHostActions,
} from "../palette/useCommandPalette";
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
export function CommandPaletteOverlay({
  hostActions,
}: {
  /** App-owned actions (e.g. sidebar toggle) some commands execute. */
  hostActions?: CommandPaletteHostActions;
} = {}): React.JSX.Element | null {
  const palette = useCommandPalette(hostActions);
  const {
    visible,
    query,
    scope,
    commands,
    matches,
    selectedIndex,
    editor,
  } = palette;
  const inputRef = useRef<HTMLInputElement>(null);
  const editorInputRef = useRef<HTMLInputElement>(null);
  const editorTextareaRef = useRef<HTMLTextAreaElement>(null);
  const activeDescendantId =
    matches.length > 0 ? `cmux-palette-option-${selectedIndex}` : undefined;

  useEffect(() => {
    if (!visible) {
      return;
    }
    if (editor?.kind === "renameWorkspace") {
      editorInputRef.current?.focus();
      editorInputRef.current?.select();
      return;
    }
    if (editor?.kind === "workspaceDescription") {
      editorTextareaRef.current?.focus();
      return;
    }
    if (visible) {
      inputRef.current?.focus();
    }
  }, [editor, visible]);

  if (!visible) {
    return null;
  }

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
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

  const onEditorInputKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    switch (event.key) {
      case "Enter":
        event.preventDefault();
        palette.submitEditor();
        break;
      case "Escape":
        event.preventDefault();
        palette.cancelEditor();
        break;
      default:
        break;
    }
  };

  const onEditorTextareaKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      palette.cancelEditor();
      return;
    }
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      palette.submitEditor();
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
        {editor == null ? (
          <>
            <input
              ref={inputRef}
              className="cmux-palette-input"
              type="text"
              spellCheck={false}
              autoComplete="off"
              aria-controls="cmux-palette-results"
              aria-activedescendant={activeDescendantId}
              placeholder={placeholder}
              value={query}
              onChange={(event) => palette.setQuery(event.target.value)}
              onKeyDown={onKeyDown}
            />
            <CommandPalette
              listId="cmux-palette-results"
              optionIdPrefix="cmux-palette-option"
              matches={matches}
              commands={commands}
              selectedIndex={selectedIndex}
              emptyLabel={emptyLabel}
              onHoverIndex={palette.hoverAt}
              onActivateIndex={palette.activateAt}
            />
          </>
        ) : (
          <div className="cmux-palette-editor">
            <div className="cmux-palette-editor-header">
              <h2 className="cmux-palette-editor-title">
                {editor.kind === "renameWorkspace"
                  ? "Rename Workspace"
                  : "Edit Workspace Description"}
              </h2>
              <p className="cmux-palette-editor-subtitle">{editor.title}</p>
            </div>
            {editor.kind === "renameWorkspace" ? (
              <input
                ref={editorInputRef}
                className="cmux-palette-input"
                type="text"
                spellCheck={false}
                autoComplete="off"
                aria-label={`Rename ${editor.title}`}
                value={editor.draft}
                onChange={(event) => palette.setEditorDraft(event.target.value)}
                onKeyDown={onEditorInputKeyDown}
              />
            ) : (
              <>
                <textarea
                  ref={editorTextareaRef}
                  className="cmux-palette-textarea"
                  spellCheck={false}
                  aria-label={`Workspace description for ${editor.title}`}
                  value={editor.draft}
                  onChange={(event) => palette.setEditorDraft(event.target.value)}
                  onKeyDown={onEditorTextareaKeyDown}
                />
                <p className="cmux-palette-editor-hint">
                  Press Ctrl+Enter to save, or Escape to cancel.
                </p>
              </>
            )}
            <div className="cmux-palette-editor-actions">
              <button
                type="button"
                className="cmux-header-button"
                onClick={palette.cancelEditor}
              >
                Cancel
              </button>
              <button
                type="button"
                className="cmux-header-button"
                onClick={palette.submitEditor}
              >
                Save
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
