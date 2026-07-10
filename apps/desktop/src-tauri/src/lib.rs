mod agent_session;
mod app_settings;
mod auth_environment;
mod browser;
mod browser_import;
mod cli;
mod command_palette;
mod config;
mod control_socket;
mod default_terminal;
mod diff;
mod directory_search;
mod feed;
mod file_explorer;
mod global_hotkey;
mod markdown;
mod mobile_pairing;
mod notifications;
mod open_file;
mod open_folder;
mod opencode_http;
mod pick_files;
mod remote_proxy;
mod right_sidebar;
mod schemes;
mod session;
mod sidebar_render;
mod terminal;
mod ui_test_hooks;
mod updater_status;
mod window;
mod window_title;
mod workspace_pull_requests;

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tauri::Manager;

fn ping_response() -> &'static str {
    "pong"
}

#[derive(serde::Serialize)]
struct DesktopCoreStatus {
    milestone: &'static str,
    platform: &'static str,
    agent_providers: Vec<&'static str>,
    ipc_fixture_request: String,
}

#[derive(serde::Serialize)]
struct AgentProviderStatus {
    id: &'static str,
    display_name: &'static str,
    executable_name: &'static str,
    transport_kind: &'static str,
    available: bool,
    executable_path: Option<String>,
    searched_directories: Vec<String>,
}

#[tauri::command]
fn ping() -> String {
    ping_response().to_string()
}

#[tauri::command]
fn desktop_core_status() -> DesktopCoreStatus {
    DesktopCoreStatus {
        milestone: cmux_core::milestone(),
        platform: cmux_core::CMUX_PLATFORM,
        agent_providers: cmux_agent::AgentSessionProviderId::ALL
            .into_iter()
            .map(cmux_agent::AgentSessionProviderId::raw_value)
            .collect(),
        ipc_fixture_request: cmux_ipc::append_line(r#"{"id":2,"method":"ping","params":{}}"#),
    }
}

#[tauri::command]
fn agent_provider_status() -> Vec<AgentProviderStatus> {
    let resolver = cmux_agent::AgentExecutableResolver::default();
    cmux_agent::AgentSessionProviderId::ALL
        .into_iter()
        .map(|provider| match resolver.resolve(provider) {
            Ok(plan) => AgentProviderStatus {
                id: provider.raw_value(),
                display_name: provider.display_name(),
                executable_name: provider.executable_name(),
                transport_kind: provider.transport_kind(),
                available: true,
                executable_path: Some(plan.executable_path.to_string_lossy().to_string()),
                searched_directories: Vec::new(),
            },
            Err(cmux_agent::AgentExecutableResolverError::Missing {
                searched_directories,
                ..
            }) => AgentProviderStatus {
                id: provider.raw_value(),
                display_name: provider.display_name(),
                executable_name: provider.executable_name(),
                transport_kind: provider.transport_kind(),
                available: false,
                executable_path: None,
                searched_directories: searched_directories
                    .into_iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect(),
            },
        })
        .collect()
}

#[tauri::command]
fn active_callback_scheme() -> String {
    auth_environment::active_callback_scheme()
}

fn route_deep_link_url(app: &tauri::AppHandle, url: &str) {
    let url = url.trim();
    let active_scheme = auth_environment::active_callback_scheme();
    let is_navigation_link = auth_environment::active_navigation_schemes()
        .iter()
        .any(|scheme| url.starts_with(&format!("{scheme}://workspace/")));
    if !(url.starts_with("cmux://")
        || url.starts_with("cmux-dev://")
        || url.starts_with("cmux-nightly://")
        || url.starts_with(&format!("{active_scheme}://"))
        || url.starts_with("ssh://"))
    {
        return;
    }

    let result = if url.starts_with("cmux-dev://notification") {
        notifications::notification_handle_activation_uri(
            app.clone(),
            app.state::<session::SessionState>(),
            url.to_owned(),
        )
        .map(|reply| reply.message)
    } else if is_navigation_link {
        session::session_handle_navigation_uri(
            app.clone(),
            app.state::<session::SessionState>(),
            url.to_owned(),
        )
        .map(|reply| reply.message)
    } else if url.starts_with("ssh://")
        || url.starts_with("cmux://ssh")
        || url.starts_with("cmux-dev://ssh")
    {
        session::session_handle_ssh_uri(
            app.clone(),
            app.state::<session::SessionState>(),
            url.to_owned(),
        )
        .map(|reply| reply.message)
    } else {
        Err(format!("unsupported cmux deep link route: {url}"))
    };

    match result {
        Ok(message) => eprintln!("[deep-link] {message}"),
        Err(error) => eprintln!("[deep-link] failed to route {url:?}: {error}"),
    }
}

const INSTALL_CLAUDE_CODE_INTEGRATION_MENU_ID: &str = "install_claude_code_integration";

fn install_native_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, Submenu};

    let install_claude = MenuItem::with_id(
        app,
        INSTALL_CLAUDE_CODE_INTEGRATION_MENU_ID,
        "Install Claude Code Integration...",
        true,
        None::<&str>,
    )?;
    let integrations = Submenu::with_items(app, "Integrations", true, &[&install_claude])?;
    let menu = Menu::with_items(app, &[&integrations])?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == INSTALL_CLAUDE_CODE_INTEGRATION_MENU_ID {
            open_claude_code_integration_installer(app);
        }
    });
    Ok(())
}

