use std::collections::HashMap;
use std::sync::Mutex;

use tauri::State;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WorkspacePaneGeometry {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

#[derive(Default)]
pub struct PaneGeometryState {
    by_workspace: Mutex<HashMap<String, WorkspacePaneGeometry>>,
}

impl PaneGeometryState {
    fn report(&self, workspace_id: &str, geometry: WorkspacePaneGeometry) -> Result<(), String> {
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
        self.by_workspace
            .lock()
            .expect("pane geometry mutex poisoned")
            .insert(workspace_id.to_string(), geometry);
        Ok(())
    }

    pub(crate) fn geometry_for(&self, workspace_id: &str) -> Option<WorkspacePaneGeometry> {
        self.by_workspace
            .lock()
            .expect("pane geometry mutex poisoned")
            .get(workspace_id)
            .copied()
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn pane_report_geometry(
    state: State<'_, PaneGeometryState>,
    workspace_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    state.report(
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
    use super::{PaneGeometryState, WorkspacePaneGeometry};

    fn geometry(width: f64) -> WorkspacePaneGeometry {
        WorkspacePaneGeometry {
            x: 240.0,
            y: 28.0,
            width,
            height: 672.0,
        }
    }

    #[test]
    fn reports_replace_geometry_without_crossing_workspaces() {
        let state = PaneGeometryState::default();
        state.report("first", geometry(760.0)).unwrap();
        state.report("second", geometry(500.0)).unwrap();
        state.report("first", geometry(800.0)).unwrap();

        assert_eq!(state.geometry_for("first"), Some(geometry(800.0)));
        assert_eq!(state.geometry_for("second"), Some(geometry(500.0)));
        assert_eq!(state.geometry_for("missing"), None);
    }

    #[test]
    fn rejects_geometry_that_cannot_describe_a_rendered_portal() {
        let state = PaneGeometryState::default();

        assert!(state.report("", geometry(760.0)).is_err());
        assert!(state.report("workspace", geometry(0.0)).is_err());
        assert!(state.report("workspace", geometry(f64::NAN)).is_err());
        assert_eq!(state.geometry_for("workspace"), None);
    }
}
