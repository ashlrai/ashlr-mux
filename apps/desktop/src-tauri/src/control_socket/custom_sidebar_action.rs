use cmux_ipc::{ControlCallResult, ControlRequest, JsonValue};
use serde_json::{json, Value};
use tauri::AppHandle;

use super::{
    custom_sidebar_manifest_for_source, f64_param, handle_control_request, i64_param,
    invalid_params, raw_string_param, string_param, usize_param,
};

pub(super) const CUSTOM_SIDEBAR_ACTION_POLICY: &str = "cmux-custom-sidebar-safe-default";
pub(super) const CUSTOM_SIDEBAR_ACTION_SCHEMA_VERSION: u32 = 1;
pub(super) const CUSTOM_SIDEBAR_ALLOWED_ACTION_METHODS: &[&str] = &[
    "extension.sidebar.snapshot",
    "sidebar.snapshot",
    "sidebar.list",
    "sidebar.validate",
    "sidebar.reload",
    "sidebar.select",
    "sidebar.open",
    "workspace.list",
    "workspace.current",
    "workspace.select",
    "workspace.next",
    "workspace.previous",
    "workspace.last",
    "workspace.sidebar_state",
    "workspace.list_status",
    "workspace.list_log",
    "workspace.list_meta",
    "workspace.list_meta_blocks",
    "workspace.set_progress",
    "workspace.clear_progress",
    "workspace.set_status",
    "workspace.clear_status",
    "workspace.report_meta",
    "workspace.clear_meta",
    "workspace.report_meta_block",
    "workspace.clear_meta_block",
    "workspace.log",
    "workspace.clear_log",
    "workspace.reset_sidebar",
    "surface.list",
    "surface.focus",
    "surface.next",
    "surface.previous",
    "set_progress",
    "set-progress",
    "clear_progress",
    "clear-progress",
    "set_status",
    "set-status",
    "clear_status",
    "clear-status",
    "list_status",
    "list-status",
    "report_meta",
    "report-meta",
    "clear_meta",
    "clear-meta",
    "list_meta",
    "list-meta",
    "report_meta_block",
    "report-meta-block",
    "clear_meta_block",
    "clear-meta-block",
    "list_meta_blocks",
    "list-meta-blocks",
    "log",
    "clear_log",
    "clear-log",
    "reset_sidebar",
    "reset-sidebar",
];

const CUSTOM_SIDEBAR_WORKSPACE_SELECTOR_KEYS: &[&str] =
    &["workspace_id", "id", "workspace_ref", "ref"];
const CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS: &[&str] =
    &["surface_id", "panel_id", "id", "surface_ref", "ref"];

pub(super) fn invoke_custom_sidebar_action(
    app: &AppHandle,
    method: String,
    params: Option<Value>,
    source_path: Option<&str>,
) -> Value {
    custom_sidebar_action_reply(handle_custom_sidebar_action_request(
        app,
        method,
        params.unwrap_or(Value::Object(serde_json::Map::new())),
        source_path,
    ))
}

fn handle_custom_sidebar_action_request(
    app: &AppHandle,
    method: String,
    params: Value,
    source_path: Option<&str>,
) -> ControlCallResult {
    let method = method.trim();
    if method.is_empty() {
        return invalid_params("Missing custom sidebar action method");
    }
    let Value::Object(params) = params else {
        return invalid_params("Custom sidebar action params must be a JSON object");
    };
    if !custom_sidebar_action_policy_allows(method) {
        return custom_sidebar_action_denied(
            method,
            custom_sidebar_manifest_for_source(source_path),
        );
    }
    if let Err(error) = validate_custom_sidebar_action_schema(method, &params) {
        return error;
    }
    handle_control_request(app, ControlRequest::new(None, method.to_string(), params))
}

pub(super) fn custom_sidebar_action_policy_allows(method: &str) -> bool {
    CUSTOM_SIDEBAR_ALLOWED_ACTION_METHODS.contains(&method)
}

pub(super) fn custom_sidebar_action_denied(
    method: &str,
    manifest: Option<Value>,
) -> ControlCallResult {
    ControlCallResult::Err {
        code: "custom_sidebar_capability_denied".to_string(),
        message: format!("Custom sidebar action '{method}' is outside the safe capability scope"),
        data: JsonValue::try_from(json!({
            "method": method,
            "policy": CUSTOM_SIDEBAR_ACTION_POLICY,
            "allowed_methods": CUSTOM_SIDEBAR_ALLOWED_ACTION_METHODS,
            "manifest": manifest,
        }))
        .ok(),
    }
}

