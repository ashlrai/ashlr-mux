import { useCallback, useEffect, useState } from "react";

import type {
  AppSessionSnapshot,
  SessionSplitOrientation,
  SessionWorkspaceLayoutSnapshot,
} from "@cmux/core-types";

import { host } from "../host/host";
import { activeLayoutOf } from "../session/activeLayout";
import type { SplitPath } from "../session/splitLayout";

/**
 * React binding for the Rust session layer (`src-tauri/src/session.rs`).
 *
 * On mount it pulls the authoritative snapshot (`session_snapshot`) and
 * subscribes to `cmux://session-changed`, so every structural mutation — from
 * this window or any other trigger — flows back through one channel. The mutator
 * commands (`session_split` / `session_close` / `session_set_divider`) also
 * return the fresh snapshot; we apply that immediately AND rely on the event, so
 * the two paths reconcile to the same value.
 *
 * Command argument keys are snake_case to match the `#[tauri::command]`
 * signatures (`panel_id`, `position`).
 */
export interface UseSession {
  /** The whole app snapshot, or `null` until the first fetch resolves. */
  snapshot: AppSessionSnapshot | null;
  /** The layout tree of the selected workspace of the first window. */
  activeLayout: SessionWorkspaceLayoutSnapshot | null;
  /** Split the pane holding `panelId` in `orientation`. */
  split: (panelId: string, orientation: SessionSplitOrientation) => void;
  /** Close the pane/panel `panelId`. */
  close: (panelId: string) => void;
  /** Persist the divider ratio of the split reached by `path`. */
  setDivider: (path: SplitPath, position: number) => void;
  /**
   * Set the surface kind of the pane holding `panelId`: `"agent"` for a
   * canonical agent session, or `null` to revert it to a terminal.
   */
  setSurfaceKind: (panelId: string, kind: string | null) => void;
}

export function useSession(): UseSession {
  const [snapshot, setSnapshot] = useState<AppSessionSnapshot | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void host
      .invoke<AppSessionSnapshot>("session_snapshot")
      .then((next) => {
        if (!disposed) {
          setSnapshot(next);
        }
      })
      .catch(() => {});

    void host
      .on<AppSessionSnapshot>("cmux://session-changed", (next) => {
        if (!disposed) {
          setSnapshot(next);
        }
      })
      .then((off) => {
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // NOTE: Tauri v2 maps JS camelCase argument keys onto Rust snake_case command
  // params, so the pane id must be sent as `panelId` (→ `panel_id`), NOT
  // `panel_id`. `path`/`position`/`orientation` are single words, unaffected.
  const split = useCallback((panelId: string, orientation: SessionSplitOrientation) => {
    void host
      .invoke<AppSessionSnapshot>("session_split", { panelId, orientation })
      .then(setSnapshot)
      .catch((error) => console.error("session_split failed", error));
  }, []);

  const close = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_close", { panelId })
      .then(setSnapshot)
      .catch((error) => console.error("session_close failed", error));
  }, []);

  const setDivider = useCallback((path: SplitPath, position: number) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_divider", { path, position })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_divider failed", error));
  }, []);

  const setSurfaceKind = useCallback((panelId: string, kind: string | null) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_surface_kind", { panelId, kind })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_surface_kind failed", error));
  }, []);

  return {
    snapshot,
    activeLayout: activeLayoutOf(snapshot),
    split,
    close,
    setDivider,
    setSurfaceKind,
  };
}
