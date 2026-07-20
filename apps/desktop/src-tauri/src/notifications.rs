//! Native notification side-effect bridge.
//!
//! The pure notification crates decide *whether* a command should run; this
//! desktop seam performs the side effect for the global `notifications.command`
//! setting. On Windows the configured command is launched through the M3 Job
//! Object supervisor with the same env contract as macOS.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::collections::BTreeSet;

use cmux_core::notifications::{
    NotificationStore, TerminalNotification, TerminalNotificationPolicyEffects,
};
#[cfg(windows)]
use cmux_notify::WindowsToastDelivery;
use cmux_notify::{
    delivery_plan, parse_activation_uri, NotificationActivation, NotificationAppIdentity,
    NotificationDelivery,
};
use cmux_process::{JobObjectSupervisor, ProcessSupervisor, SpawnSpec};
use tauri::{AppHandle, State};

const ENV_TITLE: &str = "CMUX_NOTIFICATION_TITLE";
const ENV_SUBTITLE: &str = "CMUX_NOTIFICATION_SUBTITLE";
const ENV_BODY: &str = "CMUX_NOTIFICATION_BODY";
const ACTIVATION_SCHEME: &str = "cmux-dev";
const WAITING_INPUT_NOTIFICATION_PREFIX: &str = "waiting-input";

#[derive(Default)]
pub struct NotificationCommandState {
    supervisor: Arc<JobObjectSupervisor>,
    store: Mutex<NotificationStore>,
}

#[derive(Debug, serde::Serialize)]
pub struct NotificationCommandRunReply {
    pub ran: bool,
    pub skipped_reason: Option<String>,
    pub root_pid: Option<u32>,
}

