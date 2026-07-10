import { useEffect, useState } from "react";

import { host } from "../host/host";
import { isMarkdownFilePath } from "../host/fileExplorer";
import { useSession } from "../hooks/useSession";
import { useFocusedPanelId } from "../session/focusedPane";

export interface DirectorySearchResult {
  path: string;
  lineNumber: number;
  lineText: string;
}

export function relativeSearchPath(path: string, directory: string): string {
  const normalizedPath = path.replaceAll("\\", "/");
  const normalizedDirectory = directory.replaceAll("\\", "/").replace(/\/+$/, "");
  if (
    normalizedDirectory !== "" &&
    normalizedPath.toLowerCase().startsWith(`${normalizedDirectory.toLowerCase()}/`)
  ) {
    return normalizedPath.slice(normalizedDirectory.length + 1);
  }
  return path;
}

export interface DirectorySearchActivationDeps {
  directory: string;
  focusedPanelId: string | undefined;
  openMarkdownFile: (panelId: string, filePath: string) => void;
  openFile: (panelId: string, filePath: string) => void;
  setStatus: (status: string) => void;
}

export function activateDirectorySearchResult(
  result: DirectorySearchResult,
  deps: DirectorySearchActivationDeps,
): void {
  const label = relativeSearchPath(result.path, deps.directory);
  if (isMarkdownFilePath(result.path)) {
    if (deps.focusedPanelId == null) {
      deps.setStatus("Select a pane before opening a Markdown preview.");
      return;
    }
    deps.openMarkdownFile(deps.focusedPanelId, result.path);
    deps.setStatus(`Opened ${label} in the focused pane.`);
    return;
  }

  if (deps.focusedPanelId == null) {
    deps.setStatus("Select a pane before opening a file editor.");
    return;
  }
  deps.openFile(deps.focusedPanelId, result.path);
  deps.setStatus(`Opened ${label} in the focused pane.`);
}

export interface DirectorySearchOverlayProps {
  open: boolean;
  onClose: () => void;
}

export function DirectorySearchOverlay({
  open,
  onClose,
}: DirectorySearchOverlayProps): React.JSX.Element | null {
  const { activeLayout, workspaces, selectedWorkspaceIndex, openMarkdownFile, openFile } =
    useSession();
  const focusedPanelId = useFocusedPanelId(activeLayout);
  const selectedWorkspace = workspaces[selectedWorkspaceIndex];
  const initialDirectory = selectedWorkspace?.current_directory ?? "";
  const [directory, setDirectory] = useState(initialDirectory);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<DirectorySearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activationStatus, setActivationStatus] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    setDirectory(initialDirectory);
    setQuery("");
    setResults([]);
    setError(null);
    setActivationStatus(null);
  }, [initialDirectory, open]);

  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  const runSearch = () => {
    const trimmedDirectory = directory.trim();
    const trimmedQuery = query.trim();
    if (trimmedDirectory === "" || trimmedQuery === "") {
      setResults([]);
      setError(trimmedDirectory === "" ? "Choose a directory to search." : null);
      return;
    }
    setLoading(true);
    setError(null);
    setActivationStatus(null);
    void host
      .invoke<DirectorySearchResult[]>("find_in_directory", {
        request: {
          directory: trimmedDirectory,
          query: trimmedQuery,
          limit: 100,
        },
      })
      .then(setResults)
      .catch((searchError) => {
        setResults([]);
        setError(searchError instanceof Error ? searchError.message : String(searchError));
      })
      .finally(() => setLoading(false));
  };

  const openResult = (result: DirectorySearchResult) => {
    activateDirectorySearchResult(result, {
      directory,
      focusedPanelId,
      openMarkdownFile,
      openFile,
      setStatus: setActivationStatus,
    });
  };

  if (!open) {
    return null;
  }

  return (
    <div
      className="cmux-directory-search-overlay"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <section
        className="cmux-directory-search-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Find in Directory"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="cmux-directory-search-header">
          <div>
            <h2 className="cmux-directory-search-title">Find in Directory</h2>
            <p className="cmux-directory-search-subtitle">
              Search text files from the selected workspace directory.
            </p>
          </div>
          <button
            type="button"
            className="cmux-directory-search-close"
            onClick={onClose}
          >
            Close
          </button>
        </header>
        <form
          className="cmux-directory-search-form"
          onSubmit={(event) => {
            event.preventDefault();
            runSearch();
          }}
        >
          <label className="cmux-directory-search-field">
            <span>Directory</span>
            <input
              value={directory}
              placeholder="C:\\path\\to\\project"
              onChange={(event) => setDirectory(event.target.value)}
            />
          </label>
          <label className="cmux-directory-search-field">
            <span>Query</span>
            <input
              autoFocus
              value={query}
              placeholder="Search text"
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          <button
            type="submit"
            className="cmux-directory-search-run"
            disabled={loading}
          >
            {loading ? "Searching..." : "Search"}
          </button>
        </form>
        {error !== null ? (
          <div className="cmux-directory-search-error">{error}</div>
        ) : null}
        {activationStatus !== null ? (
          <div className="cmux-directory-search-status">{activationStatus}</div>
        ) : null}
        <div className="cmux-directory-search-results" aria-live="polite">
          {results.length === 0 && !loading ? (
            <div className="cmux-directory-search-empty">
              {query.trim() === "" ? "Enter a query to search." : "No matches found."}
            </div>
          ) : (
            results.map((result) => (
              <article
                key={`${result.path}:${result.lineNumber}:${result.lineText}`}
                className="cmux-directory-search-result"
              >
                <button
                  type="button"
                  className="cmux-directory-search-result-button"
                  onClick={() => openResult(result)}
                >
                  <span className="cmux-directory-search-result-path">
                    {relativeSearchPath(result.path, directory)}:{result.lineNumber}
                  </span>
                  <span className="cmux-directory-search-result-line">
                    {result.lineText}
                  </span>
                </button>
              </article>
            ))
          )}
        </div>
      </section>
    </div>
  );
}
