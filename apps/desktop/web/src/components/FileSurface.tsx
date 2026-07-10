import { useEffect, useState } from "react";

import {
  readFileExplorerFile,
  writeFileExplorerFile,
} from "../host/fileExplorer";

export interface FileSurfaceProps {
  filePath?: string;
  wordWrap?: boolean;
}

export interface FileSaveShortcutEvent {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
}

export interface FileSaveShortcutState {
  dirty: boolean;
  loading: boolean;
  saving: boolean;
  hasFile: boolean;
}

export function fileSurfaceTitle(filePath?: string): string {
  if (filePath == null || filePath.trim() === "") {
    return "No file selected";
  }
  return filePath.split(/[\\/]/).pop() || filePath;
}

export function shouldHandleFileSaveShortcut(
  event: FileSaveShortcutEvent,
  state: FileSaveShortcutState,
): boolean {
  return (
    event.key.toLowerCase() === "s" &&
    (event.ctrlKey === true || event.metaKey === true) &&
    event.altKey !== true &&
    event.shiftKey !== true &&
    state.dirty &&
    !state.loading &&
    !state.saving &&
    state.hasFile
  );
}

export function shouldProceedWithFileReload(
  dirty: boolean,
  confirmDiscard: (message: string) => boolean,
): boolean {
  return (
    !dirty ||
    confirmDiscard("Discard unsaved changes and reload this file from disk?")
  );
}

/**
 * Built-in UTF-8 text editor surface. The backend owns file I/O and guardrails;
 * the UI owns draft/dirty state so users can inspect and make small edits
 * without leaving cmux.
 */
export function FileSurface({
  filePath,
  wordWrap = false,
}: FileSurfaceProps): React.JSX.Element {
  const [content, setContent] = useState("");
  const [savedContent, setSavedContent] = useState("");
  const [status, setStatus] = useState<string>("Ready.");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const dirty = content !== savedContent;
  const title = fileSurfaceTitle(filePath);

  const loadFile = (confirmDirty = false): void => {
    if (
      confirmDirty &&
      !shouldProceedWithFileReload(dirty, (message) =>
        typeof window === "undefined" ? true : window.confirm(message),
      )
    ) {
      return;
    }
    if (filePath == null || filePath.trim() === "") {
      setContent("");
      setSavedContent("");
      setStatus("No file selected.");
      return;
    }

    setLoading(true);
    setStatus("Loading file...");
    void readFileExplorerFile({ path: filePath })
      .then((reply) => {
        setContent(reply.content);
        setSavedContent(reply.content);
        setStatus(`Loaded ${reply.size} bytes.`);
      })
      .catch((error) => {
        setContent("");
        setSavedContent("");
        setStatus(error instanceof Error ? error.message : String(error));
      })
      .finally(() => setLoading(false));
  };

  useEffect(() => loadFile(false), [filePath]);

  const saveFile = (): void => {
    if (filePath == null || filePath.trim() === "") {
      setStatus("No file selected.");
      return;
    }
    setSaving(true);
    setStatus("Saving file...");
    void writeFileExplorerFile({ path: filePath, content })
      .then((reply) => {
        setSavedContent(content);
        setStatus(reply.message);
      })
      .catch((error) => {
        setStatus(error instanceof Error ? error.message : String(error));
      })
      .finally(() => setSaving(false));
  };

  return (
    <div className="cmux-file-surface">
      <header className="cmux-file-surface-toolbar">
        <div className="cmux-file-surface-heading">
          <span className="cmux-file-surface-title">{title}</span>
          {filePath ? (
            <span className="cmux-file-surface-path">{filePath}</span>
          ) : null}
        </div>
        <div className="cmux-file-surface-actions">
          <span className="cmux-file-surface-dirty">
            {dirty ? "Unsaved changes" : "Saved"}
          </span>
          <button type="button" onClick={() => loadFile(true)} disabled={loading || saving}>
            Reload
          </button>
          <button
            type="button"
            onClick={saveFile}
            disabled={!dirty || loading || saving || !filePath}
          >
            {saving ? "Saving..." : "Save"}
          </button>
        </div>
      </header>
      <textarea
        className={
          wordWrap
            ? "cmux-file-surface-editor is-wrapped"
            : "cmux-file-surface-editor"
        }
        value={content}
        spellCheck={false}
        aria-label="File editor"
        readOnly={loading}
        onChange={(event) => setContent(event.target.value)}
        onKeyDown={(event) => {
          if (
            shouldHandleFileSaveShortcut(event, {
              dirty,
              loading,
              saving,
              hasFile: filePath != null && filePath.trim() !== "",
            })
          ) {
            event.preventDefault();
            saveFile();
          }
        }}
      />
      <footer className="cmux-file-surface-status" aria-live="polite">
        {status}
      </footer>
    </div>
  );
}