pub(super) fn validate_custom_sidebar_action_schema(
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<(), ControlCallResult> {
    match custom_sidebar_action_schema_method(method) {
        "sidebar.select" | "sidebar.open" => {
            require_custom_sidebar_string_param(method, params, "name", &["name", "sidebar"])
        }
        "workspace.select" => require_custom_sidebar_workspace_selector(method, params),
        "workspace.set_progress" => require_custom_sidebar_finite_number_param(
            method,
            params,
            "value",
            &["value", "progress"],
        ),
        "workspace.set_status" => {
            require_custom_sidebar_string_param(method, params, "key", &["key", "name"])?;
            require_custom_sidebar_raw_string_param(
                method,
                params,
                "value",
                &["value", "status", "text"],
            )?;
            validate_custom_sidebar_optional_i64_param(method, params, "priority", &["priority"])
        }
        "workspace.clear_status" | "workspace.clear_meta" | "workspace.clear_meta_block" => {
            require_custom_sidebar_string_param(method, params, "key", &["key", "name"])
        }
        "workspace.report_meta" => {
            require_custom_sidebar_string_param(method, params, "key", &["key", "name"])?;
            require_custom_sidebar_raw_string_param(
                method,
                params,
                "value",
                &["value", "text", "markdown"],
            )?;
            validate_custom_sidebar_optional_i64_param(method, params, "priority", &["priority"])
        }
        "workspace.report_meta_block" => {
            require_custom_sidebar_string_param(method, params, "key", &["key", "name"])?;
            require_custom_sidebar_raw_string_param(
                method,
                params,
                "markdown",
                &["markdown", "value", "text"],
            )?;
            validate_custom_sidebar_optional_i64_param(method, params, "priority", &["priority"])
        }
        "workspace.log" => {
            require_custom_sidebar_raw_string_param(method, params, "message", &["message", "text"])
        }
        "surface.focus" => validate_custom_sidebar_optional_surface_selector(method, params),
        "workspace.list_log" => {
            validate_custom_sidebar_optional_usize_param(method, params, "limit", &["limit"])
        }
        method => validate_custom_sidebar_optional_common_params(method, params),
    }
}

fn custom_sidebar_action_schema_method<'a>(method: &'a str) -> &'a str {
    match method {
        "set_progress" | "set-progress" => "workspace.set_progress",
        "clear_progress" | "clear-progress" => "workspace.clear_progress",
        "set_status" | "set-status" => "workspace.set_status",
        "clear_status" | "clear-status" => "workspace.clear_status",
        "list_status" | "list-status" => "workspace.list_status",
        "report_meta" | "report-meta" => "workspace.report_meta",
        "clear_meta" | "clear-meta" => "workspace.clear_meta",
        "list_meta" | "list-meta" => "workspace.list_meta",
        "report_meta_block" | "report-meta-block" => "workspace.report_meta_block",
        "clear_meta_block" | "clear-meta-block" => "workspace.clear_meta_block",
        "list_meta_blocks" | "list-meta-blocks" => "workspace.list_meta_blocks",
        "log" => "workspace.log",
        "clear_log" | "clear-log" => "workspace.clear_log",
        "reset_sidebar" | "reset-sidebar" => "workspace.reset_sidebar",
        other => match other {
            "workspace.set_progress"
            | "workspace.clear_progress"
            | "workspace.set_status"
            | "workspace.clear_status"
            | "workspace.list_status"
            | "workspace.report_meta"
            | "workspace.clear_meta"
            | "workspace.list_meta"
            | "workspace.report_meta_block"
            | "workspace.clear_meta_block"
            | "workspace.list_meta_blocks"
            | "workspace.log"
            | "workspace.clear_log"
            | "workspace.reset_sidebar"
            | "workspace.select"
            | "surface.focus"
            | "workspace.list_log"
            | "sidebar.select"
            | "sidebar.open" => other,
            _ => other,
        },
    }
}

fn require_custom_sidebar_workspace_selector(
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<(), ControlCallResult> {
    if string_param(params, &["workspace_id", "id"]).is_some()
        || string_param(params, &["workspace_ref", "ref"]).is_some()
    {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        "workspace",
        "non-empty workspace_id/id or workspace_ref/ref string",
        CUSTOM_SIDEBAR_WORKSPACE_SELECTOR_KEYS,
    ))
}

fn validate_custom_sidebar_optional_surface_selector(
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<(), ControlCallResult> {
    if !contains_any_param(params, CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS) {
        return Ok(());
    }
    if string_param(params, &["surface_id", "panel_id", "id"]).is_some()
        || string_param(params, &["surface_ref", "ref"]).is_some()
    {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        "surface",
        "non-empty surface_id/panel_id/id or surface_ref/ref string",
        CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS,
    ))
}

fn validate_custom_sidebar_optional_common_params(
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<(), ControlCallResult> {
    validate_custom_sidebar_optional_i64_param(method, params, "priority", &["priority"])?;
    validate_custom_sidebar_optional_usize_param(method, params, "limit", &["limit"])?;
    Ok(())
}

fn require_custom_sidebar_string_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if string_param(params, keys).is_some() {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        field,
        "non-empty string",
        keys,
    ))
}

fn require_custom_sidebar_raw_string_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if raw_string_param(params, keys).is_some() {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method, field, "string", keys,
    ))
}

fn require_custom_sidebar_finite_number_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if f64_param(params, keys).is_some_and(f64::is_finite) {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        field,
        "finite number or numeric string",
        keys,
    ))
}

