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
    id: Option<&str>,
    index: Option<usize>,
    mint: &mut impl FnMut(&'static str, &str) -> String,
) -> Option<String> {
    id.map(|id| mint(kind, id)).or_else(|| {
        index.map(|index| match kind {
            "pane" => pane_ref(index),
            "surface" => surface_ref(index),
            _ => unreachable!("unsupported indexed response ref kind: {kind}"),
        })
    })
}

pub(super) fn pane_response_ref(app: &AppHandle, pane_id: Option<&str>, index: usize) -> String {
    indexed_response_ref_with("pane", pane_id, Some(index), &mut |kind, id| {
        control_handle_ref(app, kind, id)
    })
    .expect("pane id always resolves a response ref")
}

pub(super) fn surface_response_ref(app: &AppHandle, surface_id: &str, index: usize) -> String {
    indexed_response_ref_with("surface", Some(surface_id), Some(index), &mut |kind, id| {
        control_handle_ref(app, kind, id)
    })
    .expect("surface id always resolves a response ref")
}
