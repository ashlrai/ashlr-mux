import { host } from "./host/host";

export type DockSurfaceKind = "terminal" | "browser";

export type DockRuntimeIntent =
  | {
      type: "terminal";
      working_directory: string | null;
      command: string | null;
      environment: Record<string, string>;
      tmux_start_command?: string | null;
    }
  | { type: "browser"; url: string; profile: string | null };

export interface DockSurfaceSnapshot {
  id: string;
  pane_id: string;
  generation?: number;
  kind: DockSurfaceKind;
  title: string;
  runtime: DockRuntimeIntent;
}

export interface DockPaneSnapshot {
  id: string;
  surface_ids: string[];
  selected_surface_id: string | null;
  placement: "root" | "split_right" | "split_down";
  divider_position: number | null;
}

export interface DockSnapshot {
  owner_id: string;
  focused_pane_id: string | null;
  panes: DockPaneSnapshot[];
  surfaces: DockSurfaceSnapshot[];
}

export type DockAction =
  | { type: "replaced"; snapshot: DockSnapshot }
  | { type: "selected"; paneId: string; surfaceId: string; focus: boolean }
  | { type: "focused"; surfaceId: string };

export function emptyDockSnapshot(ownerId: string): DockSnapshot {
  return { owner_id: ownerId, focused_pane_id: null, panes: [], surfaces: [] };
}

export function dockCurrentSurface(snapshot: DockSnapshot): DockSurfaceSnapshot | null {
  const pane = snapshot.panes.find((candidate) => candidate.id === snapshot.focused_pane_id);
  return (
    snapshot.surfaces.find((surface) => surface.id === pane?.selected_surface_id) ?? null
  );
}

export function dockReducer(snapshot: DockSnapshot, action: DockAction): DockSnapshot {
  if (action.type === "replaced") {
    return action.snapshot;
  }
  const surface = snapshot.surfaces.find((candidate) => candidate.id === action.surfaceId);
  if (surface == null) {
    return snapshot;
  }
  if (action.type === "focused") {
    return {
      ...snapshot,
      focused_pane_id: surface.pane_id,
      panes: snapshot.panes.map((pane) =>
        pane.id === surface.pane_id ? { ...pane, selected_surface_id: surface.id } : pane,
      ),
    };
  }
  if (surface.pane_id !== action.paneId) {
    return snapshot;
  }
  return {
    ...snapshot,
    focused_pane_id: action.focus ? action.paneId : snapshot.focused_pane_id,
    panes: snapshot.panes.map((pane) =>
      pane.id === action.paneId ? { ...pane, selected_surface_id: action.surfaceId } : pane,
    ),
  };
}

export function loadDock(ownerId: string): Promise<DockSnapshot> {
  return host.invoke<DockSnapshot>("dock_snapshot", { ownerId });
}

export function createDockSurface(
  ownerId: string,
  request: {
    kind: DockSurfaceKind;
    pane_id?: string;
    placement?: "tab" | "split_right" | "split_down";
    focus?: boolean;
    url?: string;
  },
): Promise<DockSnapshot> {
  return host.invoke<DockSnapshot>("dock_create", { ownerId, request });
}

export function selectDockSurface(
  ownerId: string,
  paneId: string,
  surfaceId: string,
  focus: boolean,
): Promise<DockSnapshot> {
  return host.invoke<DockSnapshot>("dock_select", { ownerId, paneId, surfaceId, focus });
}

export function focusDockSurface(ownerId: string, surfaceId: string): Promise<DockSnapshot> {
  return host.invoke<DockSnapshot>("dock_focus", { ownerId, surfaceId });
}

export function closeDockSurface(ownerId: string, surfaceId: string): Promise<DockSnapshot> {
  return host.invoke<DockSnapshot>("dock_close", { ownerId, surfaceId });
}