fn open_claude_code_integration_installer(app: &tauri::AppHandle) {
    let command = claude_code_integration_installer_command(cli::bundled_cli_path(app).as_deref());
    let mut environment = BTreeMap::new();
    if let Some(cli_path) = cli::bundled_cli_path(app) {
        environment.insert(
            "CMUX_BUNDLED_CLI_PATH".to_owned(),
            cli_path.to_string_lossy().into_owned(),
        );
    }
    let environment = if environment.is_empty() {
        None
    } else {
        Some(environment)
    };
    let _snapshot = session::session_new_workspace(
        app.clone(),
        app.state::<session::SessionState>(),
        None,
        Some(command),
        None,
        environment,
    );
}

fn claude_code_integration_installer_command(cli_path: Option<&Path>) -> String {
    let executable = cli_path
        .map(|path| quote_windows_command_arg(&path.to_string_lossy()))
        .unwrap_or_else(|| "cmux".to_owned());
    format!("{executable} hooks claude install")
}

fn quote_windows_command_arg(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_owned();
    }
    if !value
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '&' | '|' | '<' | '>' | '^'))
    {
        return value.to_owned();
    }
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            for arg in argv {
                route_deep_link_url(app, &arg);
            }
        }));
    }

    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(terminal::TerminalState::default())
        .manage(session::SessionState::default())
        .manage(remote_proxy::RemoteProxyBrokerState::default())
        .manage(config::ConfigState::default())
        .manage(control_socket::ControlSocketState::default())
        .manage(control_socket::ControlEventState::default())
        .manage(agent_session::AgentSessionState::default())
        .manage(notifications::NotificationCommandState::default())
        .manage(feed::FeedState::default())
        .manage(right_sidebar::RightSidebarState::default())
        .manage(markdown::MarkdownState::default())
        .manage(open_folder::VSCodeInlineState::default())
        // `DiffState` needs the resolved `app_data_dir`, so it is constructed with
        // the `AppHandle` in `setup` rather than up front on the builder.
        .setup(|app| {
            use tauri_plugin_deep_link::DeepLinkExt;

            let handle = app.handle().clone();
            app.manage(diff::DiffState::new(&handle)?);
            right_sidebar::bootstrap_beta_settings(
                &handle,
                app.state::<right_sidebar::RightSidebarState>().inner(),
            );
            feed::bootstrap_feed_history(app.state::<feed::FeedState>().inner());
            install_native_menu(&handle)?;
            window::install_window_state_listeners(&handle);
            session::bootstrap_session_persistence(&handle, app.state::<session::SessionState>());
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            if let Err(error) = app.deep_link().register_all() {
                eprintln!("[deep-link] failed to register configured schemes: {error}");
            }
            match app.deep_link().get_current() {
                Ok(Some(urls)) => {
                    for url in urls {
                        route_deep_link_url(&handle, &url.to_string());
                    }
                }
                Ok(None) => {}
                Err(error) => eprintln!("[deep-link] failed to read startup URLs: {error}"),
            }
            app.deep_link().on_open_url({
                let handle = handle.clone();
                move |event| {
                    for url in event.urls() {
                        route_deep_link_url(&handle, &url.to_string());
                    }
                }
            });
            if let Err(error) = control_socket::start_control_socket_listener(&handle) {
                eprintln!("[control-socket] failed to start listener: {error}");
            }
            if let Err(error) = config::start_config_file_watcher(&handle) {
                eprintln!("[config] failed to start watcher: {error}");
            }
            let _ = session::session_snapshot(handle, app.state::<session::SessionState>());
            Ok(())
        })
        // Phase-4 custom URI schemes (WebView2 `WebResourceRequested` handlers).
        // Each delegates its parse+lookup to the pure `schemes` helpers, then reads
        // bytes and responds. DEFERRED to the UI checkpoint: that hyphenated custom
        // schemes serve bytes through WebView2, and that the pages loading them
        // carry the expected origin/token.
        .register_asynchronous_uri_scheme_protocol("cmux-diff-viewer", diff_viewer_protocol)
        .register_asynchronous_uri_scheme_protocol("cmux-md", markdown_protocol)
        .register_asynchronous_uri_scheme_protocol("cmux-local-image", local_image_protocol)
        .register_asynchronous_uri_scheme_protocol("cmux-remote-image", remote_image_protocol)
        .register_asynchronous_uri_scheme_protocol("cmux-sidebar-asset", sidebar_asset_protocol)
        .manage(browser::BrowserWebviewState::default())
        .invoke_handler(tauri::generate_handler![
            ping,
            desktop_core_status,
            agent_provider_status,
            active_callback_scheme,
            browser_import::browser_import_destination_profiles,
            browser_import::browser_import_profiles,
            browser_import::browser_import_start,
            browser::browser_attach_webview,
            browser::browser_update_webview,
            browser::browser_close_webview,
            browser::browser_webview_command,
            browser::browser_network_requests,
            browser::browser_clear_network_requests,
            ui_test_hooks::settings_open_capture,
            global_hotkey::global_hotkey_status,
            mobile_pairing::mobile_pairing_status,
            updater_status::updater_status,
            terminal::terminal_open,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_close,
            terminal::terminal_scan_listening_ports,
            session::session_snapshot,
            session::session_set_process_title,
            session::session_split,
            session::session_new_terminal_tab,
            session::session_split_browser,
            session::session_close,
            session::session_set_divider,
            session::session_set_surface_kind,
            session::session_select_adjacent_panel,
            session::session_select_workspace_surface,
            session::session_toggle_split_zoom,
            session::session_set_layout_mode,
            session::session_set_canvas_pane_frame,
            session::session_apply_canvas_action,
            session::session_new_workspace,
            session::session_move_panel_to_new_workspace,
            session::session_handle_navigation_uri,
            session::session_handle_ssh_uri,
            session::session_select_workspace,
            session::session_close_workspace,
            session::session_close_workspaces,
            session::session_set_group_collapsed,
            session::session_rename_workspace,
            session::session_set_workspace_description,
            session::session_reset_workspace_color,
            session::session_set_panel_title,
            session::session_set_panel_pinned,
            session::session_set_panel_unread,
            session::session_set_workspace_unread,
            session::session_set_workspace_pinned,
            session::session_reorder_workspaces,
            session::session_equalize_dividers,
            session::session_restore_previous_launch,
            config::config_load,
            config::config_extension_status,
            config::config_save,
            config::config_reset,
            config::config_settings_file_path,
            config::config_read_raw,
            config::config_write_raw,
            config::open_cmux_settings_file,
            config::open_ghostty_settings_file,
            notifications::notification_preview_delivery_plan,
            notifications::notification_list,
            notifications::notification_mark_read,
            notifications::notification_mark_unread,
            notifications::notification_mark_all_read,
            notifications::notification_remove,
            notifications::notification_clear_all,
            notifications::notification_record_waiting_input,
            notifications::notification_handle_activation_uri,
            notifications::notification_run_custom_command,
            notifications::notification_send_test_toast,
            agent_session::agent_session_rpc,
            agent_session::agent_scan_listening_ports,
            command_palette::command_palette_search,
            control_socket::control_socket_status,
            control_socket::restart_control_socket_listener,
            control_socket::custom_sidebar_action_invoke,
            right_sidebar::right_sidebar_update_state,
            right_sidebar::right_sidebar_beta_settings,
            right_sidebar::right_sidebar_set_beta_feature,
            feed::feed_list,
            feed::feed_load_older,
            feed::feed_resolve,
            open_file::pick_markdown_file,
            open_folder::pick_workspace_folder,
            open_folder::open_folder_in_vscode_inline,
            open_folder::vscode_inline_open_target_available,
            open_folder::vscode_serve_web_stop,
            open_folder::vscode_serve_web_restart,
            pick_files::pick_textbox_files,
            workspace_pull_requests::workspace_git_refresh,
            file_explorer::file_explorer_list_directory,
            file_explorer::file_explorer_open_path,
            file_explorer::file_explorer_read_file,
            file_explorer::file_explorer_write_file,
            diff::diff_create_session,
            diff::diff_comments_rpc,
            directory_search::find_in_directory,
            workspace_pull_requests::workspace_pull_request_links,
            markdown::cmux_lib_rpc,
            markdown::markdown_read_file,
            markdown::markdown_set_document,
            markdown::markdown_render,
            markdown::markdown_apply_theme,
            markdown::markdown_apply_typography,
            markdown::markdown_zoom_in,
            markdown::markdown_zoom_out,
            markdown::markdown_zoom_reset,
            session::session_open_markdown_file,
            session::session_open_file,
            session::session_open_diff_viewer,
            session::session_open_browser_url,
            session::session_browser_go_back,
            session::session_browser_go_forward,
            session::session_clear_browser_history,
            session::session_toggle_browser_omnibar,
            session::session_toggle_browser_focus_mode,
            session::session_toggle_browser_developer_tools,
            session::session_show_browser_developer_tools,
            session::session_set_browser_zoom,
            session::session_new_browser_workspace,
            session::session_reopen_closed_browser_tab,
            cli::cli_install_status,
            cli::install_cli,
            cli::uninstall_cli,
            default_terminal::default_terminal_status,
            default_terminal::make_default_terminal,
            window::window_state,
            window::window_minimize,
            window::window_toggle_maximize,
            window::window_toggle_fullscreen,
            window::window_new,
            window::window_close,
            window::window_open_task_manager
        ])
        .run(tauri::generate_context!())
        .expect("failed to run cmux desktop bootstrap");
}

