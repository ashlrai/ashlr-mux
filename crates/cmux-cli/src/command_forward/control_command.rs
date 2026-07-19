use super::{apply_window_selector_value, workspace_scoped_method};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlCommand {
    pub method: String,
    pub params: serde_json::Value,
}

impl ControlCommand {
    pub(super) fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }

    pub(super) fn from_cli(
        method: impl Into<String>,
        command: &'static str,
        mut params: serde_json::Value,
    ) -> Self {
        params
            .as_object_mut()
            .expect("CLI control params must be an object")
            .insert("__cmux_cli_command".into(), serde_json::json!(command));
        Self::new(method, params)
    }

    pub fn with_ambient_workspace_id(mut self, workspace_id: Option<&str>) -> Self {
        let Some(workspace_id) = workspace_id.map(str::trim) else {
            return self;
        };
        if !workspace_scoped_method(&self.method) {
            return self;
        }
        let Some(params) = self.params.as_object_mut() else {
            return self;
        };
        if params.contains_key("suppress_ambient_workspace") {
            return self;
        }
        if workspace_id.is_empty() {
            if matches!(self.method.as_str(), "tab.action" | "surface.respawn") {
                params.remove("resolve_current_workspace");
                params.insert("suppress_ambient_workspace".into(), serde_json::json!(true));
            }
            return self;
        }
        if ["workspace_id", "workspace_ref", "workspace_index"]
            .iter()
            .any(|key| params.contains_key(*key))
        {
            return self;
        }
        params.insert("workspace_id".to_string(), serde_json::json!(workspace_id));
        self
    }

    pub fn with_window_id(mut self, window_id: Option<&str>) -> Self {
        if !self.method.starts_with("workspace.group.")
            && !matches!(
                self.method.as_str(),
                "surface.read_text"
                    | "surface.clear_history"
                    | "surface.trigger_flash"
                    | "notification.clear"
                    | "notification.create"
                    | "window.current"
                    | "window.display"
                    | "right_sidebar"
                    | "workspace.list"
                    | "workspace.current"
                    | "workspace.create"
                    | "workspace.close"
                    | "workspace.select"
                    | "workspace.rename"
                    | "workspace.action"
                    | "tab.action"
                    | "surface.respawn"
            )
        {
            return self;
        }
        let Some(window_id) = window_id.map(str::trim) else {
            return self;
        };
        let Some(params) = self.params.as_object_mut() else {
            return self;
        };
        if window_id.is_empty() {
            if matches!(self.method.as_str(), "tab.action" | "surface.respawn") {
                params.insert("suppress_ambient_window".into(), serde_json::json!(true));
            }
            return self;
        }
        if params.contains_key("suppress_ambient_window") {
            return self;
        }
        if ["window_id", "window_ref", "window_index"]
            .iter()
            .any(|key| params.contains_key(*key))
        {
            return self;
        }
        apply_window_selector_value(window_id, params);
        self
    }

    pub fn with_ambient_surface_id(mut self, surface_id: Option<&str>) -> Self {
        let Some(surface_id) = surface_id.map(str::trim) else {
            return self;
        };
        if !self.method.starts_with("workspace.group.")
            && !matches!(
                self.method.as_str(),
                "workspace.list"
                    | "workspace.current"
                    | "workspace.create"
                    | "workspace.rename"
                    | "tab.action"
                    | "surface.respawn"
            )
        {
            return self;
        }
        let Some(params) = self.params.as_object_mut() else {
            return self;
        };
        if params.contains_key("suppress_ambient_surface")
            || params.contains_key("suppress_ambient_workspace")
            || params.contains_key("suppress_ambient_window")
        {
            return self;
        }
        if surface_id.is_empty() && matches!(self.method.as_str(), "tab.action" | "surface.respawn")
        {
            params.insert("suppress_ambient_surface".into(), serde_json::json!(true));
            return self;
        }
        let has_explicit_scope = [
            "window_id",
            "window_ref",
            "window_index",
            "workspace_id",
            "workspace_ref",
            "workspace_index",
            "surface_id",
            "surface_ref",
            "surface_index",
        ]
        .iter()
        .any(|key| params.contains_key(*key));
        if !has_explicit_scope {
            params
                .entry("surface_id")
                .or_insert_with(|| serde_json::json!(surface_id));
        }
        self
    }
}
