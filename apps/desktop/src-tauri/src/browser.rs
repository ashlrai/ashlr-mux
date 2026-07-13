use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::webview::WebviewBuilder;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, Window};
use url::Url;

const BROWSER_NAVIGATED_EVENT: &str = "cmux://browser-webview-navigated";
const DEFAULT_BROWSER_URL: &str = "about:blank";
const NETWORK_RECORD_LIMIT_PER_PANEL: usize = 200;
const NETWORK_BODY_CAPTURE_LIMIT_BYTES: usize = 4 * 1024;
#[allow(dead_code)]
const NETWORK_HTTP_HEAD_CAPTURE_LIMIT_BYTES: usize = 64 * 1024;
const BROWSER_EVAL_TIMEOUT: Duration = Duration::from_secs(5);

pub struct BrowserWebviewState {
    reserved_panel_ids: Mutex<BTreeSet<String>>,
    webviews: Mutex<HashMap<String, BrowserChild>>,
    init_scripts: Mutex<HashMap<String, Vec<String>>>,
    network_records: Mutex<HashMap<String, Vec<BrowserNetworkRecord>>>,
    next_network_record_id: AtomicU64,
}

impl Default for BrowserWebviewState {
    fn default() -> Self {
        Self {
            reserved_panel_ids: Mutex::new(BTreeSet::new()),
            webviews: Mutex::new(HashMap::new()),
            init_scripts: Mutex::new(HashMap::new()),
            network_records: Mutex::new(HashMap::new()),
            next_network_record_id: AtomicU64::new(1),
        }
    }
}

struct BrowserChild {
    webview: tauri::Webview,
    label: String,
    url: String,
    proxy_url: Option<String>,
    bounds: BrowserWebviewBounds,
    visible: bool,
    zoom: Option<f64>,
}

#[allow(dead_code)]
pub(crate) struct BrowserPanelRuntimeEntry {
    child: Option<BrowserChild>,
    network_records: Option<Vec<BrowserNetworkRecord>>,
    init_scripts: Option<Vec<String>>,
}

#[allow(dead_code)]
pub(crate) struct BrowserPanelRuntimeLease {
    panel_ids: BTreeSet<String>,
    entries: BTreeMap<String, BrowserPanelRuntimeEntry>,
}

#[allow(dead_code)]
pub(crate) struct BrowserPanelRollbackError {
    pub(crate) message: String,
    pub(crate) lease: BrowserPanelRuntimeLease,
}

#[allow(dead_code)]
pub(crate) struct BrowserPanelFinalizeError {
    pub(crate) failures: Vec<String>,
    pub(crate) retry: BrowserPanelRuntimeLease,
}

struct BrowserPanelMutationReservation<'a> {
    state: &'a BrowserWebviewState,
    panel_id: String,
    active: bool,
}

impl BrowserPanelMutationReservation<'_> {
    fn release(&mut self) -> Result<(), String> {
        self.state
            .reserved_panel_ids
            .lock()
            .map_err(|_| "browser reservation state lock poisoned".to_string())?
            .remove(&self.panel_id);
        self.active = false;
        Ok(())
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for BrowserPanelMutationReservation<'_> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut reserved_panel_ids = self
            .state
            .reserved_panel_ids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reserved_panel_ids.remove(&self.panel_id);
    }
}

#[allow(dead_code)]
pub(crate) fn detach_browser_panels_for_control(
    state: &BrowserWebviewState,
    panel_ids: &BTreeSet<String>,
) -> Result<BrowserPanelRuntimeLease, String> {
    let panel_ids = panel_ids
        .iter()
        .filter_map(|panel_id| {
            let panel_id = panel_id.trim();
            (!panel_id.is_empty()).then(|| panel_id.to_string())
        })
        .collect::<BTreeSet<_>>();
    let mut reserved_panel_ids = state
        .reserved_panel_ids
        .lock()
        .map_err(|_| "browser reservation state lock poisoned".to_string())?;
    let mut webviews = state
        .webviews
        .lock()
        .map_err(|_| "browser webview state lock poisoned".to_string())?;
    let mut network_records = state
        .network_records
        .lock()
        .map_err(|_| "browser network record state lock poisoned".to_string())?;
    let mut init_scripts = state
        .init_scripts
        .lock()
        .map_err(|_| "browser init script state lock poisoned".to_string())?;
    if let Some(panel_id) = panel_ids
        .iter()
        .find(|panel_id| reserved_panel_ids.contains(*panel_id))
    {
        return Err(format!("browser panel {panel_id} is already reserved"));
    }

    reserved_panel_ids.extend(panel_ids.iter().cloned());
    let entries = panel_ids
        .iter()
        .map(|panel_id| {
            (
                panel_id.clone(),
                BrowserPanelRuntimeEntry {
                    child: webviews.remove(panel_id),
                    network_records: network_records.remove(panel_id),
                    init_scripts: init_scripts.remove(panel_id),
                },
            )
        })
        .collect();
    Ok(BrowserPanelRuntimeLease { panel_ids, entries })
}