// ---------------------------------------------------------------------------
// Custom URI-scheme protocol handlers (Phase 4)
// ---------------------------------------------------------------------------

/// Build a `200 OK` byte response with an explicit `Content-Type`.
fn ok_bytes(mime: &str, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .header(tauri::http::header::CONTENT_TYPE, mime)
        .body(body)
        .expect("well-formed uri-scheme response")
}

/// Build an empty response with `status`.
fn status_only(status: tauri::http::StatusCode) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .body(Vec::new())
        .expect("well-formed uri-scheme response")
}

const REMOTE_IMAGE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REMOTE_IMAGE_READ_TIMEOUT: Duration = Duration::from_secs(10);
const REMOTE_IMAGE_READ_CHUNK: usize = 16 * 1024;

fn remote_image_connect_address(addrs: Vec<SocketAddr>) -> Result<SocketAddr, String> {
    if addrs.is_empty() {
        return Err("remote image host resolved to no addresses".to_string());
    }
    if let Some(blocked) = addrs
        .iter()
        .find(|addr| !cmux_markdown::remote_image::is_allowed_resolved_ip(addr.ip()))
    {
        return Err(format!(
            "remote image host resolved to blocked address {}",
            blocked.ip()
        ));
    }
    Ok(addrs[0])
}

fn fetch_remote_image(admitted_url: &str) -> Result<(Vec<u8>, String), String> {
    let url = url::Url::parse(admitted_url)
        .map_err(|error| format!("failed to parse admitted remote image URL: {error}"))?;
    let approved_host = cmux_markdown::remote_image::remote_image_consent_host(&url)
        .ok_or_else(|| "remote image URL lost consent host after admission".to_string())?;
    fetch_remote_image_inner(url, &approved_host, 0)
}

