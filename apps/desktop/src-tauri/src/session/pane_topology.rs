use super::*;

pub(crate) fn finalize_broken_pane_snapshot(
    snapshot: &mut AppSessionSnapshot,
    window_index: usize,
    broken: &session_ops::PaneBreakResult,
) -> bool {
    if !remint_broken_pane_identity(snapshot, window_index, broken) {
        return false;
    }
    ensure_workspace_ids(snapshot);
    ensure_pane_ids(snapshot);
    true
}
