#[derive(Debug, Clone, PartialEq)]
pub(in crate::control_socket) struct LifecycleDispatchContext {
    pub(in crate::control_socket) browser_enabled: bool,
    pub(in crate::control_socket) dock_available: bool,
    pub(in crate::control_socket) active_window_id: Option<String>,
    pub(in crate::control_socket) rendered_pane_size: Option<(f64, f64)>,
}

impl LifecycleDispatchContext {
    pub(in crate::control_socket) fn new(
        browser_enabled: bool,
        dock_available: bool,
        active_window_id: Option<String>,
    ) -> Self {
        Self {
            browser_enabled,
            dock_available,
            active_window_id,
            rendered_pane_size: None,
        }
    }

    #[cfg(test)]
    pub(in crate::control_socket) fn with_rendered_pane_size(
        mut self,
        width: f64,
        height: f64,
    ) -> Self {
        self.rendered_pane_size = Some((width, height));
        self
    }
}