fn fetch_remote_image_inner(
    url: url::Url,
    approved_host: &str,
    redirect_depth: u32,
) -> Result<(Vec<u8>, String), String> {
    let host = url
        .host_str()
        .ok_or_else(|| "remote image URL has no host".to_string())?;
    let addrs: Vec<SocketAddr> = (host, 443)
        .to_socket_addrs()
        .map_err(|error| format!("failed to resolve remote image host {host:?}: {error}"))?
        .collect();
    let connect_addr = remote_image_connect_address(addrs)?;

    let tcp = TcpStream::connect_timeout(&connect_addr, REMOTE_IMAGE_CONNECT_TIMEOUT)
        .map_err(|error| format!("failed to connect to remote image host {host:?}: {error}"))?;
    tcp.set_read_timeout(Some(REMOTE_IMAGE_READ_TIMEOUT))
        .map_err(|error| format!("failed to set remote image read timeout: {error}"))?;
    tcp.set_write_timeout(Some(REMOTE_IMAGE_READ_TIMEOUT))
        .map_err(|error| format!("failed to set remote image write timeout: {error}"))?;

    let connector = native_tls::TlsConnector::new()
        .map_err(|error| format!("failed to create TLS connector: {error}"))?;
    let mut stream = connector
        .connect(host, tcp)
        .map_err(|error| format!("remote image TLS handshake failed: {error}"))?;
    let request = cmux_markdown::remote_image::request_bytes(&url, host)
        .ok_or_else(|| "failed to frame remote image HTTP request".to_string())?;
    stream
        .write_all(&request)
        .map_err(|error| format!("failed to send remote image request: {error}"))?;

    let mut accumulator = cmux_markdown::RemoteImageAccumulator::new(url.clone());
    let mut chunk = [0_u8; REMOTE_IMAGE_READ_CHUNK];
    loop {
        let count = stream
            .read(&mut chunk)
            .map_err(|error| format!("failed to read remote image response: {error}"))?;
        if count == 0 {
            let Some(outcome) = accumulator.final_outcome() else {
                return Err("remote image response ended before a complete image".to_string());
            };
            return handle_remote_image_outcome(outcome, &url, approved_host, redirect_depth);
        }

        match accumulator.process(&chunk[..count]) {
            cmux_markdown::ProcessResult::Continue => {}
            cmux_markdown::ProcessResult::Fail => {
                return Err("remote image response failed validation".to_string());
            }
            cmux_markdown::ProcessResult::Finish(outcome) => {
                return handle_remote_image_outcome(outcome, &url, approved_host, redirect_depth);
            }
        }
    }
}