#[derive(Debug, serde::Serialize)]
pub struct NotificationDeliveryPreviewReply {
    pub decision: Option<String>,
    pub toast_xml: Option<String>,
    pub tag: Option<String>,
    pub group: Option<String>,
    pub launch: Option<String>,
    pub run_command: bool,
    pub play_in_app_sound: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct NotificationToastSendReply {
    pub sent: bool,
    pub tag: Option<String>,
    pub group: Option<String>,
    pub message: String,
}

#[derive(Debug, serde::Serialize)]
pub struct NotificationActivationHandleReply {
    pub handled: bool,
    pub changed: bool,
    pub notification_id: String,
    pub workspace_id: String,
    pub panel_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationCenterItemView {
    pub id: String,
    pub workspace_id: String,
    pub surface_id: Option<String>,
    pub panel_id: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub created_at: i64,
    pub is_read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationCenterReply {
    pub notifications: Vec<NotificationCenterItemView>,
    pub unread_count: usize,
    pub total_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct NotificationMutationEffects {
    pub center: NotificationCenterReply,
    pub cleared: Vec<TerminalNotification>,
}

#[derive(Debug, Clone)]
struct NotificationCommandRequest {
    command: String,
    title: String,
    subtitle: String,
    body: String,
    cwd: Option<PathBuf>,
}

fn notification_center_item_view(
    notification: &TerminalNotification,
) -> NotificationCenterItemView {
    NotificationCenterItemView {
        id: notification.id.clone(),
        workspace_id: notification.tab_id.clone(),
        surface_id: notification.surface_id.clone(),
        panel_id: notification.panel_id.clone(),
        title: notification.title.clone(),
        subtitle: notification.subtitle.clone(),
        body: notification.body.clone(),
        created_at: notification.created_at,
        is_read: notification.is_read,
    }
}

fn notification_center_reply(store: &NotificationStore) -> NotificationCenterReply {
    let notifications = store
        .notifications()
        .iter()
        .map(notification_center_item_view)
        .collect::<Vec<_>>();
    NotificationCenterReply {
        unread_count: notifications
            .iter()
            .filter(|notification| !notification.is_read)
            .count(),
        total_count: notifications.len(),
        notifications,
    }
}

fn non_blank(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

fn current_unix_timestamp_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn waiting_input_notification(
    workspace_id: String,
    panel_id: String,
    workspace_title: Option<String>,
    panel_title: Option<String>,
    created_at: i64,
) -> TerminalNotification {
    let workspace_title = non_blank(workspace_title);
    let panel_title = non_blank(panel_title);
    let title = panel_title
        .as_ref()
        .map(|title| format!("Waiting for input: {title}"))
        .unwrap_or_else(|| "Waiting for input".to_owned());
    let body = panel_title
        .as_ref()
        .map(|title| format!("{title} is waiting for input."))
        .unwrap_or_else(|| "The agent session is waiting for input.".to_owned());
    TerminalNotification {
        id: format!("{WAITING_INPUT_NOTIFICATION_PREFIX}:{workspace_id}:{panel_id}"),
        tab_id: workspace_id,
        surface_id: Some(panel_id.clone()),
        panel_id: Some(panel_id),
        title,
        subtitle: workspace_title.unwrap_or_else(|| "Agent session".to_owned()),
        body,
        created_at,
        is_read: false,
        pane_flash: true,
        click_action: None,
    }
}

pub(crate) fn with_notification_store<R>(
    state: &NotificationCommandState,
    action: impl FnOnce(&mut NotificationStore) -> R,
) -> Result<R, String> {
    let mut store = state
        .store
        .lock()
        .map_err(|_| "notification store mutex poisoned".to_string())?;
    Ok(action(&mut store))
}

#[tauri::command]
pub fn notification_list(
    state: State<'_, NotificationCommandState>,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state.inner(), |store| notification_center_reply(store))
}

pub(crate) fn notification_list_for_control(
    state: &NotificationCommandState,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state, |store| notification_center_reply(store))
}

pub(crate) fn notification_dismiss_for_control(
    state: &NotificationCommandState,
    id: Option<&str>,
    all_read: bool,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state, |store| {
        if let Some(id) = id {
            store.remove(id);
        } else if all_read {
            let ids = store
                .notifications()
                .iter()
                .filter(|item| item.is_read)
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();
            for id in ids {
                store.remove(&id);
            }
        }
        notification_center_reply(store)
    })
}

pub(crate) fn notification_mark_read_for_control(
    state: &NotificationCommandState,
    id: Option<&str>,
    workspace_id: Option<&str>,
    surface_id: Option<&str>,
    all: bool,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state, |store| {
        if let Some(id) = id {
            store.mark_read(id);
        } else if let Some(workspace_id) = workspace_id {
            if surface_id.is_some() {
                store.mark_read_for_tab_surface(workspace_id, surface_id);
            } else {
                store.mark_read_for_tab(workspace_id);
            }
        } else if all {
            store.mark_all_read();
        }
        notification_center_reply(store)
    })
}

pub(crate) fn set_workspace_unread_in_store(
    store: &mut NotificationStore,
    workspace_id: &str,
    unread: bool,
) -> NotificationMutationEffects {
    let cleared = if unread {
        Vec::new()
    } else {
        store
            .notifications()
            .iter()
            .filter(|notification| notification.tab_id == workspace_id && !notification.is_read)
            .cloned()
            .collect()
    };
    if unread {
        store.mark_unread_for_tab(workspace_id);
    } else {
        store.mark_read_for_tab(workspace_id);
    }
    NotificationMutationEffects {
        center: notification_center_reply(store),
        cleared,
    }
}

pub(crate) fn notification_clear_workspace_for_control(
    state: &NotificationCommandState,
    workspace_id: &str,
) -> Result<NotificationMutationEffects, String> {
    with_notification_store(state, |store| {
        let cleared = store
            .notifications()
            .iter()
            .filter(|notification| notification.tab_id == workspace_id)
            .cloned()
            .collect();
        store.clear_for_tab(workspace_id);
        NotificationMutationEffects {
            center: notification_center_reply(store),
            cleared,
        }
    })
}

pub(crate) fn clear_native_notifications(effects: &NotificationMutationEffects) {
    clear_native_notification_rows(&effects.cleared);
}

#[cfg(windows)]
fn clear_native_notification_rows(notifications: &[TerminalNotification]) {
    let keys = notifications
        .iter()
        .map(cmux_notify::supersede_key)
        .map(|key| (key.tag, key.group))
        .collect::<BTreeSet<_>>();
    if keys.is_empty() {
        return;
    }
    let _ = thread::Builder::new()
        .name("cmux-notification-clear".to_owned())
        .spawn(move || {
            let delivery = WindowsToastDelivery::new(NotificationAppIdentity::new(
                "Cmuxterm.Cmux.Dev",
                ACTIVATION_SCHEME,
            ));
            for (tag, group) in keys {
                let _ = delivery.clear(&tag, &group);
            }
        });
}

#[cfg(not(windows))]
fn clear_native_notification_rows(_notifications: &[TerminalNotification]) {}

pub(crate) fn notification_clear_for_control(
    state: &NotificationCommandState,
    workspace_id: Option<&str>,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state, |store| {
        if let Some(workspace_id) = workspace_id {
            store.clear_for_tab(workspace_id);
        } else {
            store.clear_all();
        }
        notification_center_reply(store)
    })
}

/// Canonical unregisterMainWindow drops stale notifications for the closing
/// window and each of its workspaces (AppDelegate.swift:16274-16280 at pinned
/// e1825d40d: clearNotifications(forTabId: removed.windowId), then one
/// clearNotifications(forTabId:) per tab of the removed TabManager).
pub(crate) fn notification_clear_window_for_control(
    state: &NotificationCommandState,
    window_id: &str,
    workspace_ids: &[String],
) -> Result<(), String> {
    with_notification_store(state, |store| {
        store.clear_for_tab(window_id);
        for workspace_id in workspace_ids {
            store.clear_for_tab(workspace_id);
        }
    })
}

pub(crate) fn notification_open_target_for_control(
    state: &NotificationCommandState,
    id: Option<&str>,
) -> Result<Option<TerminalNotification>, String> {
    with_notification_store(state, |store| {
        let target_id = match id {
            Some(id) => store
                .notifications()
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.id.clone()),
            None => store
                .notifications()
                .iter()
                .find(|item| !item.is_read)
                .map(|item| item.id.clone()),
        }?;
        store.mark_read(&target_id);
        store
            .notifications()
            .iter()
            .find(|item| item.id == target_id)
            .cloned()
    })
}