#[allow(dead_code)]
pub(crate) fn rollback_browser_panels_for_control(
    state: &BrowserWebviewState,
    lease: BrowserPanelRuntimeLease,
) -> Result<(), BrowserPanelRollbackError> {
    let mut reserved_panel_ids = state
        .reserved_panel_ids
        .lock()
        .expect("browser reservation state lock poisoned");
    let mut webviews = state
        .webviews
        .lock()
        .expect("browser webview state lock poisoned");
    let mut network_records = state
        .network_records
        .lock()
        .expect("browser network record state lock poisoned");
    let mut init_scripts = state
        .init_scripts
        .lock()
        .expect("browser init script state lock poisoned");
    let lost_ownership = lease
        .panel_ids
        .iter()
        .find(|panel_id| !reserved_panel_ids.contains(*panel_id));
    let collision = lease.entries.iter().find_map(|(panel_id, entry)| {
        (entry.child.is_some() && webviews.contains_key(panel_id))
            .then(|| format!("webview collision for {panel_id}"))
            .or_else(|| {
                (entry.network_records.is_some() && network_records.contains_key(panel_id))
                    .then(|| format!("network record collision for {panel_id}"))
            })
            .or_else(|| {
                (entry.init_scripts.is_some() && init_scripts.contains_key(panel_id))
                    .then(|| format!("init script collision for {panel_id}"))
            })
    });
    if lost_ownership.is_some() || collision.is_some() {
        let message = lost_ownership
            .map(|panel_id| format!("reservation ownership lost for {panel_id}"))
            .or(collision)
            .expect("browser rollback precheck failed");
        return Err(BrowserPanelRollbackError { message, lease });
    }

    for (panel_id, entry) in lease.entries {
        if let Some(child) = entry.child {
            webviews.insert(panel_id.clone(), child);
        }
        if let Some(records) = entry.network_records {
            network_records.insert(panel_id.clone(), records);
        }
        if let Some(scripts) = entry.init_scripts {
            init_scripts.insert(panel_id, scripts);
        }
    }
    for panel_id in lease.panel_ids {
        reserved_panel_ids.remove(&panel_id);
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn finalize_browser_panels_for_control(
    state: &BrowserWebviewState,
    lease: BrowserPanelRuntimeLease,
) -> Result<(), BrowserPanelFinalizeError> {
    let reserved_panel_ids = state
        .reserved_panel_ids
        .lock()
        .expect("browser reservation state lock poisoned");
    let owned = lease
        .panel_ids
        .iter()
        .all(|panel_id| reserved_panel_ids.contains(panel_id));
    drop(reserved_panel_ids);
    if !owned {
        return Err(BrowserPanelFinalizeError {
            failures: vec!["browser finalize reservation ownership lost".to_string()],
            retry: lease,
        });
    }

    let mut failures = Vec::new();
    let mut retry_panel_ids = BTreeSet::new();
    let mut retry = BTreeMap::new();
    for (panel_id, mut entry) in lease.entries {
        let result = entry.child.as_mut().map_or(Ok(()), |child| {
            child.webview.close().map_err(|error| error.to_string())
        });
        if let Err(error) = result {
            failures.push(format!("{panel_id}: {error}"));
            retry_panel_ids.insert(panel_id.clone());
            retry.insert(panel_id, entry);
        }
    }

    let mut reserved_panel_ids = state
        .reserved_panel_ids
        .lock()
        .expect("browser reservation state lock poisoned");
    for panel_id in &lease.panel_ids {
        if !retry_panel_ids.contains(panel_id) {
            reserved_panel_ids.remove(panel_id);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(BrowserPanelFinalizeError {
            failures,
            retry: BrowserPanelRuntimeLease {
                panel_ids: retry_panel_ids,
                entries: retry,
            },
        })
    }
}

fn finalize_or_restore_browser_panels(
    state: &BrowserWebviewState,
    lease: BrowserPanelRuntimeLease,
) -> Result<(), String> {
    match finalize_browser_panels_for_control(state, lease) {
        Ok(()) => Ok(()),
        Err(finalize) => {
            let message = finalize.failures.join("; ");
            rollback_browser_panels_for_control(state, finalize.retry)
                .map_err(|rollback| format!("{message}; {}", rollback.message))?;
            Err(message)
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserNetworkRecord {
    pub id: String,
    pub panel_id: String,
    pub url: String,
    pub method: String,
    pub request_headers: BTreeMap<String, String>,
    pub request_body: Option<String>,
    pub request_body_preview_kind: String,
    pub request_body_size: usize,
    pub request_body_truncated: bool,
    pub response_status: Option<u16>,
    pub response_headers: BTreeMap<String, String>,
    pub response_body: Option<String>,
    pub response_body_preview_kind: String,
    pub response_body_size: usize,
    pub response_body_truncated: bool,
    pub started_at_ms: u64,
    pub completed_at_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub source: String,
    pub transport: String,
    pub proxy_attribution: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserNetworkRequestsReply {
    pub panel_id: String,
    pub requests: Vec<BrowserNetworkRecord>,
    pub observer: BrowserNetworkObserverSummary,
    pub total_count: usize,
    pub returned_count: usize,
    pub filtered_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserNetworkClearReply {
    pub panel_id: String,
    pub cleared_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserNetworkObserverSummary {
    pub source: String,
    pub captures_url: bool,
    pub captures_method: bool,
    pub captures_request_headers: bool,
    pub captures_request_body: bool,
    pub captures_response_status: bool,
    pub captures_response_headers: bool,
    pub captures_response_body: bool,
    pub captures_timing: bool,
    pub supports_filters: bool,
    pub max_records_per_panel: usize,
    pub body_capture_limit_bytes: usize,
    pub proxy_attribution_mode: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserNetworkRequestsQuery {
    pub url_contains: Option<String>,
    pub method: Option<String>,
    pub since_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWebviewBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWebviewReply {
    pub panel_id: String,
    pub label: String,
    pub url: String,
    pub attached: bool,
    pub proxy_url: Option<String>,
    pub proxy_applied: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserNavigatedPayload {
    panel_id: String,
    url: String,
}

#[tauri::command]
pub async fn browser_attach_webview(
    app: AppHandle,
    window: Window,
    state: State<'_, BrowserWebviewState>,
    panel_id: String,
    url: Option<String>,
    bounds: BrowserWebviewBounds,
    visible: Option<bool>,
    zoom: Option<f64>,
    proxy_url: Option<String>,
) -> Result<BrowserWebviewReply, String> {
    upsert_browser_webview(
        &app,
        &window,
        &state,
        panel_id,
        url,
        bounds,
        visible.unwrap_or(true),
        zoom,
        proxy_url,
        None,
    )
}

pub(crate) fn browser_attach_webview_for_control(
    app: &AppHandle,
    state: &BrowserWebviewState,
    window_id: &str,
    panel_id: &str,
    url: Option<&str>,
    proxy_url: Option<&str>,
    visible: bool,
) -> Result<BrowserWebviewReply, String> {
    let window = app
        .get_window(window_id)
        .ok_or_else(|| format!("{window_id} window is not available for browser WebView attach"))?;
    upsert_browser_webview(
        app,
        &window,
        state,
        panel_id.to_string(),
        url.map(str::to_string),
        BrowserWebviewBounds {
            x: 0.0,
            y: 0.0,
            width: 1024.0,
            height: 768.0,
        },
        visible,
        None,
        proxy_url.map(str::to_string),
        None,
    )
}

#[tauri::command]
pub async fn browser_update_webview(
    app: AppHandle,
    window: Window,
    state: State<'_, BrowserWebviewState>,
    panel_id: String,
    url: Option<String>,
    bounds: BrowserWebviewBounds,
    visible: Option<bool>,
    zoom: Option<f64>,
    proxy_url: Option<String>,
) -> Result<BrowserWebviewReply, String> {
    upsert_browser_webview(
        &app,
        &window,
        &state,
        panel_id,
        url,
        bounds,
        visible.unwrap_or(true),
        zoom,
        proxy_url,
        None,
    )
}

#[tauri::command]
pub fn browser_close_webview(
    state: State<'_, BrowserWebviewState>,
    panel_id: String,
) -> Result<(), String> {
    let panel_ids = BTreeSet::from([panel_id]);
    let lease = detach_browser_panels_for_control(state.inner(), &panel_ids)?;
    finalize_or_restore_browser_panels(state.inner(), lease)
}

#[cfg(test)]
pub(crate) fn strict_browser_runtime_teardown_transaction<T>(
    prepare: impl FnOnce() -> Result<T, String>,
    close: impl FnOnce(&T) -> Result<(), String>,
    commit: impl FnOnce(T) -> Result<(), String>,
) -> Result<(), String> {
    let prepared = prepare()?;
    close(&prepared)?;
    commit(prepared)
}

pub(crate) fn browser_close_webview_strict_for_control(
    state: &BrowserWebviewState,
    panel_id: &str,
) -> Result<(), String> {
    let panel_ids = BTreeSet::from([panel_id.to_string()]);
    let lease = detach_browser_panels_for_control(state, &panel_ids)?;
    if lease
        .entries
        .get(panel_id)
        .is_none_or(|entry| entry.child.is_none())
    {
        rollback_browser_panels_for_control(state, lease).map_err(|rollback| rollback.message)?;
        return Err(format!("browser runtime {panel_id} is unavailable"));
    }
    finalize_or_restore_browser_panels(state, lease)
}

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

#[tauri::command]
pub fn browser_webview_command(
    state: State<'_, BrowserWebviewState>,
    panel_id: String,
    command: String,
) -> Result<(), String> {
    browser_webview_command_for_control(state.inner(), &panel_id, &command).map(|_| ())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWebviewCommandReply {
    pub panel_id: String,
    pub command: String,
    pub attached: bool,
}

pub(crate) fn browser_webview_command_for_control(
    state: &BrowserWebviewState,
    panel_id: &str,
    command: &str,
) -> Result<BrowserWebviewCommandReply, String> {
    let webview = {
        let reserved_panel_ids = state
            .reserved_panel_ids
            .lock()
            .map_err(|_| "browser reservation state lock poisoned".to_string())?;
        if reserved_panel_ids.contains(panel_id) {
            return Err(format!("browser panel {panel_id} is reserved"));
        }
        let guard = state
            .webviews
            .lock()
            .map_err(|_| "browser webview state lock poisoned".to_string())?;
        let Some(child) = guard.get(panel_id) else {
            return Ok(BrowserWebviewCommandReply {
                panel_id: panel_id.to_string(),
                command: command.to_string(),
                attached: false,
            });
        };
        child.webview.clone()
    };
    match command {
        "reload" => webview.reload().map_err(|error| error.to_string()),
        "focus" => webview.set_focus().map_err(|error| error.to_string()),
        "openDevtools" => {
            webview.open_devtools();
            Ok(())
        }
        "closeDevtools" => {
            webview.close_devtools();
            Ok(())
        }
        _ => Ok(()),
    }?;
    Ok(BrowserWebviewCommandReply {
        panel_id: panel_id.to_string(),
        command: command.to_string(),
        attached: true,
    })
}

pub(crate) fn browser_eval_for_control(
    state: &BrowserWebviewState,
    panel_id: &str,
    script: &str,
) -> Result<Value, String> {
    let webview = {
        let reserved_panel_ids = state
            .reserved_panel_ids
            .lock()
            .map_err(|_| "browser reservation state lock poisoned".to_string())?;
        if reserved_panel_ids.contains(panel_id) {
            return Err(format!("browser panel {panel_id} is reserved"));
        }
        let guard = state
            .webviews
            .lock()
            .map_err(|_| "browser webview state lock poisoned".to_string())?;
        let Some(child) = guard.get(panel_id) else {
            return Err(format!(
                "browser WebView is not attached for surface {panel_id}"
            ));
        };
        child.webview.clone()
    };

    let (tx, rx) = mpsc::channel();
    webview
        .eval_with_callback(script.to_string(), move |value| {
            let _ = tx.send(value);
        })
        .map_err(|error| error.to_string())?;

    let raw = rx.recv_timeout(BROWSER_EVAL_TIMEOUT).map_err(|_| {
        format!(
            "browser eval timed out after {}ms",
            BROWSER_EVAL_TIMEOUT.as_millis()
        )
    })?;
    parse_webview_eval_callback_value(&raw)
}

pub(crate) fn browser_add_init_script_for_control(
    app: &AppHandle,
    state: &BrowserWebviewState,
    panel_id: &str,
    script: &str,
) -> Result<BrowserWebviewReply, String> {
    let mut reserved_panel_ids = state
        .reserved_panel_ids
        .lock()
        .map_err(|_| "browser reservation state lock poisoned".to_string())?;
    if reserved_panel_ids.contains(panel_id) {
        return Err(format!("browser panel {panel_id} is reserved"));
    }
    reserved_panel_ids.insert(panel_id.to_string());
    drop(reserved_panel_ids);
    let mut mutation_reservation = BrowserPanelMutationReservation {
        state,
        panel_id: panel_id.to_string(),
        active: true,
    };
    let (old_child, prior_scripts) = {
        let mut webviews = state
            .webviews
            .lock()
            .map_err(|_| "browser webview state lock poisoned".to_string())?;
        let mut init_scripts = state
            .init_scripts
            .lock()
            .map_err(|_| "browser init script state lock poisoned".to_string())?;
        let old_child = webviews
            .remove(panel_id)
            .ok_or_else(|| format!("browser WebView is not attached for surface {panel_id}"))?;
        let prior_scripts = init_scripts.get(panel_id).cloned();
        init_scripts
            .entry(panel_id.to_string())
            .or_default()
            .push(script.to_string());
        (old_child, prior_scripts)
    };
    let url = old_child.url.clone();
    let proxy_url = old_child.proxy_url.clone();
    let bounds = old_child.bounds.clone();
    let visible = old_child.visible;
    let zoom = old_child.zoom;
    if let Err(error) = old_child.webview.close().map_err(|error| error.to_string()) {
        let mut webviews = state
            .webviews
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut init_scripts = state
            .init_scripts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        webviews.insert(panel_id.to_string(), old_child);
        restore_browser_init_scripts(&mut init_scripts, panel_id, prior_scripts);
        drop(init_scripts);
        drop(webviews);
        mutation_reservation.release()?;
        return Err(error);
    }

    let rebuilt = app
        .get_window("main")
        .ok_or_else(|| "main window is not available for browser WebView attach".to_string())
        .and_then(|window| {
            upsert_browser_webview(
                app,
                &window,
                state,
                panel_id.to_string(),
                Some(url.clone()),
                bounds.clone(),
                visible,
                zoom,
                proxy_url.clone(),
                Some(&mut mutation_reservation),
            )
        });
    match rebuilt {
        Ok(reply) => {
            mutation_reservation.release()?;
            record_observed_navigation(state, panel_id, &url)?;
            Ok(reply)
        }
        Err(primary) => {
            {
                let mut init_scripts = state
                    .init_scripts
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                restore_browser_init_scripts(&mut init_scripts, panel_id, prior_scripts);
            }
            let restored = app
                .get_window("main")
                .ok_or_else(|| {
                    "main window is not available for browser WebView restore".to_string()
                })
                .and_then(|window| {
                    upsert_browser_webview(
                        app,
                        &window,
                        state,
                        panel_id.to_string(),
                        Some(url),
                        bounds,
                        visible,
                        zoom,
                        proxy_url,
                        Some(&mut mutation_reservation),
                    )
                });
            mutation_reservation.release()?;
            match restored {
                Ok(_) => Err(primary),
                Err(restore) => Err(format!(
                    "{primary}; failed to restore browser runtime: {restore}"
                )),
            }
        }
    }
}

fn restore_browser_init_scripts(
    init_scripts: &mut HashMap<String, Vec<String>>,
    panel_id: &str,
    prior_scripts: Option<Vec<String>>,
) {
    match prior_scripts {
        Some(scripts) => {
            init_scripts.insert(panel_id.to_string(), scripts);
        }
        None => {
            init_scripts.remove(panel_id);
        }
    }
}

fn parse_webview_eval_callback_value(raw: &str) -> Result<Value, String> {
    let parsed: Value = serde_json::from_str(raw)
        .map_err(|error| format!("browser eval returned invalid JSON ({error}): {raw}"))?;
    if let Some(inner) = parsed.as_str() {
        if let Ok(inner_json) = serde_json::from_str::<Value>(inner) {
            return Ok(inner_json);
        }
    }
    Ok(parsed)
}

#[tauri::command]
pub fn browser_network_requests(
    state: State<'_, BrowserWebviewState>,
    panel_id: String,
    url_contains: Option<String>,
    method: Option<String>,
    since_id: Option<String>,
    limit: Option<usize>,
) -> Result<BrowserNetworkRequestsReply, String> {
    browser_network_requests_for_control(
        state.inner(),
        &panel_id,
        BrowserNetworkRequestsQuery {
            url_contains,
            method,
            since_id,
            limit: limit.map(|value| value.min(200)),
        },
    )
}

#[tauri::command]
pub fn browser_clear_network_requests(
    state: State<'_, BrowserWebviewState>,
    panel_id: String,
) -> Result<BrowserNetworkClearReply, String> {
    browser_clear_network_requests_for_control(state.inner(), &panel_id)
}

fn upsert_browser_webview<'a>(
    app: &AppHandle,
    window: &Window,
    state: &'a BrowserWebviewState,
    panel_id: String,
    url: Option<String>,
    bounds: BrowserWebviewBounds,
    visible: bool,
    zoom: Option<f64>,
    proxy_url: Option<String>,
    mutation_reservation: Option<&mut BrowserPanelMutationReservation<'a>>,
) -> Result<BrowserWebviewReply, String> {
    let normalized_url = normalize_child_url(url.as_deref())?;
    let webview_url = parse_child_url(&normalized_url)?;
    let normalized_proxy_url = normalize_proxy_url(proxy_url.as_deref())?;
    let bounds = normalize_bounds(bounds);
    let label = browser_webview_label(&panel_id);

    let owns_mutation_reservation = mutation_reservation.is_none();
    let mut owned_mutation_reservation = if owns_mutation_reservation {
        let mut reserved_panel_ids = state
            .reserved_panel_ids
            .lock()
            .map_err(|_| "browser reservation state lock poisoned".to_string())?;
        if reserved_panel_ids.contains(&panel_id) {
            return Err(format!("browser panel {panel_id} is reserved"));
        }
        reserved_panel_ids.insert(panel_id.clone());
        drop(reserved_panel_ids);
        Some(BrowserPanelMutationReservation {
            state,
            panel_id: panel_id.clone(),
            active: true,
        })
    } else {
        None
    };
    let mutation_reservation = match mutation_reservation {
        Some(reservation) => {
            let reserved_panel_ids = state
                .reserved_panel_ids
                .lock()
                .map_err(|_| "browser reservation state lock poisoned".to_string())?;
            if !reservation.active
                || !std::ptr::eq(reservation.state, state)
                || reservation.panel_id != panel_id
                || !reserved_panel_ids.contains(&panel_id)
            {
                return Err(format!(
                    "browser panel {panel_id} mutation ownership is unavailable"
                ));
            }
            drop(reserved_panel_ids);
            reservation
        }
        None => owned_mutation_reservation
            .as_mut()
            .expect("owned browser mutation reservation"),
    };
    let existing = {
        let guard = state
            .webviews
            .lock()
            .map_err(|_| "browser webview state lock poisoned".to_string())?;
        guard.get(&panel_id).map(|child| {
            (
                child.webview.clone(),
                child.label.clone(),
                child.url.clone(),
                child.proxy_url.clone(),
            )
        })
    };
    let init_scripts = state
        .init_scripts
        .lock()
        .map_err(|_| "browser init script state lock poisoned".to_string())?
        .get(&panel_id)
        .cloned()
        .unwrap_or_default();
    if let Some((webview, child_label, child_url, child_proxy_url)) = existing {
        if child_proxy_url != normalized_proxy_url {
            let mut webviews = state
                .webviews
                .lock()
                .map_err(|_| "browser webview state lock poisoned".to_string())?;
            webviews.remove(&panel_id);
            drop(webviews);
            webview.close().map_err(|error| error.to_string())?;
        } else {
            let mut navigated = false;
            webview
                .set_position(LogicalPosition::new(bounds.x, bounds.y))
                .map_err(|error| error.to_string())?;
            webview
                .set_size(LogicalSize::new(bounds.width, bounds.height))
                .map_err(|error| error.to_string())?;
            apply_visibility(&webview, visible)?;
            if child_url != normalized_url {
                webview
                    .navigate(webview_url)
                    .map_err(|error| error.to_string())?;
                navigated = true;
                if let Some(child) = state
                    .webviews
                    .lock()
                    .map_err(|_| "browser webview state lock poisoned".to_string())?
                    .get_mut(&panel_id)
                {
                    child.url = normalized_url.clone();
                }
            }
            if let Some(zoom) = zoom {
                webview
                    .set_zoom(normalize_zoom(zoom))
                    .map_err(|error| error.to_string())?;
            }
            if let Some(child) = state
                .webviews
                .lock()
                .map_err(|_| "browser webview state lock poisoned".to_string())?
                .get_mut(&panel_id)
            {
                child.bounds = bounds.clone();
                child.visible = visible;
                child.zoom = zoom.or(child.zoom);
            }
            if owns_mutation_reservation {
                mutation_reservation.release()?;
            }
            if navigated && owns_mutation_reservation {
                record_observed_navigation(state, &panel_id, &normalized_url)?;
            }
            return Ok(BrowserWebviewReply {
                panel_id,
                label: child_label,
                url: normalized_url,
                attached: true,
                proxy_url: child_proxy_url.clone(),
                proxy_applied: child_proxy_url.is_some(),
            });
        }
    }

    let event_app = app.clone();
    let event_panel_id = panel_id.clone();
    let mut builder = WebviewBuilder::new(label.clone(), WebviewUrl::External(webview_url))
        .on_navigation(move |url| {
            let browser_state = event_app.state::<BrowserWebviewState>();
            let _ =
                record_observed_navigation(browser_state.inner(), &event_panel_id, url.as_str());
            let _ = event_app.emit(
                BROWSER_NAVIGATED_EVENT,
                BrowserNavigatedPayload {
                    panel_id: event_panel_id.clone(),
                    url: url.to_string(),
                },
            );
            true
        });
    for script in &init_scripts {
        builder = builder.initialization_script(script.clone());
    }
    if let Some(proxy_url) = normalized_proxy_url.as_deref() {
        let parsed_proxy_url = parse_proxy_url(proxy_url)?;
        let browser_args = webview2_proxy_browser_args(&parsed_proxy_url)?;
        builder = builder
            .proxy_url(parsed_proxy_url)
            .additional_browser_args(&browser_args)
            .data_directory(browser_proxy_data_directory(app, &panel_id, proxy_url)?);
    }
    let webview = window
        .add_child(
            builder,
            LogicalPosition::new(bounds.x, bounds.y),
            LogicalSize::new(bounds.width, bounds.height),
        )
        .map_err(|error| error.to_string())?;
    if let Err(error) = apply_visibility(&webview, visible) {
        let _ = webview.close();
        return Err(error);
    }
    if let Some(zoom) = zoom {
        if let Err(error) = webview
            .set_zoom(normalize_zoom(zoom))
            .map_err(|error| error.to_string())
        {
            let _ = webview.close();
            return Err(error);
        }
    }
    let mut reserved_panel_ids = match state.reserved_panel_ids.lock() {
        Ok(reserved_panel_ids) => reserved_panel_ids,
        Err(_) => {
            let _ = webview.close();
            return Err("browser reservation state lock poisoned".to_string());
        }
    };
    if !reserved_panel_ids.contains(&panel_id) {
        drop(reserved_panel_ids);
        webview.close().map_err(|error| error.to_string())?;
        return Err(format!("browser panel {panel_id} reservation was lost"));
    }
    let mut webviews = match state.webviews.lock() {
        Ok(webviews) => webviews,
        Err(_) => {
            drop(reserved_panel_ids);
            let _ = webview.close();
            return Err("browser webview state lock poisoned".to_string());
        }
    };
    webviews.insert(
        panel_id.clone(),
        BrowserChild {
            webview,
            label: label.clone(),
            url: normalized_url.clone(),
            proxy_url: normalized_proxy_url.clone(),
            bounds: bounds.clone(),
            visible,
            zoom,
        },
    );
    if owns_mutation_reservation {
        reserved_panel_ids.remove(&panel_id);
        mutation_reservation.disarm();
    }
    drop(webviews);
    drop(reserved_panel_ids);
    if owns_mutation_reservation {
        record_observed_navigation(state, &panel_id, &normalized_url)?;
    }

    Ok(BrowserWebviewReply {
        panel_id,
        label,
        url: normalized_url,
        attached: true,
        proxy_url: normalized_proxy_url.clone(),
        proxy_applied: normalized_proxy_url.is_some(),
    })
}

pub(crate) fn browser_network_requests_for_control(
    state: &BrowserWebviewState,
    panel_id: &str,
    query: BrowserNetworkRequestsQuery,
) -> Result<BrowserNetworkRequestsReply, String> {
    let reserved_panel_ids = state
        .reserved_panel_ids
        .lock()
        .map_err(|_| "browser reservation state lock poisoned".to_string())?;
    if reserved_panel_ids.contains(panel_id) {
        return Err(format!("browser panel {panel_id} is reserved"));
    }
    let all_requests = state
        .network_records
        .lock()
        .map_err(|_| "browser network record state lock poisoned".to_string())?
        .get(panel_id)
        .cloned()
        .unwrap_or_default();
    let total_count = all_requests.len();
    let observer = network_observer_summary(&all_requests);
    let requests = filter_network_records(all_requests, &query);
    let filtered_count = requests.len();
    let requests = apply_network_record_limit(requests, query.limit);
    let returned_count = requests.len();
    Ok(BrowserNetworkRequestsReply {
        panel_id: panel_id.to_string(),
        requests,
        total_count,
        returned_count,
        filtered_count,
        observer,
    })
}

pub(crate) fn browser_clear_network_requests_for_control(
    state: &BrowserWebviewState,
    panel_id: &str,
) -> Result<BrowserNetworkClearReply, String> {
    let reserved_panel_ids = state
        .reserved_panel_ids
        .lock()
        .map_err(|_| "browser reservation state lock poisoned".to_string())?;
    if reserved_panel_ids.contains(panel_id) {
        return Err(format!("browser panel {panel_id} is reserved"));
    }
    let mut guard = state
        .network_records
        .lock()
        .map_err(|_| "browser network record state lock poisoned".to_string())?;
    let cleared_count = guard.remove(panel_id).map_or(0, |records| records.len());
    Ok(BrowserNetworkClearReply {
        panel_id: panel_id.to_string(),
        cleared_count,
    })
}

fn network_observer_summary(records: &[BrowserNetworkRecord]) -> BrowserNetworkObserverSummary {
    let mut sources = records
        .iter()
        .map(|record| record.source.as_str())
        .collect::<Vec<_>>();
    sources.sort_unstable();
    sources.dedup();
    let source = match sources.as_slice() {
        [] => "wkwebview-navigation".to_string(),
        [single] => (*single).to_string(),
        _ => "mixed".to_string(),
    };
    let has_rich_records = records.iter().any(|record| {
        matches!(
            record.source.as_str(),
            "tauri-uri-scheme" | "proxy-stream-http"
        )
    });
    let has_proxy_tunnel_records = records
        .iter()
        .any(|record| record.source.as_str() == "proxy-stream-tunnel");
    let mut proxy_attributions = records
        .iter()
        .filter_map(|record| record.proxy_attribution.as_deref())
        .collect::<Vec<_>>();
    proxy_attributions.sort_unstable();
    proxy_attributions.dedup();
    let proxy_attribution_mode = match proxy_attributions.as_slice() {
        [] => "none".to_string(),
        [single] => (*single).to_string(),
        _ => "mixed".to_string(),
    };
    BrowserNetworkObserverSummary {
        source,
        captures_url: true,
        captures_method: true,
        captures_request_headers: has_rich_records,
        captures_request_body: records
            .iter()
            .any(|record| record.request_body.is_some() || record.request_body_size > 0),
        captures_response_status: records
            .iter()
            .any(|record| record.response_status.is_some()),
        captures_response_headers: records
            .iter()
            .any(|record| !record.response_headers.is_empty()),
        captures_response_body: records
            .iter()
            .any(|record| record.response_body.is_some() || record.response_body_size > 0),
        captures_timing: true,
        supports_filters: true,
        max_records_per_panel: NETWORK_RECORD_LIMIT_PER_PANEL,
        body_capture_limit_bytes: NETWORK_BODY_CAPTURE_LIMIT_BYTES,
        proxy_attribution_mode,
        note: if has_rich_records && has_proxy_tunnel_records {
            "Mixed records include rich cleartext request/response metadata where available plus opaque proxy tunnel observations for encrypted traffic.".to_string()
        } else if has_rich_records {
            "Rich records include request headers/body metadata, response status/headers, body size, and timing for cmux custom schemes or cleartext proxy HTTP streams; TLS proxy payloads remain opaque without interception.".to_string()
        } else if has_proxy_tunnel_records {
            "Opaque proxy tunnel records capture URL authority, tunnel method/status when available, bounded byte previews, attribution, and timing; encrypted request/response headers remain unavailable without interception.".to_string()
        } else {
            "WKWebView exposes navigation callbacks but not full HTTP request/response metadata; proxy-level observers can replace these best-effort records when available.".to_string()
        },
    }
}

fn filter_network_records(
    requests: Vec<BrowserNetworkRecord>,
    query: &BrowserNetworkRequestsQuery,
) -> Vec<BrowserNetworkRecord> {
    let after_since = match query.since_id.as_deref() {
        Some(since_id) => requests
            .iter()
            .position(|record| record.id == since_id)
            .map(|index| index + 1)
            .unwrap_or(0),
        None => 0,
    };
    let method = query
        .method
        .as_ref()
        .map(|value| value.to_ascii_uppercase());
    requests
        .into_iter()
        .skip(after_since)
        .filter(|record| {
            query
                .url_contains
                .as_ref()
                .is_none_or(|needle| record.url.contains(needle))
        })
        .filter(|record| {
            method
                .as_ref()
                .is_none_or(|expected| record.method.eq_ignore_ascii_case(expected))
        })
        .collect()
}

fn apply_network_record_limit(
    requests: Vec<BrowserNetworkRecord>,
    limit: Option<usize>,
) -> Vec<BrowserNetworkRecord> {
    let Some(limit) = limit else {
        return requests;
    };
    if requests.len() <= limit {
        return requests;
    }
    requests[requests.len() - limit..].to_vec()
}

fn record_observed_navigation(
    state: &BrowserWebviewState,
    panel_id: &str,
    url: &str,
) -> Result<(), String> {
    let timestamp = now_ms();
    let id = state.next_network_record_id.fetch_add(1, Ordering::Relaxed);
    push_network_record(
        state,
        BrowserNetworkRecord {
            id: format!("browser-network-{id}"),
            panel_id: panel_id.to_string(),
            url: url.to_string(),
            method: "GET".to_string(),
            request_headers: BTreeMap::new(),
            request_body: None,
            request_body_preview_kind: "unavailable".to_string(),
            request_body_size: 0,
            request_body_truncated: false,
            response_status: None,
            response_headers: BTreeMap::new(),
            response_body: None,
            response_body_preview_kind: "unavailable".to_string(),
            response_body_size: 0,
            response_body_truncated: false,
            started_at_ms: timestamp,
            completed_at_ms: Some(timestamp),
            duration_ms: Some(0),
            source: "wkwebview-navigation".to_string(),
            transport: transport_for_url(url),
            proxy_attribution: None,
            note: Some("Navigation observed by WKWebView/Tauri; headers, body, and status require proxy-level capture.".to_string()),
        },
    )
}

pub(crate) fn record_custom_scheme_network_request(
    state: &BrowserWebviewState,
    panel_id: &str,
    request: &tauri::http::Request<Vec<u8>>,
    response: &tauri::http::Response<Vec<u8>>,
    started_at_ms: u64,
    completed_at_ms: u64,
) -> Result<(), String> {
    let id = state.next_network_record_id.fetch_add(1, Ordering::Relaxed);
    let request_body = network_body_text(request.body());
    let response_body = network_body_text(response.body());
    push_network_record(
        state,
        BrowserNetworkRecord {
            id: format!("browser-network-{id}"),
            panel_id: panel_id.to_string(),
            url: request.uri().to_string(),
            method: request.method().as_str().to_string(),
            request_headers: network_header_map(request.headers()),
            request_body: request_body.body,
            request_body_preview_kind: request_body.preview_kind,
            request_body_size: request.body().len(),
            request_body_truncated: request_body.truncated,
            response_status: Some(response.status().as_u16()),
            response_headers: network_header_map(response.headers()),
            response_body: response_body.body,
            response_body_preview_kind: response_body.preview_kind,
            response_body_size: response.body().len(),
            response_body_truncated: response_body.truncated,
            started_at_ms,
            completed_at_ms: Some(completed_at_ms),
            duration_ms: Some(completed_at_ms.saturating_sub(started_at_ms)),
            source: "tauri-uri-scheme".to_string(),
            transport: transport_for_url(&request.uri().to_string()),
            proxy_attribution: None,
            note: Some("cmux custom URI-scheme request served by Tauri; external HTTP request/response metadata still requires proxy-level capture.".to_string()),
        },
    )
}

#[allow(dead_code)]
pub(crate) fn record_proxy_http_exchange_observation(
    state: &BrowserWebviewState,
    panel_id: &str,
    tunnel_protocol: &str,
    target_host: &str,
    target_port: u16,
    request_bytes: &[u8],
    response_bytes: &[u8],
    started_at_ms: u64,
    completed_at_ms: u64,
) -> Result<(), String> {
    record_proxy_http_exchange_observation_with_attribution(
        state,
        panel_id,
        tunnel_protocol,
        target_host,
        target_port,
        request_bytes,
        response_bytes,
        started_at_ms,
        completed_at_ms,
        None,
    )
}

#[allow(dead_code)]
pub(crate) fn record_proxy_http_exchange_observation_with_attribution(
    state: &BrowserWebviewState,
    panel_id: &str,
    tunnel_protocol: &str,
    target_host: &str,
    target_port: u16,
    request_bytes: &[u8],
    response_bytes: &[u8],
    started_at_ms: u64,
    completed_at_ms: u64,
    proxy_attribution: Option<&str>,
) -> Result<(), String> {
    let parsed_request = parse_proxy_http_request_head(request_bytes, target_host, target_port)?;
    let parsed_response = parse_proxy_http_response_head(response_bytes)?;
    let request_body = network_body_text(parsed_request.body_prefix);
    let response_body = network_body_text(parsed_response.body_prefix);
    let id = state.next_network_record_id.fetch_add(1, Ordering::Relaxed);
    push_network_record(
        state,
        BrowserNetworkRecord {
            id: format!("browser-network-{id}"),
            panel_id: panel_id.to_string(),
            url: parsed_request.url,
            method: parsed_request.method,
            request_headers: parsed_request.headers,
            request_body: request_body.body,
            request_body_preview_kind: request_body.preview_kind,
            request_body_size: parsed_request.body_prefix.len(),
            request_body_truncated: request_body.truncated,
            response_status: Some(parsed_response.status),
            response_headers: parsed_response.headers,
            response_body: response_body.body,
            response_body_preview_kind: response_body.preview_kind,
            response_body_size: parsed_response.body_prefix.len(),
            response_body_truncated: response_body.truncated,
            started_at_ms,
            completed_at_ms: Some(completed_at_ms),
            duration_ms: Some(completed_at_ms.saturating_sub(started_at_ms)),
            source: "proxy-stream-http".to_string(),
            transport: tunnel_protocol.to_string(),
            proxy_attribution: proxy_attribution.map(str::to_string),
            note: Some("HTTP request/response metadata parsed from cleartext proxy stream bytes; TLS payloads remain opaque without MITM interception.".to_string()),
        },
    )
}

#[allow(dead_code)]
pub(crate) fn record_proxy_tunnel_observation_with_attribution(
    state: &BrowserWebviewState,
    panel_id: &str,
    tunnel_protocol: &str,
    target_host: &str,
    target_port: u16,
    upstream_prefix: &[u8],
    upstream_truncated: bool,
    downstream_prefix: &[u8],
    downstream_truncated: bool,
    started_at_ms: u64,
    completed_at_ms: u64,
    proxy_attribution: Option<&str>,
) -> Result<(), String> {
    let request_body = network_opaque_tunnel_body(upstream_prefix);
    let response_body = network_opaque_tunnel_body(downstream_prefix);
    let id = state.next_network_record_id.fetch_add(1, Ordering::Relaxed);
    push_network_record(
        state,
        BrowserNetworkRecord {
            id: format!("browser-network-{id}"),
            panel_id: panel_id.to_string(),
            url: proxy_tunnel_url(target_host, target_port),
            method: "CONNECT".to_string(),
            request_headers: BTreeMap::new(),
            request_body: request_body.body,
            request_body_preview_kind: request_body.preview_kind,
            request_body_size: upstream_prefix.len(),
            request_body_truncated: request_body.truncated || upstream_truncated,
            response_status: Some(200),
            response_headers: BTreeMap::new(),
            response_body: response_body.body,
            response_body_preview_kind: response_body.preview_kind,
            response_body_size: downstream_prefix.len(),
            response_body_truncated: response_body.truncated || downstream_truncated,
            started_at_ms,
            completed_at_ms: Some(completed_at_ms),
            duration_ms: Some(completed_at_ms.saturating_sub(started_at_ms)),
            source: "proxy-stream-tunnel".to_string(),
            transport: tunnel_protocol.to_string(),
            proxy_attribution: proxy_attribution.map(str::to_string),
            note: Some("Opaque proxy tunnel observed after broker handshake; status 200 represents successful tunnel establishment, while encrypted origin request/response headers remain unavailable without interception.".to_string()),
        },
    )
}

#[allow(dead_code)]
struct ParsedProxyHttpRequest<'a> {
    method: String,
    url: String,
    headers: BTreeMap<String, String>,
    body_prefix: &'a [u8],
}

#[allow(dead_code)]
struct ParsedProxyHttpResponse<'a> {
    status: u16,
    headers: BTreeMap<String, String>,
    body_prefix: &'a [u8],
}

#[allow(dead_code)]
fn parse_proxy_http_request_head<'a>(
    bytes: &'a [u8],
    target_host: &str,
    target_port: u16,
) -> Result<ParsedProxyHttpRequest<'a>, String> {
    let (head, body_prefix) = split_http_head(bytes)?;
    let text = std::str::from_utf8(head)
        .map_err(|_| "proxy HTTP request head is not valid UTF-8".to_string())?;
    let mut lines = text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "proxy HTTP request head is empty".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "proxy HTTP request line is missing method".to_string())?;
    let target = parts
        .next()
        .ok_or_else(|| "proxy HTTP request line is missing target".to_string())?;
    let headers = parse_http_headers(lines)?;
    let host = headers
        .get("host")
        .map(String::as_str)
        .unwrap_or(target_host);
    let url = proxy_request_url(target, host, target_port);
    Ok(ParsedProxyHttpRequest {
        method: method.to_ascii_uppercase(),
        url,
        headers,
        body_prefix,
    })
}

#[allow(dead_code)]
fn parse_proxy_http_response_head(bytes: &[u8]) -> Result<ParsedProxyHttpResponse<'_>, String> {
    let (head, body_prefix) = split_http_head(bytes)?;
    let text = std::str::from_utf8(head)
        .map_err(|_| "proxy HTTP response head is not valid UTF-8".to_string())?;
    let mut lines = text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| "proxy HTTP response head is empty".to_string())?;
    let mut parts = status_line.split_whitespace();
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/") {
        return Err("proxy HTTP response line is missing HTTP version".to_string());
    }
    let status = parts
        .next()
        .ok_or_else(|| "proxy HTTP response line is missing status".to_string())?
        .parse::<u16>()
        .map_err(|_| "proxy HTTP response status is not numeric".to_string())?;
    Ok(ParsedProxyHttpResponse {
        status,
        headers: parse_http_headers(lines)?,
        body_prefix,
    })
}

#[allow(dead_code)]
fn split_http_head(bytes: &[u8]) -> Result<(&[u8], &[u8]), String> {
    if bytes.len() > NETWORK_HTTP_HEAD_CAPTURE_LIMIT_BYTES {
        return Err(format!(
            "proxy HTTP head exceeded {} bytes",
            NETWORK_HTTP_HEAD_CAPTURE_LIMIT_BYTES
        ));
    }
    let marker = b"\r\n\r\n";
    let Some(index) = bytes
        .windows(marker.len())
        .position(|window| window == marker)
    else {
        return Err("proxy HTTP head is incomplete".to_string());
    };
    Ok((&bytes[..index], &bytes[index + marker.len()..]))
}

#[allow(dead_code)]
fn parse_http_headers<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<String, String>, String> {
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(format!("proxy HTTP header is malformed: {line}"));
        };
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            return Err("proxy HTTP header name is empty".to_string());
        }
        headers.insert(name, value.trim().to_string());
    }
    Ok(headers)
}

#[allow(dead_code)]
fn proxy_request_url(target: &str, host: &str, target_port: u16) -> String {
    if Url::parse(target).is_ok() {
        return target.to_string();
    }
    if target == "*" {
        return format!("http://{}", host_with_default_port(host, target_port));
    }
    let authority = host_with_default_port(host, target_port);
    if target.starts_with('/') {
        format!("http://{authority}{target}")
    } else {
        format!("http://{authority}/{target}")
    }
}

#[allow(dead_code)]
fn proxy_tunnel_url(target_host: &str, target_port: u16) -> String {
    let host = bracket_ipv6_host(target_host.trim());
    if target_port == 443 {
        format!("https://{host}/")
    } else {
        format!("tcp://{host}:{target_port}/")
    }
}

#[allow(dead_code)]
fn bracket_ipv6_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

#[allow(dead_code)]
fn host_with_default_port(host: &str, target_port: u16) -> String {
    let trimmed = host.trim();
    if trimmed.contains(':') || target_port == 80 {
        trimmed.to_string()
    } else {
        format!("{trimmed}:{target_port}")
    }
}

struct NetworkBodyText {
    body: Option<String>,
    preview_kind: String,
    truncated: bool,
}

impl NetworkBodyText {
    fn empty() -> Self {
        Self {
            body: None,
            preview_kind: "empty".to_string(),
            truncated: false,
        }
    }

    fn binary(body: &[u8]) -> Self {
        Self {
            body: Some(format!("<binary body: {} bytes>", body.len())),
            preview_kind: "binary".to_string(),
            truncated: body.len() > NETWORK_BODY_CAPTURE_LIMIT_BYTES,
        }
    }
}

fn network_opaque_tunnel_body(body: &[u8]) -> NetworkBodyText {
    if body.is_empty() {
        NetworkBodyText::empty()
    } else {
        NetworkBodyText::binary(body)
    }
}

fn network_body_text(body: &[u8]) -> NetworkBodyText {
    if body.is_empty() {
        return NetworkBodyText::empty();
    }
    let truncated = body.len() > NETWORK_BODY_CAPTURE_LIMIT_BYTES;
    let captured = if truncated {
        &body[..NETWORK_BODY_CAPTURE_LIMIT_BYTES]
    } else {
        body
    };
    match String::from_utf8(captured.to_vec()) {
        Ok(text) => NetworkBodyText {
            body: Some(text),
            preview_kind: "text".to_string(),
            truncated,
        },
        Err(_) => NetworkBodyText::binary(body),
    }
}

fn network_header_map(headers: &tauri::http::HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value
                    .to_str()
                    .map(str::to_string)
                    .unwrap_or_else(|_| "<non-utf8 header value>".to_string()),
            )
        })
        .collect()
}