fn handle_remote_image_outcome(
    outcome: cmux_markdown::Outcome,
    request_url: &url::Url,
    approved_host: &str,
    redirect_depth: u32,
) -> Result<(Vec<u8>, String), String> {
    match outcome {
        cmux_markdown::Outcome::Image { data, mime } => Ok((data, mime)),
        cmux_markdown::Outcome::Redirect(redirect) => {
            let next_depth = redirect_depth + 1;
            let Some(next_url) =
                cmux_markdown::redirect_decision(&redirect, request_url, approved_host, next_depth)
            else {
                return Err("remote image redirect was blocked".to_string());
            };
            fetch_remote_image_inner(next_url, approved_host, next_depth)
        }
    }
}

/// `cmux-diff-viewer://<token>/<path>` — serve a file from the token's registered
/// allowlist (trust-gated by the diff session registry). Untrusted/unknown →
/// `403`; a registered file that vanished on disk → `404`.
fn diff_viewer_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let started_at_ms = uri_scheme_now_ms();
    let uri = request.uri().to_string();
    let app = ctx.app_handle();
    let response = match app.try_state::<diff::DiffState>() {
        Some(state) => {
            let registered = state.resolve_diff_request(&uri, std::time::SystemTime::now());
            let bundled = || {
                app.path()
                    .resource_dir()
                    .ok()
                    .and_then(|root| schemes::resolve_diff_asset_request(&root, &uri))
            };
            match registered.or_else(bundled) {
                Some((path, mime)) => match std::fs::read(&path) {
                    Ok(bytes) => ok_bytes(&mime, bytes),
                    Err(_) => status_only(tauri::http::StatusCode::NOT_FOUND),
                },
                None => status_only(tauri::http::StatusCode::FORBIDDEN),
            }
        }
        None => status_only(tauri::http::StatusCode::INTERNAL_SERVER_ERROR),
    };
    respond_observed_uri_scheme(&ctx, &request, response, responder, started_at_ms);
}