pub(crate) fn notification_create_for_control(
    state: &NotificationCommandState,
    workspace_id: String,
    surface_id: String,
    title: String,
    subtitle: String,
    body: String,
) -> Result<TerminalNotification, String> {
    let notification = TerminalNotification {
        id: uuid::Uuid::new_v4().to_string(),
        tab_id: workspace_id,
        surface_id: Some(surface_id.clone()),
        panel_id: Some(surface_id),
        title,
        subtitle,
        body,
        created_at: current_unix_timestamp_seconds(),
        is_read: false,
        pane_flash: true,
        click_action: None,
    };
    with_notification_store(state, |store| {
        store.record(notification.clone(), false);
    })?;
    deliver_control_notification(&notification)?;
    Ok(notification)
}

#[cfg(windows)]
fn deliver_control_notification(notification: &TerminalNotification) -> Result<(), String> {
    deliver_windows_notification(notification, "default", None).map(|_| ())
}

#[cfg(not(windows))]
fn deliver_control_notification(_notification: &TerminalNotification) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn notification_mark_read(
    id: String,
    state: State<'_, NotificationCommandState>,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state.inner(), |store| {
        store.mark_read(&id);
        notification_center_reply(store)
    })
}

#[tauri::command]
pub fn notification_mark_unread(
    id: String,
    state: State<'_, NotificationCommandState>,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state.inner(), |store| {
        store.mark_unread(&id);
        notification_center_reply(store)
    })
}

#[tauri::command]
pub fn notification_remove(
    id: String,
    state: State<'_, NotificationCommandState>,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state.inner(), |store| {
        store.remove(&id);
        notification_center_reply(store)
    })
}

#[tauri::command]
pub fn notification_mark_all_read(
    state: State<'_, NotificationCommandState>,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state.inner(), |store| {
        store.mark_all_read();
        notification_center_reply(store)
    })
}

#[tauri::command]
pub fn notification_clear_all(
    state: State<'_, NotificationCommandState>,
) -> Result<NotificationCenterReply, String> {
    with_notification_store(state.inner(), |store| {
        store.clear_all();
        notification_center_reply(store)
    })
}

#[tauri::command]
pub fn notification_record_waiting_input(
    workspace_id: String,
    panel_id: String,
    workspace_title: Option<String>,
    panel_title: Option<String>,
    state: State<'_, NotificationCommandState>,
) -> Result<NotificationCenterReply, String> {
    let notification = waiting_input_notification(
        workspace_id,
        panel_id,
        workspace_title,
        panel_title,
        current_unix_timestamp_seconds(),
    );
    with_notification_store(state.inner(), |store| {
        store.record(notification, false);
        notification_center_reply(store)
    })
}

