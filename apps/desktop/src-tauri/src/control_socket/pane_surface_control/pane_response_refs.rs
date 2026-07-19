use super::*;

pub(in crate::control_socket) fn pane_response_window_identity(
    window: &SessionWindowSnapshot,
    window_index: usize,
) -> (Option<String>, Option<String>) {
    (
        window.window_id.clone(),
        window.window_id.as_ref().map(|_| window_ref(window_index)),
    )
}

pub(in crate::control_socket) fn indexed_response_ref_with(
    kind: &'static str,
    _id: Option<&str>,
    index: Option<usize>,
    _mint: &mut impl FnMut(&'static str, &str) -> String,
) -> Option<String> {
    index.map(|index| match kind {
        "pane" => pane_ref(index),
        "surface" => surface_ref(index),
        _ => unreachable!("unsupported indexed response ref kind: {kind}"),
    })
}