/// `cmux-md://…/<asset>` — serve the viewer shell HTML / lazy libraries / CSS from
/// the bundled markdown-viewer assets.
fn markdown_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let started_at_ms = uri_scheme_now_ms();
    let uri = request.uri().to_string();
    let app = ctx.app_handle();
    let assets = app
        .path()
        .resource_dir()
        .ok()
        .and_then(|root| {
            app.try_state::<markdown::MarkdownState>()
                .map(|s| (s, root))
        })
        .and_then(|(state, root)| state.assets(&root));
    let response = match assets.and_then(|assets| schemes::resolve_md_request(&assets, &uri)) {
        Some((bytes, mime)) => ok_bytes(mime, bytes),
        None => status_only(tauri::http::StatusCode::NOT_FOUND),
    };
    respond_observed_uri_scheme(&ctx, &request, response, responder, started_at_ms);
}

/// `cmux-local-image://…?url=<file-url>` — serve an on-disk image jailed to the
/// calling webview's markdown-document directory.
fn local_image_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let started_at_ms = uri_scheme_now_ms();
    let uri = request.uri().to_string();
    let panel_id = schemes::markdown_panel_id_from_request(&uri);
    let markdown_file = ctx
        .app_handle()
        .try_state::<markdown::MarkdownState>()
        .map(|state| {
            let key = panel_id.as_deref().unwrap_or(ctx.webview_label());
            state.markdown_file_for(key)
        })
        .unwrap_or_default();
    let response = match schemes::resolve_local_image_request(&uri, &markdown_file) {
        Some((path, mime)) => match std::fs::read(&path) {
            Ok(bytes) => ok_bytes(&mime, bytes),
            Err(_) => status_only(tauri::http::StatusCode::NOT_FOUND),
        },
        None => status_only(tauri::http::StatusCode::FORBIDDEN),
    };
    respond_observed_uri_scheme(&ctx, &request, response, responder, started_at_ms);
}