#[tauri::command]
pub fn notification_preview_delivery_plan(
    title: Option<String>,
    subtitle: Option<String>,
    body: Option<String>,
    sound: Option<String>,
    custom_sound_file_path: Option<String>,
    workspace_id: Option<String>,
    surface_id: Option<String>,
    panel_id: Option<String>,
    desktop: Option<bool>,
    sound_effect: Option<bool>,
    command_effect: Option<bool>,
    suppressed: Option<bool>,
) -> NotificationDeliveryPreviewReply {
    let notification = settings_test_notification(
        "settings-preview",
        title.unwrap_or_else(|| "cmux test notification".to_owned()),
        subtitle.unwrap_or_else(|| "Settings > Notifications".to_owned()),
        body.unwrap_or_else(|| {
            "Preview of the Windows toast payload cmux will deliver.".to_owned()
        }),
        workspace_id,
        surface_id,
        panel_id,
    );
    let effects = TerminalNotificationPolicyEffects {
        desktop: desktop.unwrap_or(true),
        sound: sound_effect.unwrap_or(true),
        command: command_effect.unwrap_or(false),
        ..TerminalNotificationPolicyEffects::default()
    };
    let identity = NotificationAppIdentity::new("Cmuxterm.Cmux.Dev", "cmux-dev");
    let plan = delivery_plan(
        &identity,
        &notification,
        &effects,
        suppressed.unwrap_or(false),
        sound.as_deref().unwrap_or("default"),
        custom_sound_file_path.as_deref(),
    );

    match plan {
        None => NotificationDeliveryPreviewReply {
            decision: None,
            toast_xml: None,
            tag: None,
            group: None,
            launch: None,
            run_command: false,
            play_in_app_sound: false,
        },
        Some(plan) => {
            let toast = plan.toast;
            NotificationDeliveryPreviewReply {
                decision: Some(format!("{:?}", plan.decision)),
                toast_xml: toast.as_ref().map(|toast| toast.xml.clone()),
                tag: toast.as_ref().map(|toast| toast.tag.clone()),
                group: toast.as_ref().map(|toast| toast.group.clone()),
                launch: toast.as_ref().map(|toast| toast.launch.clone()),
                run_command: plan.run_command,
                play_in_app_sound: plan.play_in_app_sound,
            }
        }
    }
}

#[tauri::command]
pub fn notification_handle_activation_uri(
    app: AppHandle,
    state: State<'_, crate::session::SessionState>,
    uri: String,
) -> Result<NotificationActivationHandleReply, String> {
    let activation =
        parse_activation_uri(&uri, Some(ACTIVATION_SCHEME)).map_err(|error| error.to_string())?;
    route_notification_activation(app, state, activation)
}

fn route_notification_activation(
    app: AppHandle,
    state: State<'_, crate::session::SessionState>,
    activation: NotificationActivation,
) -> Result<NotificationActivationHandleReply, String> {
    let target_panel_id = activation
        .surface_id
        .as_deref()
        .or(activation.panel_id.as_deref())
        .ok_or_else(|| {
            "notification activation URI did not include a surfaceId or panelId".to_owned()
        })?
        .to_owned();
    let (changed, snapshot) = crate::session::select_workspace_surface(
        &app,
        &state,
        &activation.tab_id,
        &target_panel_id,
    )
    .map_err(|error| match error {
        crate::session::PaneTopologyControlError::Publication(error) => error,
        crate::session::PaneTopologyControlError::Operation(error) => match error {},
    })?;
    let handled = changed
        || crate::session::workspace_surface_is_selected(
            &snapshot,
            &activation.tab_id,
            &target_panel_id,
        );
    let message = if handled {
        "Notification activation routed to workspace surface.".to_owned()
    } else {
        "Notification activation target was not found in the current session.".to_owned()
    };

    Ok(NotificationActivationHandleReply {
        handled,
        changed,
        notification_id: activation.notification_id,
        workspace_id: activation.tab_id,
        panel_id: Some(target_panel_id),
        message,
    })
}

#[tauri::command]
pub fn notification_send_test_toast(
    sound: Option<String>,
    custom_sound_file_path: Option<String>,
    workspace_id: Option<String>,
    surface_id: Option<String>,
    panel_id: Option<String>,
) -> Result<NotificationToastSendReply, String> {
    send_test_toast(
        sound.as_deref(),
        custom_sound_file_path.as_deref(),
        workspace_id,
        surface_id,
        panel_id,
    )
}

