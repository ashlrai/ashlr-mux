//! Browser history and persisted pane-state mutations.

use cmux_browser_history::{NavigationAvailability, SessionHistoryURLSanitizer};

use super::Layout;

fn browser_history_sanitizer() -> SessionHistoryURLSanitizer {
    SessionHistoryURLSanitizer::new(|url| {
        let Some(url) = url else {
            return false;
        };
        if matches!(url.scheme(), "cmux-diff-viewer" | "cmux-remote-image") {
            return true;
        }
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        matches!(
            url.host_str().map(|host| host.to_ascii_lowercase()),
            Some(host) if matches!(
                host.as_str(),
                "cmux-diff-viewer.localhost" | "cmux-remote-image.localhost"
            )
        )
    })
}

/// Return the canonical persisted browser-history string for `url`, dropping
/// blank, `about:blank`, invalid, and temporary app-proxy URLs.
pub fn serializable_browser_history_url(url: &str) -> Option<String> {
    let sanitizer = browser_history_sanitizer();
    let sanitized = sanitizer.sanitized_session_history_url(Some(url))?;
    sanitizer.serializable_session_history_url_string(Some(&sanitized))
}

fn push_browser_history_url(stack: &mut Option<Vec<String>>, url: &str) {
    if let Some(serialized) = serializable_browser_history_url(url) {
        stack.get_or_insert_with(Vec::new).push(serialized);
    }
}

fn sanitize_browser_history_stack(stack: &mut Option<Vec<String>>) {
    let Some(urls) = stack.as_mut() else {
        return;
    };
    let sanitized: Vec<String> = urls
        .iter()
        .filter_map(|url| serializable_browser_history_url(url))
        .collect();
    if sanitized.is_empty() {
        *stack = None;
    } else {
        *urls = sanitized;
    }
}

fn pop_browser_history_url(stack: &mut Option<Vec<String>>) -> Option<String> {
    sanitize_browser_history_stack(stack);
    loop {
        let Some(candidate) = stack.as_mut().and_then(Vec::pop) else {
            *stack = None;
            return None;
        };
        if stack.as_ref().is_some_and(Vec::is_empty) {
            *stack = None;
        }
        if let Some(serialized) = serializable_browser_history_url(&candidate) {
            return Some(serialized);
        }
    }
}

fn has_serializable_browser_history_url(stack: Option<&[String]>) -> bool {
    stack.is_some_and(|urls| {
        urls.iter()
            .any(|url| serializable_browser_history_url(url).is_some())
    })
}

/// Resolve pane-local browser back/forward availability using the same
/// sanitizer as the session-history replay model.
pub fn browser_navigation_availability(
    back_history: Option<&[String]>,
    forward_history: Option<&[String]>,
) -> NavigationAvailability {
    NavigationAvailability::new(
        has_serializable_browser_history_url(back_history),
        has_serializable_browser_history_url(forward_history),
    )
}
/// Bind or clear the browser URL of the pane that holds `panel_id`.
pub fn set_browser_url(node: &mut Layout, panel_id: &str, url: Option<String>) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                p.browser_url = url;
                if p.browser_url.is_none() {
                    p.browser_back_history = None;
                    p.browser_forward_history = None;
                }
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            set_browser_url(&mut s.first, panel_id, url.clone())
                || set_browser_url(&mut s.second, panel_id, url)
        }
    }
}

/// Navigate the pane-local browser to `url`, preserving back/forward stacks.
/// The history vectors are stacks: the last entry is the next destination.
pub fn navigate_browser(node: &mut Layout, panel_id: &str, url: String) -> bool {
    match node {
        Layout::Pane(p) => {
            if !p.panel_ids.iter().any(|id| id == panel_id) {
                return false;
            }
            if p.browser_url.as_deref() == Some(url.as_str()) {
                return true;
            }
            if let Some(current) = p.browser_url.as_deref() {
                push_browser_history_url(&mut p.browser_back_history, current);
            }
            p.browser_url = Some(url);
            p.browser_forward_history = None;
            true
        }
        Layout::Split(s) => {
            navigate_browser(&mut s.first, panel_id, url.clone())
                || navigate_browser(&mut s.second, panel_id, url)
        }
    }
}

/// Move the pane-local browser to the previous history entry, if any.
pub fn browser_go_back(node: &mut Layout, panel_id: &str) -> bool {
    match node {
        Layout::Pane(p) => {
            if !p.panel_ids.iter().any(|id| id == panel_id) {
                return false;
            }
            let Some(previous) = pop_browser_history_url(&mut p.browser_back_history) else {
                return false;
            };
            if let Some(current) = p.browser_url.as_deref() {
                push_browser_history_url(&mut p.browser_forward_history, current);
            }
            p.browser_url = Some(previous);
            true
        }
        Layout::Split(s) => {
            browser_go_back(&mut s.first, panel_id) || browser_go_back(&mut s.second, panel_id)
        }
    }
}