/// `cmux-remote-image://…?url=<https>` — validate the outbound URL against the
/// SSRF gate, resolve and screen every returned address, then fetch the image
/// through a size-limited HTTP/1.1-over-TLS response accumulator.
fn remote_image_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let started_at_ms = uri_scheme_now_ms();
    let uri = request.uri().to_string();
    let response = match schemes::remote_image_request(&uri) {
        Some(admitted_url) => match fetch_remote_image(&admitted_url) {
            Ok((bytes, mime)) => ok_bytes(&mime, bytes),
            Err(error) => {
                eprintln!("[remote-image] failed to fetch {admitted_url:?}: {error}");
                status_only(tauri::http::StatusCode::BAD_GATEWAY)
            }
        },
        None => status_only(tauri::http::StatusCode::FORBIDDEN),
    };
    respond_observed_uri_scheme(&ctx, &request, response, responder, started_at_ms);
}

/// `cmux-sidebar-asset://<sidebar>/<asset>?source=<sidebar-file>` — serve image
/// files from the source sidebar's adjacent `<name>.assets/` directory.
fn sidebar_asset_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let started_at_ms = uri_scheme_now_ms();
    let uri = request.uri().to_string();
    let response = match control_socket::resolve_custom_sidebar_asset_request(&uri) {
        Some((path, mime)) => match std::fs::read(&path) {
            Ok(bytes) => ok_bytes(&mime, bytes),
            Err(_) => status_only(tauri::http::StatusCode::NOT_FOUND),
        },
        None => status_only(tauri::http::StatusCode::FORBIDDEN),
    };
    respond_observed_uri_scheme(&ctx, &request, response, responder, started_at_ms);
}

fn respond_observed_uri_scheme(
    ctx: &tauri::UriSchemeContext<'_, tauri::Wry>,
    request: &tauri::http::Request<Vec<u8>>,
    response: tauri::http::Response<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
    started_at_ms: u64,
) {
    if let Some(panel_id) = browser::panel_id_from_browser_webview_label(ctx.webview_label()) {
        if let Some(state) = ctx.app_handle().try_state::<browser::BrowserWebviewState>() {
            let completed_at_ms = uri_scheme_now_ms();
            let _ = browser::record_custom_scheme_network_request(
                state.inner(),
                &panel_id,
                request,
                &response,
                started_at_ms,
                completed_at_ms,
            );
        }
    }
    responder.respond(response);
}