#[cfg(windows)]
fn send_test_toast(
    sound: Option<&str>,
    custom_sound_file_path: Option<&str>,
    workspace_id: Option<String>,
    surface_id: Option<String>,
    panel_id: Option<String>,
) -> Result<NotificationToastSendReply, String> {
    let notification = settings_test_notification(
        "settings-test-toast",
        "cmux test notification".to_owned(),
        "Settings > Notifications".to_owned(),
        "This toast was delivered through the Windows notification backend.".to_owned(),
        workspace_id,
        surface_id,
        panel_id,
    );
    let (tag, group) = deliver_windows_notification(
        &notification,
        sound.unwrap_or("default"),
        custom_sound_file_path,
    )?;
    Ok(NotificationToastSendReply {
        sent: true,
        tag,
        group,
        message: "Windows test toast sent.".to_owned(),
    })
}

#[cfg(windows)]
fn deliver_windows_notification(
    notification: &TerminalNotification,
    sound: &str,
    custom_sound_file_path: Option<&str>,
) -> Result<(Option<String>, Option<String>), String> {
    let identity = NotificationAppIdentity::new("Cmuxterm.Cmux.Dev", "cmux-dev");
    let effects = TerminalNotificationPolicyEffects {
        desktop: true,
        sound: true,
        command: false,
        ..TerminalNotificationPolicyEffects::default()
    };
    let plan = delivery_plan(
        &identity,
        notification,
        &effects,
        false,
        sound,
        custom_sound_file_path,
    )
    .ok_or_else(|| "notification did not produce a deliverable plan".to_owned())?;
    let tag = plan.toast.as_ref().map(|toast| toast.tag.clone());
    let group = plan.toast.as_ref().map(|toast| toast.group.clone());
    WindowsToastDelivery::new(identity)
        .deliver(&plan)
        .map_err(|error| error.to_string())?;
    Ok((tag, group))
}

#[cfg(not(windows))]
fn send_test_toast(
    _sound: Option<&str>,
    _custom_sound_file_path: Option<&str>,
    _workspace_id: Option<String>,
    _surface_id: Option<String>,
    _panel_id: Option<String>,
) -> Result<NotificationToastSendReply, String> {
    Err("Windows toast delivery is only available on Windows.".to_owned())
}

fn settings_test_notification(
    id: &str,
    title: String,
    subtitle: String,
    body: String,
    workspace_id: Option<String>,
    surface_id: Option<String>,
    panel_id: Option<String>,
) -> TerminalNotification {
    TerminalNotification {
        id: id.to_owned(),
        tab_id: workspace_id.unwrap_or_else(|| "settings".to_owned()),
        surface_id: Some(surface_id.unwrap_or_else(|| "notifications".to_owned())),
        panel_id,
        title,
        subtitle,
        body,
        created_at: 0,
        is_read: false,
        pane_flash: true,
        click_action: None,
    }
}

#[tauri::command]
pub fn notification_run_custom_command(
    command: String,
    title: Option<String>,
    subtitle: Option<String>,
    body: Option<String>,
    cwd: Option<String>,
    state: State<'_, NotificationCommandState>,
) -> Result<NotificationCommandRunReply, String> {
    let request = NotificationCommandRequest {
        command,
        title: title.unwrap_or_default(),
        subtitle: subtitle.unwrap_or_default(),
        body: body.unwrap_or_default(),
        cwd: cwd
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from),
    };
    let Some(spec) = notification_command_spec(&request) else {
        return Ok(NotificationCommandRunReply {
            ran: false,
            skipped_reason: Some("No notification command is configured.".to_owned()),
            root_pid: None,
        });
    };

    let (handle, io) = state
        .supervisor
        .spawn_captured(spec)
        .map_err(|error| error.to_string())?;
    let supervisor = Arc::clone(&state.supervisor);
    thread::Builder::new()
        .name("cmux-notification-command-reaper".to_owned())
        .spawn(move || {
            let (stdin, chunks) = io.into_parts();
            drop(stdin);
            while chunks.recv().is_ok() {}
            supervisor.reap(handle.id);
        })
        .map_err(|error| format!("failed to start notification command reaper: {error}"))?;

    Ok(NotificationCommandRunReply {
        ran: true,
        skipped_reason: None,
        root_pid: Some(handle.root_pid),
    })
}