fn push_network_record(
    state: &BrowserWebviewState,
    record: BrowserNetworkRecord,
) -> Result<(), String> {
    let reserved_panel_ids = state
        .reserved_panel_ids
        .lock()
        .map_err(|_| "browser reservation state lock poisoned".to_string())?;
    if reserved_panel_ids.contains(&record.panel_id) {
        return Err(format!("browser panel {} is reserved", record.panel_id));
    }
    let mut guard = state
        .network_records
        .lock()
        .map_err(|_| "browser network record state lock poisoned".to_string())?;
    let records = guard.entry(record.panel_id.clone()).or_default();
    records.push(record);
    if records.len() > NETWORK_RECORD_LIMIT_PER_PANEL {
        let overflow = records.len() - NETWORK_RECORD_LIMIT_PER_PANEL;
        records.drain(0..overflow);
    }
    Ok(())
}

fn transport_for_url(url: &str) -> String {
    Url::parse(url)
        .ok()
        .map(|parsed| parsed.scheme().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn apply_visibility(webview: &tauri::Webview, visible: bool) -> Result<(), String> {
    if visible {
        webview.show().map_err(|error| error.to_string())
    } else {
        webview.hide().map_err(|error| error.to_string())
    }
}

fn normalize_child_url(url: Option<&str>) -> Result<String, String> {
    let trimmed = url.unwrap_or(DEFAULT_BROWSER_URL).trim();
    if trimmed.is_empty() {
        Ok(DEFAULT_BROWSER_URL.to_string())
    } else {
        parse_child_url(trimmed)?;
        Ok(trimmed.to_string())
    }
}

fn parse_child_url(raw: &str) -> Result<Url, String> {
    Url::parse(raw).map_err(|_| format!("invalid browser URL: {raw}"))
}

fn normalize_proxy_url(proxy_url: Option<&str>) -> Result<Option<String>, String> {
    let Some(proxy_url) = proxy_url else {
        return Ok(None);
    };
    let trimmed = proxy_url.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    parse_proxy_url(trimmed)?;
    Ok(Some(trimmed.to_string()))
}

fn browser_proxy_data_directory(
    app: &AppHandle,
    panel_id: &str,
    proxy_url: &str,
) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("could not resolve browser proxy data directory: {error}"))?;
    let key = format!("{panel_id}-{proxy_url}")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let path = base.join("browser-webviews").join(key);
    std::fs::create_dir_all(&path)
        .map_err(|error| format!("could not create browser proxy data directory: {error}"))?;
    Ok(path)
}