fn uri_scheme_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        agent_provider_status, claude_code_integration_installer_command, desktop_core_status,
        ping, ping_response, remote_image_connect_address,
    };

    fn addr(ip: &str) -> std::net::SocketAddr {
        std::net::SocketAddr::new(ip.parse().expect("test IP parses"), 443)
    }

    #[test]
    fn ping_response_is_stable() {
        assert_eq!(ping_response(), "pong");
    }

    #[test]
    fn claude_installer_menu_command_uses_quoted_cli_path() {
        let command = claude_code_integration_installer_command(Some(std::path::Path::new(
            r"C:\Program Files\cmux\cmux.exe",
        )));
        assert_eq!(
            command,
            r#""C:\Program Files\cmux\cmux.exe" hooks claude install"#
        );
    }

    #[test]
    fn remote_image_connect_address_rejects_empty_resolution() {
        assert!(remote_image_connect_address(Vec::new()).is_err());
    }

    #[test]
    fn remote_image_connect_address_rejects_any_blocked_resolution() {
        let result = remote_image_connect_address(vec![addr("8.8.8.8"), addr("127.0.0.1")]);
        assert!(result.is_err());
    }

    #[test]
    fn remote_image_connect_address_accepts_public_resolution() {
        assert_eq!(
            remote_image_connect_address(vec![addr("8.8.8.8"), addr("1.1.1.1")]).unwrap(),
            addr("8.8.8.8")
        );
    }

    #[test]
    fn ping_command_returns_the_bootstrap_reply() {
        assert_eq!(ping(), "pong");
    }

    #[test]
    fn desktop_status_uses_shared_core() {
        let status = desktop_core_status();
        assert_eq!(status.milestone, "M1");
        assert_eq!(status.platform, "windows-m1-core");
        assert!(status.agent_providers.contains(&"codex"));
        assert!(status.ipc_fixture_request.ends_with('\n'));
    }

    #[test]
    fn desktop_status_lists_every_agent_provider_in_declared_order() {
        let status = desktop_core_status();
        // The web layer renders providers by index, so the order is part of
        // the contract, not an implementation detail.
        assert_eq!(status.agent_providers, vec!["codex", "claude", "opencode"]);
    }

    #[test]
    fn desktop_status_ipc_fixture_matches_the_golden_ping_request() {
        let status = desktop_core_status();
        // Exactly the bootstrap ping request the socket-v2 golden fixture
        // encodes, terminated with the framing newline.
        assert_eq!(
            status.ipc_fixture_request,
            "{\"id\":2,\"method\":\"ping\",\"params\":{}}\n"
        );
        // The request must be a single newline-delimited frame.
        assert_eq!(status.ipc_fixture_request.matches('\n').count(), 1);
        assert!(status.ipc_fixture_request.starts_with('{'));
    }

    #[test]
    fn desktop_status_serializes_to_the_shape_the_web_bridge_consumes() {
        // Contract parity: tauri-bridge.ts unwraps `desktop_core_status` as a
        // bare object (no { ok, value } envelope). Assert the exact JSON keys
        // and value types the web layer relies on, so web/Rust drift is caught
        // without a native launch.
        let status = desktop_core_status();
        let value = serde_json::to_value(&status).expect("status serializes");

        let object = value.as_object().expect("status is a JSON object");

        // Exact key set — no extra, no missing.
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "agent_providers",
                "ipc_fixture_request",
                "milestone",
                "platform",
            ]
        );

        assert_eq!(object["milestone"], serde_json::json!("M1"));
        assert_eq!(object["platform"], serde_json::json!("windows-m1-core"));
        assert_eq!(
            object["agent_providers"],
            serde_json::json!(["codex", "claude", "opencode"])
        );
        assert!(object["agent_providers"].is_array());
        assert!(object["milestone"].is_string());
        assert!(object["platform"].is_string());
        assert!(object["ipc_fixture_request"].is_string());

        // The bare object must NOT look like a NativeReply envelope; otherwise
        // callNative would try to unwrap/throw on it.
        assert!(object.get("ok").is_none());
        assert!(object.get("value").is_none());
        assert!(object.get("error").is_none());
    }

    #[test]
    fn agent_provider_status_lists_every_provider_in_declared_order() {
        let status = agent_provider_status();
        let ids: Vec<&str> = status.iter().map(|provider| provider.id).collect();
        assert_eq!(ids, vec!["codex", "claude", "opencode"]);

        let names: Vec<&str> = status
            .iter()
            .map(|provider| provider.display_name)
            .collect();
        assert_eq!(names, vec!["Codex", "Claude Code", "OpenCode"]);
    }

    #[test]
    fn agent_provider_status_serializes_to_the_settings_shape() {
        let status = agent_provider_status();
        let value = serde_json::to_value(&status).expect("status serializes");
        let rows = value.as_array().expect("provider status is an array");
        assert_eq!(rows.len(), 3);

        let first = rows[0]
            .as_object()
            .expect("provider status row is an object");
        let mut keys: Vec<&str> = first.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "available",
                "display_name",
                "executable_name",
                "executable_path",
                "id",
                "searched_directories",
                "transport_kind",
            ]
        );
        assert!(first["id"].is_string());
        assert!(first["display_name"].is_string());
        assert!(first["executable_name"].is_string());
        assert!(first["transport_kind"].is_string());
        assert!(first["available"].is_boolean());
        assert!(first["searched_directories"].is_array());
    }

    #[test]
    fn ping_command_is_a_bare_string_not_an_envelope() {
        // callNative passes a bare string straight through. Confirm `ping`
        // serializes to a plain JSON string (no { ok, value } wrapper).
        let value = serde_json::to_value(ping()).expect("ping serializes");
        assert_eq!(value, serde_json::json!("pong"));
        assert!(value.is_string());
    }
}