fn notification_command_spec(request: &NotificationCommandRequest) -> Option<SpawnSpec> {
    let command = request.command.trim();
    if command.is_empty() {
        return None;
    }

    let mut env: BTreeMap<String, String> = std::env::vars().collect();
    apply_notification_env(&mut env, &request.title, &request.subtitle, &request.body);

    let (program, args) = shell_program_and_args(command, &env);
    let mut spec = SpawnSpec::new(program).args(args).env(env);
    if let Some(cwd) = &request.cwd {
        spec = spec.current_dir(cwd);
    }
    Some(spec)
}

fn apply_notification_env(
    env: &mut BTreeMap<String, String>,
    title: &str,
    subtitle: &str,
    body: &str,
) {
    env.insert(ENV_TITLE.to_owned(), title.to_owned());
    env.insert(ENV_SUBTITLE.to_owned(), subtitle.to_owned());
    env.insert(ENV_BODY.to_owned(), body.to_owned());
}

fn shell_program_and_args(command: &str, env: &BTreeMap<String, String>) -> (PathBuf, Vec<String>) {
    #[cfg(windows)]
    {
        let program = env
            .iter()
            .find(|(key, value)| key.eq_ignore_ascii_case("ComSpec") && !value.is_empty())
            .map(|(_, value)| PathBuf::from(value))
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        (
            program,
            vec!["/d".to_owned(), "/c".to_owned(), command.to_owned()],
        )
    }

    #[cfg(not(windows))]
    {
        let _ = env;
        (
            PathBuf::from("/bin/sh"),
            vec!["-c".to_owned(), command.to_owned()],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: &str, is_read: bool) -> TerminalNotification {
        TerminalNotification {
            id: id.to_owned(),
            tab_id: "workspace-1".to_owned(),
            surface_id: Some(format!("surface-{id}")),
            panel_id: Some("panel-1".to_owned()),
            title: "Done".to_owned(),
            subtitle: "Agent".to_owned(),
            body: "Finished".to_owned(),
            created_at: 123,
            is_read,
            pane_flash: true,
            click_action: None,
        }
    }

    include!("notifications/control_contract_red.rs");

    /// Canonical unregisterMainWindow clearing scope (AppDelegate.swift:
    /// 16274-16280 at pinned e1825d40d): the window id row and each workspace
    /// row go; unrelated workspaces survive.
    #[test]
    fn clear_window_for_control_drops_window_and_workspace_rows_only() {
        let state = NotificationCommandState::default();
        let seed = |tab_id: &str, id: &str| {
            let mut row = notification(id, false);
            row.tab_id = tab_id.to_owned();
            row
        };
        with_notification_store(&state, |store| {
            store.record(seed("window-2", "n-window"), false);
            store.record(seed("workspace-2", "n-ws2"), false);
            store.record(seed("workspace-2b", "n-ws2b"), false);
            store.record(seed("workspace-other", "n-keep"), false);
        })
        .expect("seed");

        notification_clear_window_for_control(
            &state,
            "window-2",
            &["workspace-2".to_owned(), "workspace-2b".to_owned()],
        )
        .expect("clear");

        let remaining = with_notification_store(&state, |store| {
            store
                .notifications()
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        })
        .expect("read");
        assert_eq!(remaining, ["n-keep"], "only unrelated workspaces survive");
    }

    #[test]
    fn workspace_action_unread_wrapper_uses_notification_store_manual_state() {
        let state = NotificationCommandState::default();
        with_notification_store(&state, |store| {
            store.record(notification("unread-1", false), false);
            store.record(notification("already-read", true), false);
        })
        .expect("seed");
        with_notification_store(&state, |store| {
            set_workspace_unread_in_store(store, "workspace-1", true)
        })
        .expect("mark unread");
        assert!(
            with_notification_store(&state, |store| store.has_manual_unread("workspace-1"))
                .unwrap()
        );

        let effects = with_notification_store(&state, |store| {
            set_workspace_unread_in_store(store, "workspace-1", false)
        })
        .expect("mark read");
        assert_eq!(
            effects
                .cleared
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["unread-1"],
            "native/event dismissal effects retain the exact unread rows"
        );
        assert!(
            !with_notification_store(&state, |store| store.has_manual_unread("workspace-1"))
                .unwrap()
        );
    }

    #[test]
    fn workspace_close_clear_retains_exact_rows_for_native_and_event_effects() {
        let state = NotificationCommandState::default();
        with_notification_store(&state, |store| {
            store.record(notification("closed-unread", false), false);
            store.record(notification("closed-read", true), false);
            let mut unrelated = notification("unrelated", false);
            unrelated.tab_id = "workspace-2".to_owned();
            store.record(unrelated, false);
        })
        .expect("seed");

        let effects = notification_clear_workspace_for_control(&state, "workspace-1")
            .expect("clear closed workspace notifications");

        let mut cleared = effects
            .cleared
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>();
        cleared.sort_unstable();
        assert_eq!(cleared, ["closed-read", "closed-unread"]);
        assert_eq!(effects.center.total_count, 1);
        assert_eq!(effects.center.notifications[0].id, "unrelated");
    }

    fn request(command: &str) -> NotificationCommandRequest {
        NotificationCommandRequest {
            command: command.to_owned(),
            title: "Done".to_owned(),
            subtitle: "Workspace".to_owned(),
            body: "Agent finished".to_owned(),
            cwd: Some(PathBuf::from(r"C:\work\demo")),
        }
    }

    #[test]
    fn notification_center_reply_projects_store_rows_and_counts_unread() {
        let mut store = NotificationStore::new();
        store.record(notification("read", true), false);
        store.record(notification("unread", false), false);

        let reply = notification_center_reply(&store);

        assert_eq!(reply.total_count, 2);
        assert_eq!(reply.unread_count, 1);
        assert_eq!(reply.notifications[0].id, "unread");
        assert_eq!(reply.notifications[0].workspace_id, "workspace-1");
        assert_eq!(
            reply.notifications[0].surface_id.as_deref(),
            Some("surface-unread")
        );
        assert_eq!(reply.notifications[0].panel_id.as_deref(), Some("panel-1"));
        assert!(!reply.notifications[0].is_read);
    }

    #[test]
    fn notification_center_reply_reflects_mark_all_read() {
        let mut store = NotificationStore::new();
        store.record(notification("unread", false), false);
        store.mark_all_read();

        let reply = notification_center_reply(&store);

        assert_eq!(reply.total_count, 1);
        assert_eq!(reply.unread_count, 0);
        assert!(reply.notifications[0].is_read);
    }

    #[test]
    fn notification_center_reply_returns_marked_unread_first() {
        let mut store = NotificationStore::new();
        store.record(notification("old", false), false);
        store.record(notification("new", false), false);
        store.mark_read("old");

        store.mark_unread("old");
        let reply = notification_center_reply(&store);

        assert_eq!(reply.unread_count, 2);
        assert_eq!(reply.notifications[0].id, "old");
        assert!(!reply.notifications[0].is_read);
    }

    #[test]
    fn waiting_input_notification_includes_custom_panel_title() {
        let notification = waiting_input_notification(
            "workspace-1".to_owned(),
            "panel-1".to_owned(),
            Some("  Phoenix  ".to_owned()),
            Some("  API logs  ".to_owned()),
            99,
        );

        assert_eq!(notification.id, "waiting-input:workspace-1:panel-1");
        assert_eq!(notification.tab_id, "workspace-1");
        assert_eq!(notification.surface_id.as_deref(), Some("panel-1"));
        assert_eq!(notification.panel_id.as_deref(), Some("panel-1"));
        assert_eq!(notification.title, "Waiting for input: API logs");
        assert_eq!(notification.subtitle, "Phoenix");
        assert_eq!(notification.body, "API logs is waiting for input.");
        assert_eq!(notification.created_at, 99);
        assert!(!notification.is_read);
    }

    #[test]
    fn waiting_input_notification_falls_back_without_custom_panel_title() {
        let notification = waiting_input_notification(
            "workspace-1".to_owned(),
            "panel-1".to_owned(),
            Some(" ".to_owned()),
            Some("\t".to_owned()),
            100,
        );

        assert_eq!(notification.title, "Waiting for input");
        assert_eq!(notification.subtitle, "Agent session");
        assert_eq!(notification.body, "The agent session is waiting for input.");
    }

    #[test]
    fn blank_command_is_skipped() {
        assert!(notification_command_spec(&request("   ")).is_none());
    }

    #[test]
    fn spec_includes_notification_env_and_cwd() {
        let spec = notification_command_spec(&request("echo %CMUX_NOTIFICATION_TITLE%"))
            .expect("non-empty command builds a spawn spec");
        assert_eq!(spec.env[ENV_TITLE], "Done");
        assert_eq!(spec.env[ENV_SUBTITLE], "Workspace");
        assert_eq!(spec.env[ENV_BODY], "Agent finished");
        assert_eq!(spec.current_dir, Some(PathBuf::from(r"C:\work\demo")));
    }

    #[test]
    fn notification_env_overrides_existing_values() {
        let mut env = BTreeMap::from([
            (ENV_TITLE.to_owned(), "old-title".to_owned()),
            (ENV_SUBTITLE.to_owned(), "old-subtitle".to_owned()),
            (ENV_BODY.to_owned(), "old-body".to_owned()),
        ]);
        apply_notification_env(&mut env, "new-title", "new-subtitle", "new-body");
        assert_eq!(env[ENV_TITLE], "new-title");
        assert_eq!(env[ENV_SUBTITLE], "new-subtitle");
        assert_eq!(env[ENV_BODY], "new-body");
    }

    #[cfg(windows)]
    #[test]
    fn windows_shell_uses_comspec_and_cmd_c() {
        let env = BTreeMap::from([(
            "ComSpec".to_owned(),
            r"C:\Windows\System32\cmd.exe".to_owned(),
        )]);
        let (program, args) = shell_program_and_args("echo hi", &env);
        assert_eq!(program, PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        assert_eq!(args, vec!["/d", "/c", "echo hi"]);
    }

    #[test]
    fn delivery_preview_uses_notify_plan() {
        let reply = notification_preview_delivery_plan(
            Some("Title & body".to_owned()),
            None,
            None,
            Some("Ping".to_owned()),
            None,
            None,
            None,
            None,
            Some(true),
            Some(true),
            Some(true),
            Some(false),
        );
        assert_eq!(reply.decision.as_deref(), Some("Desktop"));
        assert_eq!(reply.tag.as_deref(), Some("notifications"));
        assert_eq!(reply.group.as_deref(), Some("settings"));
        assert!(reply.run_command);
        assert!(!reply.play_in_app_sound);
        let xml = reply.toast_xml.expect("desktop preview builds toast XML");
        assert!(xml.contains("Title &amp; body"));
        assert!(xml.contains("ms-winsoundevent:Notification.IM"));
    }

    #[test]
    fn delivery_preview_can_target_current_workspace_surface() {
        let reply = notification_preview_delivery_plan(
            None,
            None,
            None,
            Some("default".to_owned()),
            None,
            Some("workspace-1".to_owned()),
            Some("surface-1".to_owned()),
            Some("surface-1".to_owned()),
            Some(true),
            Some(true),
            Some(false),
            Some(false),
        );
        assert_eq!(reply.tag.as_deref(), Some("surface-1"));
        assert_eq!(reply.group.as_deref(), Some("workspace-1"));
        assert_eq!(
            reply.launch.as_deref(),
            Some(
                "cmux-dev://notification?id=settings-preview&tabId=workspace-1&surfaceId=surface-1&panelId=surface-1"
            )
        );
    }

    #[test]
    fn suppressed_delivery_preview_has_no_toast_or_command() {
        let reply = notification_preview_delivery_plan(
            None,
            None,
            None,
            Some("default".to_owned()),
            None,
            None,
            None,
            None,
            Some(true),
            Some(true),
            Some(true),
            Some(true),
        );
        assert_eq!(reply.decision.as_deref(), Some("Suppressed"));
        assert_eq!(reply.toast_xml, None);
        assert!(!reply.run_command);
        assert!(reply.play_in_app_sound);
    }

    #[test]
    fn delivery_preview_respects_disabled_sound_effect() {
        let reply = notification_preview_delivery_plan(
            None,
            None,
            None,
            Some("Ping".to_owned()),
            None,
            None,
            None,
            None,
            Some(true),
            Some(false),
            Some(false),
            Some(false),
        );
        assert_eq!(reply.decision.as_deref(), Some("Desktop"));
        assert!(!reply.play_in_app_sound);
        assert!(reply
            .toast_xml
            .expect("desktop preview builds toast XML")
            .contains(r#"<audio silent="true"/>"#));
    }
}