fn validate_custom_sidebar_optional_i64_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if !contains_any_param(params, keys) || i64_param(params, keys).is_some() {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        field,
        "integer or integer string",
        keys,
    ))
}

fn validate_custom_sidebar_optional_usize_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if !contains_any_param(params, keys) || usize_param(params, keys).is_some() {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        field,
        "non-negative integer or integer string",
        keys,
    ))
}

fn contains_any_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().any(|key| params.contains_key(*key))
}

fn custom_sidebar_action_schema_invalid(
    method: &str,
    field: &str,
    expected: &str,
    accepted_keys: &'static [&'static str],
) -> ControlCallResult {
    ControlCallResult::Err {
        code: "custom_sidebar_action_schema_invalid".to_string(),
        message: format!(
            "Custom sidebar action '{method}' has invalid params: {field} must be {expected}"
        ),
        data: JsonValue::try_from(json!({
            "method": method,
            "schema_method": custom_sidebar_action_schema_method(method),
            "schema_version": CUSTOM_SIDEBAR_ACTION_SCHEMA_VERSION,
            "field": field,
            "expected": expected,
            "accepted_keys": accepted_keys,
        }))
        .ok(),
    }
}

pub(super) fn custom_sidebar_action_schema_catalog() -> Value {
    json!({
        "version": CUSTOM_SIDEBAR_ACTION_SCHEMA_VERSION,
        "selector_keys": {
            "workspace": CUSTOM_SIDEBAR_WORKSPACE_SELECTOR_KEYS,
            "surface": CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS,
        },
        "methods": [
            {
                "method": "workspace.select",
                "required": [
                    {
                        "field": "workspace",
                        "accepted_keys": CUSTOM_SIDEBAR_WORKSPACE_SELECTOR_KEYS,
                        "expected": "non-empty workspace_id/id or workspace_ref/ref string",
                    },
                ],
            },
            {
                "method": "surface.focus",
                "optional": [
                    {
                        "field": "surface",
                        "accepted_keys": CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS,
                        "expected": "non-empty surface_id/panel_id/id or surface_ref/ref string",
                    },
                ],
            },
            {
                "method": "sidebar.select",
                "required": [
                    {
                        "field": "name",
                        "accepted_keys": ["name", "sidebar"],
                        "expected": "non-empty string",
                    },
                ],
            },
            {
                "method": "sidebar.open",
                "required": [
                    {
                        "field": "name",
                        "accepted_keys": ["name", "sidebar"],
                        "expected": "non-empty string",
                    },
                ],
            },
            {
                "method": "workspace.set_progress",
                "aliases": ["set_progress", "set-progress"],
                "required": [
                    {
                        "field": "value",
                        "accepted_keys": ["value", "progress"],
                        "expected": "finite number or numeric string",
                    },
                ],
            },
            {
                "method": "workspace.set_status",
                "aliases": ["set_status", "set-status"],
                "required": [
                    {
                        "field": "key",
                        "accepted_keys": ["key", "name"],
                        "expected": "non-empty string",
                    },
                    {
                        "field": "value",
                        "accepted_keys": ["value", "status", "text"],
                        "expected": "string",
                    },
                ],
                "optional": [
                    {
                        "field": "priority",
                        "accepted_keys": ["priority"],
                        "expected": "integer or integer string",
                    },
                ],
            },
            {
                "method": "workspace.clear_status",
                "aliases": ["clear_status", "clear-status"],
                "required": [
                    {
                        "field": "key",
                        "accepted_keys": ["key", "name"],
                        "expected": "non-empty string",
                    },
                ],
            },
            {
                "method": "workspace.report_meta",
                "aliases": ["report_meta", "report-meta"],
                "required": [
                    {
                        "field": "key",
                        "accepted_keys": ["key", "name"],
                        "expected": "non-empty string",
                    },
                    {
                        "field": "value",
                        "accepted_keys": ["value", "text", "markdown"],
                        "expected": "string",
                    },
                ],
            },
            {
                "method": "workspace.report_meta_block",
                "aliases": ["report_meta_block", "report-meta-block"],
                "required": [
                    {
                        "field": "key",
                        "accepted_keys": ["key", "name"],
                        "expected": "non-empty string",
                    },
                    {
                        "field": "markdown",
                        "accepted_keys": ["markdown", "value", "text"],
                        "expected": "string",
                    },
                ],
            },
            {
                "method": "workspace.log",
                "aliases": ["log"],
                "required": [
                    {
                        "field": "message",
                        "accepted_keys": ["message", "text"],
                        "expected": "string",
                    },
                ],
            },
        ],
    })
}

pub(super) fn custom_sidebar_action_reply(result: ControlCallResult) -> Value {
    match result {
        ControlCallResult::Ok(value) => json!({
            "ok": true,
            "value": Value::from(value),
        }),
        ControlCallResult::Err {
            code,
            message,
            data,
        } => json!({
            "ok": false,
            "error": {
                "code": code,
                "userMessage": message,
                "data": data.map(Value::from),
            },
        }),
    }
}
