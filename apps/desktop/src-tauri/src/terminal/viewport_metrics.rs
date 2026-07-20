use std::sync::Mutex;

use tauri::State;

use super::{terminal_resize_id_for_control, TerminalState};

#[derive(Default)]
pub(super) struct TerminalViewportMetrics {
    cell_dimensions: Mutex<Option<TerminalCellDimensions>>,
    pane_grid_fields: Mutex<Option<(u64, u64, u64, u64)>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalCellDimensions {
    pub(super) width_px: u16,
    pub(super) height_px: u16,
}

pub(super) fn parse_terminal_cell_dimensions(
    width_px: Option<u16>,
    height_px: Option<u16>,
) -> Result<Option<TerminalCellDimensions>, String> {
    match (width_px, height_px) {
        (None, None) => Ok(None),
        (Some(width_px), Some(height_px)) if width_px > 0 && height_px > 0 => {
            Ok(Some(TerminalCellDimensions {
                width_px,
                height_px,
            }))
        }
        _ => Err("terminal cell dimensions must both be positive".to_string()),
    }
}

fn record_terminal_cell_dimensions(
    state: &TerminalState,
    id: u32,
    dimensions: Option<TerminalCellDimensions>,
) -> Result<(), String> {
    let Some(dimensions) = dimensions else {
        return Ok(());
    };
    let registry = state.try_runtime_registry()?;
    let session = registry
        .sessions
        .get(&id)
        .ok_or_else(|| format!("unknown terminal session {id}"))?;
    let viewport_metrics = session.viewport_metrics.clone();
    let grid = session.grid.clone();
    drop(registry);
    *viewport_metrics
        .cell_dimensions
        .lock()
        .map_err(|_| "terminal cell dimensions mutex poisoned".to_string())? = Some(dimensions);
    let size = grid
        .lock()
        .map_err(|_| "terminal grid mutex poisoned".to_string())?
        .size();
    *viewport_metrics
        .pane_grid_fields
        .lock()
        .map_err(|_| "terminal pane grid mutex poisoned".to_string())? = Some((
        size.columns as u64,
        size.screen_lines as u64,
        u64::from(dimensions.width_px),
        u64::from(dimensions.height_px),
    ));
    Ok(())
}

/// Resize a session's pseudo console (the explicit Windows analogue of SIGWINCH).
#[tauri::command]
pub fn terminal_resize(
    state: State<'_, TerminalState>,
    id: u32,
    cols: u16,
    rows: u16,
    cell_width_px: Option<u16>,
    cell_height_px: Option<u16>,
) -> Result<(), String> {
    let dimensions = parse_terminal_cell_dimensions(cell_width_px, cell_height_px)?;
    terminal_resize_id_for_control(state.inner(), id, cols, rows)?;
    record_terminal_cell_dimensions(state.inner(), id, dimensions)
}

pub(crate) fn terminal_pane_grid_fields_for_panel(
    state: &TerminalState,
    panel_id: &str,
) -> Option<(u64, u64, u64, u64)> {
    let registry = state.registry.lock().ok()?;
    let viewport_metrics = registry
        .sessions
        .values()
        .find(|session| session.panel_id.as_deref() == Some(panel_id))?
        .viewport_metrics
        .clone();
    drop(registry);
    let retained = *viewport_metrics.pane_grid_fields.lock().ok()?;
    retained
}

pub(crate) fn terminal_remember_pane_grid_fields(
    state: &TerminalState,
    panel_id: &str,
    fields: (u64, u64, u64, u64),
) {
    let Some(viewport_metrics) = state.registry.lock().ok().and_then(|registry| {
        registry
            .sessions
            .values()
            .find(|session| session.panel_id.as_deref() == Some(panel_id))
            .map(|session| session.viewport_metrics.clone())
    }) else {
        return;
    };
    if let Ok(mut cache) = viewport_metrics.pane_grid_fields.lock() {
        *cache = Some(fields);
    };
}

#[cfg(test)]
mod tests {
    use super::{parse_terminal_cell_dimensions, TerminalCellDimensions};

    #[test]
    fn cell_dimensions_require_a_complete_positive_pair() {
        assert_eq!(parse_terminal_cell_dimensions(None, None).unwrap(), None);
        assert_eq!(
            parse_terminal_cell_dimensions(Some(8), Some(17)).unwrap(),
            Some(TerminalCellDimensions {
                width_px: 8,
                height_px: 17,
            })
        );
        assert!(parse_terminal_cell_dimensions(Some(8), None).is_err());
        assert!(parse_terminal_cell_dimensions(Some(0), Some(17)).is_err());
    }
}
