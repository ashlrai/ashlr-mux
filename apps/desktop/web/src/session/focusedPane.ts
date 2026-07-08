import { useSyncExternalStore } from "react";

import { firstActivePanelId, isPane, type Layout } from "./splitLayout";

/**
 * C4 — focused-pane tracking, the web-side source of truth for "which panel
 * owns keyboard focus".
 *
 * Canonical macOS derives this from the bonsplit tree's focused pane surfaced
 * as a panel id (`WorkspaceSurfaceListModel.focusedPanelId`,
 * Packages/macOS/CmuxWorkspaces/.../WorkspaceSurfaceListModel.swift:61-67),
 * fed by pane tap gestures and the AppKit first-responder sync
 * (WorkspaceContentView.swift:215-217 / 231-238). The port's generated session
 * model has no focused field and adding one is a crates + core-types change,
 * so for this slice the focus SoT lives web-side: Workspace writes this store
 * from capture-phase pointer/focus events, the command palette reads it.
 * Snapshot-persisted focus (Workspace.swift:132 writes, 242-246 restores) is a
 * noted follow-up once the Rust model grows the field.
 */

export interface FocusedPaneStore {
  focus(panelId: string): void;
  clear(): void;
  get(): string | undefined;
  subscribe(listener: () => void): () => void;
}

/**
 * A last-focused-panel cell. Listeners are notified only on CHANGE — pane
 * wrappers re-fire pointer/focus events constantly (every click, every xterm
 * refocus), and a same-id write must never re-render the palette host.
 */
export function createFocusedPaneStore(): FocusedPaneStore {
  let current: string | undefined;
  const listeners = new Set<() => void>();
  const notify = (): void => {
    for (const listener of listeners) {
      listener();
    }
  };
  return {
    focus(panelId: string): void {
      if (panelId !== current) {
        current = panelId;
        notify();
      }
    },
    clear(): void {
      if (current !== undefined) {
        current = undefined;
        notify();
      }
    },
    get: () => current,
    subscribe(listener: () => void): () => void {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** The app singleton: `Workspace` writes it, `useCommandPalette` reads it. */
export const focusedPaneStore = createFocusedPaneStore();

/** Every panel id hosted by any leaf pane of `layout` (all `panel_ids`). */
export function panelIdsInLayout(layout: Layout): ReadonlySet<string> {
  const ids = new Set<string>();
  const walk = (node: Layout): void => {
    if (isPane(node)) {
      for (const id of node.pane.panel_ids) {
        ids.add(id);
      }
      return;
    }
    walk(node.split.first);
    walk(node.split.second);
  };
  walk(layout);
  return ids;
}

/**
 * The focused panel id revalidated against the CURRENT layout, with a
 * first-leaf fallback — mirroring the canonical session-restore
 * validate-then-fallback (Workspace.swift:242-246) and the
 * `performSplitShortcut` workspace-default fallback (AppDelegate.swift:14545).
 *
 * The store deliberately keeps the RAW last-focused id even when it is absent
 * from the current workspace's layout: resolution revalidates per read, so
 * switching workspaces falls back to that workspace's first leaf and switching
 * back restores the remembered focus (cheap per-workspace focus memory; panel
 * ids are globally unique, so a stale id can never collide with another
 * workspace's pane).
 */
export function resolveActivePanelId(
  focused: string | undefined,
  layout: Layout | null | undefined,
): string | undefined {
  if (!layout) {
    return undefined;
  }
  if (focused !== undefined && panelIdsInLayout(layout).has(focused)) {
    return focused;
  }
  return firstActivePanelId(layout);
}

/** The resolved focused panel id, subscribed to the singleton store. */
export function useFocusedPanelId(layout: Layout | null | undefined): string | undefined {
  // The third (server-snapshot) argument keeps SSR test renders working — the
  // palette host renders under `renderToStaticMarkup` in tests.
  const focused = useSyncExternalStore(
    focusedPaneStore.subscribe,
    focusedPaneStore.get,
    focusedPaneStore.get,
  );
  return resolveActivePanelId(focused, layout);
}
