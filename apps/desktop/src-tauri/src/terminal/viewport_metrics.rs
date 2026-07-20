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
    let cell_dimensions = session.cell_dimensions.clone();
    let grid = session.grid.clone();
    let pane_grid_fields = session.pane_grid_fields.clone();
    drop(registry);
    *cell_dimensions
        .lock()
        .map_err(|_| "terminal cell dimensions mutex poisoned".to_string())? = Some(dimensions);
    let size = grid
        .lock()
        .map_err(|_| "terminal grid mutex poisoned".to_string())?
        .size();
    *pane_grid_fields
        .lock()
        .map_err(|_| "terminal pane grid mutex poisoned".to_string())? = Some((
        size.columns as u64,
        size.screen_lines as u64,
        u64::from(dimensions.width_px),
        u64::from(dimensions.height_px),
    ));
    Ok(())
}

pub(crate) fn terminal_pane_grid_fields_for_panel(
    state: &TerminalState,
    panel_id: &str,
) -> Option<(u64, u64, u64, u64)> {
    let registry = state.registry.lock().ok()?;
    let fields = registry
        .sessions
        .values()
        .find(|session| session.panel_id.as_deref() == Some(panel_id))?
        .pane_grid_fields
        .clone();
    drop(registry);
    let retained = *fields.lock().ok()?;
    retained
}

pub(crate) fn terminal_remember_pane_grid_fields(
    state: &TerminalState,
    panel_id: &str,
    fields: (u64, u64, u64, u64),
) {
    let Some(cache) = state.registry.lock().ok().and_then(|registry| {
        registry
            .sessions
            .values()
            .find(|session| session.panel_id.as_deref() == Some(panel_id))
            .map(|session| session.pane_grid_fields.clone())
    }) else {
        return;
    };
    if let Ok(mut cache) = cache.lock() {
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