/// Move the pane-local browser to the next forward-history entry, if any.
pub fn browser_go_forward(node: &mut Layout, panel_id: &str) -> bool {
    match node {
        Layout::Pane(p) => {
            if !p.panel_ids.iter().any(|id| id == panel_id) {
                return false;
            }
            let Some(next) = pop_browser_history_url(&mut p.browser_forward_history) else {
                return false;
            };
            if let Some(current) = p.browser_url.as_deref() {
                push_browser_history_url(&mut p.browser_back_history, current);
            }
            p.browser_url = Some(next);
            true
        }
        Layout::Split(s) => {
            browser_go_forward(&mut s.first, panel_id)
                || browser_go_forward(&mut s.second, panel_id)
        }
    }
}

/// Clear the pane-local browser back/forward history while keeping the current
/// URL and zoom. Returns true only when the stored history changed.
pub fn clear_browser_history(node: &mut Layout, panel_id: &str) -> bool {
    match node {
        Layout::Pane(p) => {
            if !p.panel_ids.iter().any(|id| id == panel_id) {
                return false;
            }
            let changed = p.browser_back_history.is_some() || p.browser_forward_history.is_some();
            p.browser_back_history = None;
            p.browser_forward_history = None;
            changed
        }
        Layout::Split(s) => {
            clear_browser_history(&mut s.first, panel_id)
                || clear_browser_history(&mut s.second, panel_id)
        }
    }
}

/// Toggle the pane-local browser omnibar. Absence means visible, so the first
/// toggle stores `false`.
pub fn toggle_browser_omnibar_visible(node: &mut Layout, panel_id: &str) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                let current = p.browser_omnibar_visible.unwrap_or(true);
                p.browser_omnibar_visible = Some(!current);
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            toggle_browser_omnibar_visible(&mut s.first, panel_id)
                || toggle_browser_omnibar_visible(&mut s.second, panel_id)
        }
    }
}

/// Toggle pane-local browser focus mode. Absence means inactive.
pub fn toggle_browser_focus_mode(node: &mut Layout, panel_id: &str) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                let current = p.browser_focus_mode_active.unwrap_or(false);
                p.browser_focus_mode_active = Some(!current);
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            toggle_browser_focus_mode(&mut s.first, panel_id)
                || toggle_browser_focus_mode(&mut s.second, panel_id)
        }
    }
}

/// Toggle the pane-local browser developer-tools drawer. When opening without
/// a selected panel, default to the inspector lane.
pub fn toggle_browser_developer_tools(node: &mut Layout, panel_id: &str) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                let current = p.browser_developer_tools_visible.unwrap_or(false);
                p.browser_developer_tools_visible = Some(!current);
                if !current && p.browser_developer_tools_panel.is_none() {
                    p.browser_developer_tools_panel = Some("inspector".to_string());
                }
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            toggle_browser_developer_tools(&mut s.first, panel_id)
                || toggle_browser_developer_tools(&mut s.second, panel_id)
        }
    }
}

/// Show the pane-local browser developer-tools drawer on a specific lane.
pub fn show_browser_developer_tools(
    node: &mut Layout,
    panel_id: &str,
    panel: impl Into<String>,
) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                p.browser_developer_tools_visible = Some(true);
                p.browser_developer_tools_panel = Some(panel.into());
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            let panel = panel.into();
            show_browser_developer_tools(&mut s.first, panel_id, panel.clone())
                || show_browser_developer_tools(&mut s.second, panel_id, panel)
        }
    }
}

/// Ensure a pane-local browser zoom exists without resetting an existing zoom.
pub fn ensure_browser_page_zoom(node: &mut Layout, panel_id: &str, zoom: f64) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                if p.browser_page_zoom.is_none() {
                    p.browser_page_zoom = Some(zoom);
                }
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            ensure_browser_page_zoom(&mut s.first, panel_id, zoom)
                || ensure_browser_page_zoom(&mut s.second, panel_id, zoom)
        }
    }
}

/// Set the browser zoom factor for the pane that holds `panel_id`.
pub fn set_browser_page_zoom(node: &mut Layout, panel_id: &str, zoom: Option<f64>) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                p.browser_page_zoom = zoom;
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            set_browser_page_zoom(&mut s.first, panel_id, zoom)
                || set_browser_page_zoom(&mut s.second, panel_id, zoom)
        }
    }
}
