use super::BrowserWebviewState;

pub(crate) fn browser_has_webview_for_control(
    state: &BrowserWebviewState,
    panel_id: &str,
) -> Result<bool, String> {
    let reserved_panel_ids = state
        .reserved_panel_ids
        .lock()
        .map_err(|_| "browser reservation state lock poisoned".to_string())?;
    if reserved_panel_ids.contains(panel_id) {
        return Err(format!("browser panel {panel_id} is reserved"));
    }
    state
        .webviews
        .lock()
        .map(|webviews| webviews.contains_key(panel_id))
        .map_err(|_| "browser webview state lock poisoned".to_string())
}