fn parse_proxy_url(raw: &str) -> Result<Url, String> {
    let parsed = Url::parse(raw).map_err(|_| format!("invalid browser proxyUrl: {raw}"))?;
    match parsed.scheme() {
        "http" | "socks5" => {}
        scheme => {
            return Err(format!(
                "browser proxyUrl must use http:// or socks5://, got {scheme}://"
            ));
        }
    }
    if parsed.host_str().is_none() {
        return Err("browser proxyUrl must include a host".to_string());
    }
    if parsed.port().is_none() {
        return Err("browser proxyUrl must include a port".to_string());
    }
    Ok(parsed)
}

fn webview2_proxy_browser_args(proxy_url: &Url) -> Result<String, String> {
    let scheme = proxy_url.scheme();
    let host = proxy_url
        .host_str()
        .ok_or_else(|| "browser proxyUrl must include a host".to_string())?;
    let port = proxy_url
        .port()
        .ok_or_else(|| "browser proxyUrl must include a port".to_string())?;
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    Ok(format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --proxy-server={scheme}://{host}:{port}"
    ))
}

fn normalize_bounds(bounds: BrowserWebviewBounds) -> BrowserWebviewBounds {
    BrowserWebviewBounds {
        x: bounds.x.max(0.0).round(),
        y: bounds.y.max(0.0).round(),
        width: bounds.width.max(1.0).round(),
        height: bounds.height.max(1.0).round(),
    }
}

