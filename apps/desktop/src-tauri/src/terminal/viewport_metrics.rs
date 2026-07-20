use super::TerminalState;

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

pub(super) fn record_terminal_cell_dimensions(
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
    *session
        .cell_dimensions
        .lock()
        .map_err(|_| "terminal cell dimensions mutex poisoned".to_string())? = Some(dimensions);
    Ok(())
}

pub(crate) fn terminal_grid_metrics_for_panel(
    state: &TerminalState,
    panel_id: &str,
) -> Option<(usize, usize, u16, u16)> {
    let registry = state.registry.lock().ok()?;
    if registry.reserved_panel_ids.contains(panel_id) {
        return None;
    }
    let session = registry
        .sessions
        .values()
        .find(|session| session.panel_id.as_deref() == Some(panel_id))?;
    let grid = session.grid.clone();
    let cell_dimensions = session.cell_dimensions.clone();
    drop(registry);

    let size = grid.lock().ok()?.size();
    let cells = (*cell_dimensions.lock().ok()?)?;
    Some((
        size.columns,
        size.screen_lines,
        cells.width_px,
        cells.height_px,
    ))
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
