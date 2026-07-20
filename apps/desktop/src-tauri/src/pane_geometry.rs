use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use tauri::{State, WebviewWindow};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WorkspacePaneGeometry {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PaneGeometryAuthority {
    Uninitialized,
    WorkspaceUnrendered,
    Rendered(WorkspacePaneGeometry),
}

#[derive(Default)]
struct PaneGeometryRegistry {
    by_window: HashMap<String, HashMap<String, WorkspacePaneGeometry>>,
    latest_by_window: HashMap<String, WorkspacePaneGeometry>,
    grid_by_panel: HashMap<String, (u64, u64, u64, u64)>,
}

#[derive(Default)]
pub struct PaneGeometryState {
    registry: Mutex<PaneGeometryRegistry>,
}

impl PaneGeometryState {
    fn report(
        &self,
        window_label: &str,
        workspace_id: &str,
        geometry: WorkspacePaneGeometry,
    ) -> Result<(), String> {
        if window_label.trim().is_empty() {
            return Err("window label must not be empty".to_string());
        }
        if workspace_id.trim().is_empty() {
            return Err("workspaceId must not be empty".to_string());
        }
        if ![geometry.x, geometry.y, geometry.width, geometry.height]
            .into_iter()
            .all(f64::is_finite)
            || geometry.width <= 0.0
            || geometry.height <= 0.0
        {
            return Err(
                "pane geometry must contain finite coordinates and positive dimensions".to_string(),
            );
        }
        let mut registry = self.registry.lock().expect("pane geometry mutex poisoned");
        registry
            .by_window
            .entry(window_label.to_string())
            .or_default()
            .insert(workspace_id.to_string(), geometry);
        registry
            .latest_by_window
            .insert(window_label.to_string(), geometry);
        Ok(())
    }

    pub(crate) fn authority_for(
        &self,
        window_label: &str,
        workspace_id: &str,
    ) -> PaneGeometryAuthority {
        let registry = self.registry.lock().expect("pane geometry mutex poisoned");
        let Some(by_workspace) = registry.by_window.get(window_label) else {
            return PaneGeometryAuthority::Uninitialized;
        };
        by_workspace.get(workspace_id).copied().map_or(
            PaneGeometryAuthority::WorkspaceUnrendered,
            PaneGeometryAuthority::Rendered,
        )
    }

    pub(crate) fn latest_for_window(&self, window_label: &str) -> Option<WorkspacePaneGeometry> {
        self.registry
            .lock()
            .expect("pane geometry mutex poisoned")
            .latest_by_window
            .get(window_label)
            .copied()
    }

    pub(crate) fn remember_grid_fields(&self, panel_id: &str, fields: (u64, u64, u64, u64)) {
        self.registry
            .lock()
            .expect("pane geometry mutex poisoned")
            .grid_by_panel
            .insert(panel_id.to_string(), fields);
    }

    pub(crate) fn grid_fields_for_panel(&self, panel_id: &str) -> Option<(u64, u64, u64, u64)> {
        self.registry
            .lock()
            .expect("pane geometry mutex poisoned")
            .grid_by_panel
            .get(panel_id)
            .copied()
    }

    pub(crate) fn retain_grid_fields(&self, active_panel_ids: &HashSet<String>) {
        self.registry
            .lock()
            .expect("pane geometry mutex poisoned")
            .grid_by_panel
            .retain(|panel_id, _| active_panel_ids.contains(panel_id));
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn pane_report_geometry(
    window: WebviewWindow,
    state: State<'_, PaneGeometryState>,
    workspace_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    state.report(
        window.label(),
        &workspace_id,
        WorkspacePaneGeometry {
            x,
            y,
            width,
            height,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{PaneGeometryAuthority, PaneGeometryState, WorkspacePaneGeometry};

    fn geometry(width: f64) -> WorkspacePaneGeometry {
        WorkspacePaneGeometry {
            x: 240.0,
            y: 28.0,
            width,
            height: 672.0,
        }
    }

    #[test]
    fn reports_are_scoped_by_window_and_workspace() {
        let state = PaneGeometryState::default();
        state.report("main", "first", geometry(760.0)).unwrap();
        state.report("aux", "first", geometry(500.0)).unwrap();
        state.report("main", "first", geometry(800.0)).unwrap();

        assert_eq!(
            state.authority_for("main", "first"),
            PaneGeometryAuthority::Rendered(geometry(800.0))
        );
        assert_eq!(state.latest_for_window("main"), Some(geometry(800.0)));
        assert_eq!(
            state.authority_for("aux", "first"),
            PaneGeometryAuthority::Rendered(geometry(500.0))
        );
        assert_eq!(
            state.authority_for("main", "unrendered"),
            PaneGeometryAuthority::WorkspaceUnrendered
        );
        assert_eq!(
            state.authority_for("missing", "first"),
            PaneGeometryAuthority::Uninitialized
        );
    }

    #[test]
    fn rejects_geometry_that_cannot_describe_a_rendered_portal() {
        let state = PaneGeometryState::default();

        assert!(state.report("", "workspace", geometry(760.0)).is_err());
        assert!(state.report("main", "", geometry(760.0)).is_err());
        assert!(state.report("main", "workspace", geometry(0.0)).is_err());
        assert!(state
            .report("main", "workspace", geometry(f64::NAN))
            .is_err());
        assert_eq!(
            state.authority_for("main", "workspace"),
            PaneGeometryAuthority::Uninitialized
        );
    }
}
