import { useEffect, useReducer, useState } from "react";

import {
  closeDockSurface,
  createDockSurface,
  dockReducer,
  emptyDockSnapshot,
  focusDockSurface,
  loadDock,
  selectDockSurface,
  type DockSnapshot,
  type DockSurfaceSnapshot,
} from "../dock";
import { host } from "../host/host";

export function DockPanel({
  ownerId,
  snapshot: suppliedSnapshot,
  renderSurface,
}: {
  ownerId?: string;
  snapshot?: DockSnapshot;
  renderSurface?: (surface: DockSurfaceSnapshot, active: boolean) => React.ReactNode;
}): React.JSX.Element {
  const effectiveOwner = ownerId ?? suppliedSnapshot?.owner_id ?? "";
  const [snapshot, dispatch] = useReducer(
    dockReducer,
    suppliedSnapshot ?? emptyDockSnapshot(effectiveOwner),
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (suppliedSnapshot != null) {
      dispatch({ type: "replaced", snapshot: suppliedSnapshot });
      return;
    }
    if (effectiveOwner === "") {
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void loadDock(effectiveOwner)
      .then((next) => !disposed && dispatch({ type: "replaced", snapshot: next }))
      .catch((reason) => !disposed && setError(String(reason)));
    void host.on<DockSnapshot>("cmux://dock-changed", (next) => {
      if (!disposed && next.owner_id === effectiveOwner) {
        dispatch({ type: "replaced", snapshot: next });
      }
    }).then((off) => {
      unlisten = off;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [effectiveOwner, suppliedSnapshot]);

  const apply = (operation: Promise<DockSnapshot>) => {
    setError(null);
    void operation
      .then((next) => dispatch({ type: "replaced", snapshot: next }))
      .catch((reason) => setError(String(reason)));
  };

  const create = (kind: "terminal" | "browser", paneId?: string) => {
    if (effectiveOwner === "") return;
    apply(
      createDockSurface(effectiveOwner, {
        kind,
        pane_id: paneId,
        placement: "tab",
        focus: true,
        ...(kind === "browser" ? { url: "about:blank" } : {}),
      }),
    );
  };

  const renderDockSurface = (surface: DockSurfaceSnapshot, active: boolean) => (
    <section
      key={surface.id}
      className={active ? "cmux-dock-surface is-active" : "cmux-dock-surface"}
      data-dock-kind={surface.kind}
      hidden={!active}
    >
      {renderSurface?.(surface, active) ?? <span>{surface.title}</span>}
    </section>
  );

  return (
    <div className="cmux-dock" aria-label="Dock">
      <div className="cmux-dock-toolbar">
        <button type="button" aria-label="New Dock terminal" onClick={() => create("terminal")}>
          + Terminal
        </button>
        <button type="button" aria-label="New Dock browser" onClick={() => create("browser")}>
          + Browser
        </button>
      </div>
      {error != null && <p role="alert">{error}</p>}
      {snapshot.panes.length === 0 ? (
        <div className="cmux-dock-empty">Add a terminal or browser to this Dock.</div>
      ) : (
        <div className="cmux-dock-panes">
          {snapshot.panes.map((pane) => {
            const surfaces = pane.surface_ids
              .map((id) => snapshot.surfaces.find((surface) => surface.id === id))
              .filter((surface): surface is DockSurfaceSnapshot => surface != null);
            return (
              <section
                key={pane.id}
                className={pane.id === snapshot.focused_pane_id ? "cmux-dock-pane is-focused" : "cmux-dock-pane"}
                data-dock-placement={pane.placement}
                style={pane.divider_position == null ? undefined : { flexGrow: pane.divider_position }}
              >
                <div className="cmux-dock-tabs" role="tablist">
                  {surfaces.map((surface) => (
                    <div key={surface.id} className="cmux-dock-tab-wrap">
                      <button
                        type="button"
                        role="tab"
                        aria-selected={surface.id === pane.selected_surface_id}
                        onClick={() => {
                          if (effectiveOwner === "") return;
                          dispatch({ type: "selected", paneId: pane.id, surfaceId: surface.id, focus: true });
                          apply(selectDockSurface(effectiveOwner, pane.id, surface.id, true));
                        }}
                      >
                        {surface.title}
                      </button>
                      <button
                        type="button"
                        aria-label={`Close ${surface.title}`}
                        onClick={() => effectiveOwner !== "" && apply(closeDockSurface(effectiveOwner, surface.id))}
                      >
                        ×
                      </button>
                    </div>
                  ))}
                  <button type="button" aria-label="New Dock terminal" onClick={() => create("terminal", pane.id)}>+</button>
                </div>
                <div
                  className="cmux-dock-pane-content"
                  onFocus={() => {
                    const selected = pane.selected_surface_id;
                    if (effectiveOwner !== "" && selected != null) {
                      dispatch({ type: "focused", surfaceId: selected });
                      apply(focusDockSurface(effectiveOwner, selected));
                    }
                  }}
                >
                  {surfaces.map((surface) => renderDockSurface(surface, surface.id === pane.selected_surface_id))}
                </div>
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}