fn normalize_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() {
        zoom.clamp(0.25, 3.0)
    } else {
        1.0
    }
}

fn browser_webview_label(panel_id: &str) -> String {
    let safe_panel_id: String = panel_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '/' | ':') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("browser:{safe_panel_id}")
}

pub(crate) fn panel_id_from_browser_webview_label(label: &str) -> Option<String> {
    label
        .strip_prefix("browser:")
        .filter(|panel_id| !panel_id.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_webview_label_sanitizes_for_tauri() {
        assert_eq!(
            browser_webview_label("pane 1!*"),
            "browser:pane_1__".to_string()
        );
    }

    #[test]
    fn normalize_child_url_defaults_and_validates() {
        assert_eq!(normalize_child_url(None).unwrap(), "about:blank");
        assert_eq!(normalize_child_url(Some("  ")).unwrap(), "about:blank");
        assert_eq!(
            normalize_child_url(Some("https://example.com/a")).unwrap(),
            "https://example.com/a"
        );
        assert!(normalize_child_url(Some("not a url")).is_err());
    }

    #[test]
    fn normalize_proxy_url_accepts_http_and_socks5_loopback_endpoints() {
        assert_eq!(normalize_proxy_url(None).unwrap(), None);
        assert_eq!(normalize_proxy_url(Some("  ")).unwrap(), None);
        assert_eq!(
            normalize_proxy_url(Some(" http://127.0.0.1:31337 ")).unwrap(),
            Some("http://127.0.0.1:31337".to_string())
        );
        assert_eq!(
            normalize_proxy_url(Some("socks5://localhost:1080")).unwrap(),
            Some("socks5://localhost:1080".to_string())
        );
    }

    #[test]
    fn normalize_proxy_url_rejects_unsupported_or_ambiguous_values() {
        assert!(normalize_proxy_url(Some("https://127.0.0.1:8443")).is_err());
        assert!(normalize_proxy_url(Some("http://127.0.0.1")).is_err());
        assert!(normalize_proxy_url(Some("socks5:///1080")).is_err());
        assert!(normalize_proxy_url(Some("not a proxy")).is_err());
    }

    #[test]
    fn normalize_bounds_and_zoom_are_browser_safe() {
        let bounds = normalize_bounds(BrowserWebviewBounds {
            x: -10.4,
            y: 12.6,
            width: 0.0,
            height: 44.2,
        });
        assert_eq!(bounds.x, 0.0);
        assert_eq!(bounds.y, 13.0);
        assert_eq!(bounds.width, 1.0);
        assert_eq!(bounds.height, 44.0);
        assert_eq!(normalize_zoom(f64::NAN), 1.0);
        assert_eq!(normalize_zoom(9.0), 3.0);
        assert_eq!(normalize_zoom(0.1), 0.25);
    }

    #[test]
    fn network_requests_records_navigation_metadata() {
        let state = BrowserWebviewState::default();
        record_observed_navigation(&state, "surface-1", "https://example.com/path").unwrap();

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert_eq!(reply.panel_id, "surface-1");
        assert_eq!(reply.requests.len(), 1);
        assert_eq!(reply.total_count, 1);
        assert_eq!(reply.returned_count, 1);
        assert_eq!(reply.filtered_count, 1);
        assert_eq!(reply.requests[0].url, "https://example.com/path");
        assert_eq!(reply.requests[0].method, "GET");
        assert_eq!(reply.requests[0].transport, "https");
        assert_eq!(reply.requests[0].request_body_size, 0);
        assert_eq!(reply.requests[0].request_body_preview_kind, "unavailable");
        assert_eq!(reply.requests[0].proxy_attribution, None);
        assert_eq!(reply.requests[0].response_body, None);
        assert_eq!(reply.requests[0].response_body_size, 0);
        assert_eq!(reply.requests[0].response_body_preview_kind, "unavailable");
        assert!(!reply.requests[0].response_body_truncated);
        assert_eq!(reply.requests[0].response_status, None);
        assert!(reply.observer.captures_url);
        assert!(!reply.observer.captures_response_status);
        assert!(reply.observer.supports_filters);
        assert_eq!(reply.observer.proxy_attribution_mode, "none");
        assert_eq!(
            reply.observer.max_records_per_panel,
            NETWORK_RECORD_LIMIT_PER_PANEL
        );
        assert_eq!(
            reply.observer.body_capture_limit_bytes,
            NETWORK_BODY_CAPTURE_LIMIT_BYTES
        );
    }

    #[test]
    fn network_requests_are_bounded_per_panel() {
        let state = BrowserWebviewState::default();
        for index in 0..(NETWORK_RECORD_LIMIT_PER_PANEL + 5) {
            record_observed_navigation(
                &state,
                "surface-1",
                &format!("https://example.com/{index}"),
            )
            .unwrap();
        }

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert_eq!(reply.requests.len(), NETWORK_RECORD_LIMIT_PER_PANEL);
        assert_eq!(
            reply.requests.first().map(|record| record.url.as_str()),
            Some("https://example.com/5")
        );
    }

    #[test]
    fn network_requests_can_be_cleared_per_panel() {
        let state = BrowserWebviewState::default();
        record_observed_navigation(&state, "surface-1", "https://example.com/one").unwrap();
        record_observed_navigation(&state, "surface-1", "https://example.com/two").unwrap();
        record_observed_navigation(&state, "surface-2", "https://example.com/other").unwrap();

        let cleared = browser_clear_network_requests_for_control(&state, "surface-1").unwrap();
        assert_eq!(cleared.panel_id, "surface-1");
        assert_eq!(cleared.cleared_count, 2);

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert!(reply.requests.is_empty());
        assert_eq!(reply.total_count, 0);

        let reply = browser_network_requests_for_control(
            &state,
            "surface-2",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert_eq!(reply.requests.len(), 1);

        let cleared = browser_clear_network_requests_for_control(&state, "surface-1").unwrap();
        assert_eq!(cleared.cleared_count, 0);
    }

    #[test]
    fn network_requests_support_since_url_method_and_limit_filters() {
        let state = BrowserWebviewState::default();
        record_observed_navigation(&state, "surface-1", "https://example.com/one").unwrap();
        record_observed_navigation(&state, "surface-1", "https://example.com/two").unwrap();
        record_observed_navigation(&state, "surface-1", "https://other.test/two").unwrap();

        let first_page = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery {
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            first_page
                .requests
                .iter()
                .map(|record| record.url.as_str())
                .collect::<Vec<_>>(),
            vec!["https://other.test/two"]
        );

        let since_id = first_page.requests[0].id.clone();
        let filtered = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery {
                url_contains: Some("example.com".to_string()),
                method: Some("get".to_string()),
                since_id: Some(since_id),
                limit: Some(5),
            },
        )
        .unwrap();
        assert_eq!(filtered.total_count, 3);
        assert_eq!(filtered.filtered_count, 0);
        assert_eq!(filtered.returned_count, 0);

        let filtered = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery {
                url_contains: Some("two".to_string()),
                method: Some("GET".to_string()),
                since_id: None,
                limit: Some(10),
            },
        )
        .unwrap();
        assert_eq!(
            filtered
                .requests
                .iter()
                .map(|record| record.url.as_str())
                .collect::<Vec<_>>(),
            vec!["https://example.com/two", "https://other.test/two"]
        );
        assert_eq!(filtered.filtered_count, 2);
        assert_eq!(filtered.returned_count, 2);
    }

    #[test]
    fn browser_webview_label_maps_back_to_panel_id() {
        assert_eq!(
            panel_id_from_browser_webview_label("browser:surface-1"),
            Some("surface-1".to_string())
        );
        assert_eq!(panel_id_from_browser_webview_label("main"), None);
        assert_eq!(panel_id_from_browser_webview_label("browser:"), None);
    }

    #[test]
    fn custom_scheme_network_requests_capture_headers_status_body_size_and_timing() {
        let state = BrowserWebviewState::default();
        let request = tauri::http::Request::builder()
            .method("POST")
            .uri("cmux-md://surface-1/render")
            .header("x-cmux-test", "request")
            .body(b"hello body".to_vec())
            .unwrap();
        let response = tauri::http::Response::builder()
            .status(201)
            .header("content-type", "text/plain")
            .body(b"response body".to_vec())
            .unwrap();

        record_custom_scheme_network_request(&state, "surface-1", &request, &response, 10, 25)
            .unwrap();

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert_eq!(reply.requests.len(), 1);
        let record = &reply.requests[0];
        assert_eq!(record.source, "tauri-uri-scheme");
        assert_eq!(record.url, "cmux-md://surface-1/render");
        assert_eq!(record.method, "POST");
        assert_eq!(
            record
                .request_headers
                .get("x-cmux-test")
                .map(String::as_str),
            Some("request")
        );
        assert_eq!(record.request_body.as_deref(), Some("hello body"));
        assert_eq!(record.request_body_preview_kind, "text");
        assert_eq!(record.request_body_size, "hello body".len());
        assert!(!record.request_body_truncated);
        assert_eq!(record.response_status, Some(201));
        assert_eq!(
            record
                .response_headers
                .get("content-type")
                .map(String::as_str),
            Some("text/plain")
        );
        assert_eq!(record.response_body.as_deref(), Some("response body"));
        assert_eq!(record.response_body_preview_kind, "text");
        assert_eq!(record.response_body_size, "response body".len());
        assert!(!record.response_body_truncated);
        assert_eq!(record.duration_ms, Some(15));
        assert_eq!(reply.observer.source, "tauri-uri-scheme");
        assert!(reply.observer.captures_request_headers);
        assert!(reply.observer.captures_request_body);
        assert!(reply.observer.captures_response_status);
        assert!(reply.observer.captures_response_headers);
        assert!(reply.observer.captures_response_body);
    }

    #[test]
    fn proxy_http_exchange_records_cleartext_request_response_metadata() {
        let state = BrowserWebviewState::default();
        let request = b"POST /submit?ok=1 HTTP/1.1\r\nHost: example.com\r\nContent-Type: text/plain\r\n\r\npayload";
        let response =
            b"HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nX-Test: yes\r\n\r\n{\"ok\":true}";

        record_proxy_http_exchange_observation_with_attribution(
            &state,
            "surface-1",
            "socks5",
            "example.com",
            80,
            request,
            response,
            100,
            150,
            Some("panel"),
        )
        .unwrap();

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert_eq!(reply.requests.len(), 1);
        let record = &reply.requests[0];
        assert_eq!(record.source, "proxy-stream-http");
        assert_eq!(record.transport, "socks5");
        assert_eq!(record.proxy_attribution.as_deref(), Some("panel"));
        assert_eq!(record.method, "POST");
        assert_eq!(record.url, "http://example.com/submit?ok=1");
        assert_eq!(
            record.request_headers.get("host").map(String::as_str),
            Some("example.com")
        );
        assert_eq!(record.request_body.as_deref(), Some("payload"));
        assert_eq!(record.request_body_preview_kind, "text");
        assert_eq!(record.request_body_size, "payload".len());
        assert_eq!(record.response_status, Some(202));
        assert_eq!(
            record
                .response_headers
                .get("content-type")
                .map(String::as_str),
            Some("application/json")
        );
        assert_eq!(record.response_body.as_deref(), Some("{\"ok\":true}"));
        assert_eq!(record.response_body_preview_kind, "text");
        assert_eq!(record.response_body_size, "{\"ok\":true}".len());
        assert!(!record.response_body_truncated);
        assert_eq!(record.duration_ms, Some(50));
        assert_eq!(reply.observer.source, "proxy-stream-http");
        assert_eq!(reply.observer.proxy_attribution_mode, "panel");
        assert!(reply.observer.captures_request_headers);
        assert!(reply.observer.captures_request_body);
        assert!(reply.observer.captures_response_status);
        assert!(reply.observer.captures_response_headers);
        assert!(reply.observer.captures_response_body);
    }

    #[test]
    fn proxy_tunnel_observation_records_opaque_https_metadata() {
        let state = BrowserWebviewState::default();
        let upstream = [0x16, 0x03, 0x01, 0x00, 0x2a];
        let downstream = [0x16, 0x03, 0x03, 0x00, 0x31];

        record_proxy_tunnel_observation_with_attribution(
            &state,
            "surface-1",
            "http-connect",
            "secure.example",
            443,
            &upstream,
            true,
            &downstream,
            false,
            100,
            175,
            Some("panel"),
        )
        .unwrap();

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert_eq!(reply.requests.len(), 1);
        let record = &reply.requests[0];
        assert_eq!(record.source, "proxy-stream-tunnel");
        assert_eq!(record.transport, "http-connect");
        assert_eq!(record.proxy_attribution.as_deref(), Some("panel"));
        assert_eq!(record.method, "CONNECT");
        assert_eq!(record.url, "https://secure.example/");
        assert!(record.request_headers.is_empty());
        assert_eq!(record.request_body_preview_kind, "binary");
        assert_eq!(record.request_body_size, upstream.len());
        assert!(record.request_body_truncated);
        assert_eq!(record.response_status, Some(200));
        assert!(record.response_headers.is_empty());
        assert_eq!(record.response_body_preview_kind, "binary");
        assert_eq!(record.response_body_size, downstream.len());
        assert!(!record.response_body_truncated);
        assert_eq!(record.duration_ms, Some(75));
        assert_eq!(reply.observer.source, "proxy-stream-tunnel");
        assert_eq!(reply.observer.proxy_attribution_mode, "panel");
        assert!(reply.observer.captures_response_status);
        assert!(reply.observer.captures_request_body);
        assert!(reply.observer.captures_response_body);
    }

    #[test]
    fn proxy_tunnel_printable_ascii_prefix_remains_binary() {
        let state = BrowserWebviewState::default();
        let upstream = b"apparently textual tunnel bytes";
        let downstream = b"{\"apparently\":\"json\"}";

        record_proxy_tunnel_observation_with_attribution(
            &state,
            "surface-1",
            "socks5",
            "secure.example",
            443,
            upstream,
            false,
            downstream,
            false,
            100,
            175,
            Some("panel"),
        )
        .unwrap();

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        let record = &reply.requests[0];
        assert_eq!(record.request_body_preview_kind, "binary");
        assert_eq!(
            record.request_body.as_deref(),
            Some("<binary body: 31 bytes>")
        );
        assert_eq!(record.response_body_preview_kind, "binary");
        assert_eq!(
            record.response_body.as_deref(),
            Some("<binary body: 21 bytes>")
        );
    }

    #[test]
    fn network_reply_serializes_tunnel_record_for_control_socket_clients() {
        let state = BrowserWebviewState::default();
        record_proxy_tunnel_observation_with_attribution(
            &state,
            "surface-1",
            "socks5",
            "secure.example",
            443,
            &[0x16, 0x03, 0x01],
            false,
            &[0x16, 0x03, 0x03],
            true,
            10,
            30,
            Some("panel"),
        )
        .unwrap();

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        let payload = serde_json::to_value(reply).unwrap();
        let record = &payload["requests"][0];

        assert_eq!(payload["panelId"], serde_json::json!("surface-1"));
        assert_eq!(
            payload["observer"]["source"],
            serde_json::json!("proxy-stream-tunnel")
        );
        assert_eq!(
            payload["observer"]["proxyAttributionMode"],
            serde_json::json!("panel")
        );
        assert_eq!(record["source"], serde_json::json!("proxy-stream-tunnel"));
        assert_eq!(record["transport"], serde_json::json!("socks5"));
        assert_eq!(record["proxyAttribution"], serde_json::json!("panel"));
        assert_eq!(record["method"], serde_json::json!("CONNECT"));
        assert_eq!(record["url"], serde_json::json!("https://secure.example/"));
        assert_eq!(record["responseStatus"], serde_json::json!(200));
        assert_eq!(
            record["requestBodyPreviewKind"],
            serde_json::json!("binary")
        );
        assert_eq!(
            record["responseBodyPreviewKind"],
            serde_json::json!("binary")
        );
        assert_eq!(record["requestBodySize"], serde_json::json!(3));
        assert_eq!(record["responseBodySize"], serde_json::json!(3));
        assert_eq!(record["requestBodyTruncated"], serde_json::json!(false));
        assert_eq!(record["responseBodyTruncated"], serde_json::json!(true));
        assert_eq!(record["startedAtMs"], serde_json::json!(10));
        assert_eq!(record["completedAtMs"], serde_json::json!(30));
        assert_eq!(record["durationMs"], serde_json::json!(20));
        assert!(
            record["note"]
                .as_str()
                .is_some_and(|note| note.contains("Opaque proxy tunnel observed")),
            "serialized record should include opaque tunnel note: {record}"
        );
        assert!(record.get("response_status").is_none());
        assert!(record.get("proxy_attribution").is_none());
        assert!(record.get("request_body_preview_kind").is_none());
    }

    #[test]
    fn proxy_http_exchange_marks_bounded_request_and_response_body_capture() {
        let state = BrowserWebviewState::default();
        let large_request_body = "r".repeat(NETWORK_BODY_CAPTURE_LIMIT_BYTES + 3);
        let large_response_body = "s".repeat(NETWORK_BODY_CAPTURE_LIMIT_BYTES + 7);
        let request = format!(
            "PUT /large HTTP/1.1\r\nHost: example.com\r\nContent-Length: {}\r\n\r\n{}",
            large_request_body.len(),
            large_request_body
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            large_response_body.len(),
            large_response_body
        );

        record_proxy_http_exchange_observation(
            &state,
            "surface-1",
            "http-connect",
            "example.com",
            8080,
            request.as_bytes(),
            response.as_bytes(),
            10,
            15,
        )
        .unwrap();

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        let record = &reply.requests[0];
        assert_eq!(record.url, "http://example.com:8080/large");
        assert_eq!(record.request_body_size, large_request_body.len());
        assert_eq!(record.request_body_preview_kind, "text");
        assert_eq!(
            record.request_body.as_ref().map(String::len),
            Some(NETWORK_BODY_CAPTURE_LIMIT_BYTES)
        );
        assert!(record.request_body_truncated);
        assert_eq!(record.response_body_size, large_response_body.len());
        assert_eq!(record.response_body_preview_kind, "text");
        assert_eq!(
            record.response_body.as_ref().map(String::len),
            Some(NETWORK_BODY_CAPTURE_LIMIT_BYTES)
        );
        assert!(record.response_body_truncated);
        assert_eq!(record.duration_ms, Some(5));
    }

    #[test]
    fn proxy_http_exchange_marks_binary_body_previews() {
        let state = BrowserWebviewState::default();
        let mut request =
            b"POST /upload HTTP/1.1\r\nHost: example.com\r\nContent-Length: 4\r\n\r\n".to_vec();
        request.extend_from_slice(&[0, 159, 146, 150]);
        let mut response =
            b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\n".to_vec();
        response.extend_from_slice(&[137, 80, 78, 71]);

        record_proxy_http_exchange_observation_with_attribution(
            &state,
            "surface-1",
            "socks5",
            "example.com",
            80,
            &request,
            &response,
            10,
            15,
            Some("panel"),
        )
        .unwrap();

        let reply = browser_network_requests_for_control(
            &state,
            "surface-1",
            BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        let record = &reply.requests[0];
        assert_eq!(record.request_body_preview_kind, "binary");
        assert_eq!(record.request_body_size, 4);
        assert_eq!(
            record.request_body.as_deref(),
            Some("<binary body: 4 bytes>")
        );
        assert_eq!(record.response_body_preview_kind, "binary");
        assert_eq!(record.response_body_size, 4);
        assert_eq!(
            record.response_body.as_deref(),
            Some("<binary body: 4 bytes>")
        );
    }

    #[test]
    fn proxy_http_parser_preserves_absolute_form_urls_and_non_default_ports() {
        let request = b"GET http://other.test/path HTTP/1.1\r\nHost: ignored.test\r\n\r\n";
        let response = b"HTTP/1.1 204 No Content\r\n\r\n";
        let parsed_request = parse_proxy_http_request_head(request, "fallback.test", 8080).unwrap();
        let parsed_response = parse_proxy_http_response_head(response).unwrap();

        assert_eq!(parsed_request.method, "GET");
        assert_eq!(parsed_request.url, "http://other.test/path");
        assert_eq!(parsed_response.status, 204);

        let request = b"GET relative HTTP/1.1\r\nHost: fallback.test\r\n\r\n";
        let parsed_request = parse_proxy_http_request_head(request, "fallback.test", 8080).unwrap();
        assert_eq!(parsed_request.url, "http://fallback.test:8080/relative");
    }

    #[test]
    fn proxy_http_parser_rejects_incomplete_or_invalid_heads() {
        assert!(parse_proxy_http_request_head(b"GET / HTTP/1.1\r\n", "example.com", 80).is_err());
        assert!(parse_proxy_http_response_head(b"nope\r\n\r\n").is_err());
        assert!(parse_proxy_http_request_head(
            b"GET / HTTP/1.1\r\nBroken\r\n\r\n",
            "example.com",
            80
        )
        .is_err());
    }
}
