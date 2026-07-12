//! Durable publication for pane-local browser view-state commands.

use super::*;

#[derive(Debug, Clone, Copy)]
enum BrowserWriter {
    Back,
    Forward,
    ClearHistory,
    ToggleOmnibar,
    ToggleFocus,
    ToggleDeveloperTools,
    ShowDeveloperTools,
    SetZoom,
}

impl BrowserWriter {
    fn helper(self) -> &'static str {
        match self {
            Self::Back => "browser_go_back_for_control",
            Self::Forward => "browser_go_forward_for_control",
            Self::ClearHistory => "clear_browser_history_for_control",
            Self::ToggleOmnibar => "toggle_browser_omnibar_for_control",
            Self::ToggleFocus => "toggle_browser_focus_mode_for_control",
            Self::ToggleDeveloperTools => "toggle_browser_developer_tools_for_control",
            Self::ShowDeveloperTools => "show_browser_developer_tools_for_control",
            Self::SetZoom => "set_browser_zoom_for_control",
        }
    }

    fn public(self) -> Option<&'static str> {
        match self {
            Self::Back => Some("session_browser_go_back"),
            Self::Forward => Some("session_browser_go_forward"),
            Self::ClearHistory => Some("session_clear_browser_history"),
            Self::ToggleOmnibar => Some("session_toggle_browser_omnibar"),
            Self::ToggleFocus => Some("session_toggle_browser_focus_mode"),
            Self::ToggleDeveloperTools => Some("session_toggle_browser_developer_tools"),
            Self::ShowDeveloperTools => Some("session_show_browser_developer_tools"),
            Self::SetZoom => None,
        }
    }

    fn route(self) -> &'static str {
        match self {
            Self::Back => "browser_back",
            Self::Forward => "browser_forward",
            Self::ClearHistory => "browser_clear_history",
            Self::ToggleOmnibar => "browser_toggle_omnibar",
            Self::ToggleFocus => "browser_toggle_focus_mode",
            Self::ToggleDeveloperTools => "browser_toggle_developer_tools",
            Self::ShowDeveloperTools => "browser_show_developer_tools",
            Self::SetZoom => "browser_set_zoom",
        }
    }
}

const WRITERS: [BrowserWriter; 8] = [
    BrowserWriter::Back,
    BrowserWriter::Forward,
    BrowserWriter::ClearHistory,
    BrowserWriter::ToggleOmnibar,
    BrowserWriter::ToggleFocus,
    BrowserWriter::ToggleDeveloperTools,
    BrowserWriter::ShowDeveloperTools,
    BrowserWriter::SetZoom,
];

struct RecordingPublication {
    calls: Vec<&'static str>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
}

impl RecordingPublication {
    fn new(snapshot: &AppSessionSnapshot) -> Self {
        Self {
            calls: Vec::new(),
            persist_error: None,
            baseline: snapshot.clone(),
            events: Vec::new(),
        }
    }
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        self.persist_error.take().map_or(Ok(()), Err)
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
        self.baseline = candidate.clone();
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        self.events.push(candidate.clone());
        Ok(())
    }
}

fn initial() -> AppSessionSnapshot {
    initial_snapshot("surface-1")
}

fn pane(snapshot: &AppSessionSnapshot) -> &cmux_core::session::SessionPaneLayoutSnapshot {
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) =
        snapshot.windows[0].tab_manager.workspaces[0]
            .layout
            .as_ref()
    else {
        panic!("expected pane")
    };
    pane
}

fn seed_history(snapshot: &mut AppSessionSnapshot) {
    assert!(apply_open_browser_url(
        snapshot,
        "surface-1",
        Some("https://one.example")
    ));
    assert!(apply_open_browser_url(
        snapshot,
        "surface-1",
        Some("https://two.example")
    ));
}

fn seed_for_changed(writer: BrowserWriter) -> AppSessionSnapshot {
    let mut snapshot = initial();
    match writer {
        BrowserWriter::Back | BrowserWriter::ClearHistory => seed_history(&mut snapshot),
        BrowserWriter::Forward => {
            seed_history(&mut snapshot);
            assert!(apply_browser_go_back(&mut snapshot, "surface-1"));
        }
        _ => {}
    }
    snapshot
}

fn apply_changed(writer: BrowserWriter, snapshot: &mut AppSessionSnapshot) -> bool {
    match writer {
        BrowserWriter::Back => apply_browser_go_back(snapshot, "surface-1"),
        BrowserWriter::Forward => apply_browser_go_forward(snapshot, "surface-1"),
        BrowserWriter::ClearHistory => apply_clear_browser_history(snapshot, "surface-1"),
        BrowserWriter::ToggleOmnibar => apply_toggle_browser_omnibar(snapshot, "surface-1"),
        BrowserWriter::ToggleFocus => apply_toggle_browser_focus_mode(snapshot, "surface-1"),
        BrowserWriter::ToggleDeveloperTools => {
            apply_toggle_browser_developer_tools(snapshot, "surface-1")
        }
        BrowserWriter::ShowDeveloperTools => {
            apply_show_browser_developer_tools(snapshot, "surface-1", "inspector")
        }
        BrowserWriter::SetZoom => apply_set_browser_zoom(snapshot, "surface-1", 12.0),
    }
}

fn apply_noop(writer: BrowserWriter, snapshot: &mut AppSessionSnapshot) -> bool {
    match writer {
        BrowserWriter::ShowDeveloperTools => {
            apply_show_browser_developer_tools(snapshot, "missing", "inspector")
        }
        BrowserWriter::SetZoom => apply_set_browser_zoom(snapshot, "missing", 99.0),
        BrowserWriter::Back => apply_browser_go_back(snapshot, "missing"),
        BrowserWriter::Forward => apply_browser_go_forward(snapshot, "missing"),
        BrowserWriter::ClearHistory => apply_clear_browser_history(snapshot, "missing"),
        BrowserWriter::ToggleOmnibar => apply_toggle_browser_omnibar(snapshot, "missing"),
        BrowserWriter::ToggleFocus => apply_toggle_browser_focus_mode(snapshot, "missing"),
        BrowserWriter::ToggleDeveloperTools => {
            apply_toggle_browser_developer_tools(snapshot, "missing")
        }
    }
}

#[test]
fn all_eight_changed_browser_writers_publish_exact_normalized_view_state() {
    for writer in WRITERS {
        let before = seed_for_changed(writer);
        let mut expected = before.clone();
        assert!(apply_changed(writer, &mut expected), "{}", writer.helper());
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let committed = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_changed(writer, candidate)
        })
        .unwrap();
        assert_eq!(committed, expected, "{}", writer.helper());
        assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
        assert_eq!(publication.baseline, expected);
        assert_eq!(publication.events, [expected]);
    }

    let mut history = seed_for_changed(BrowserWriter::ClearHistory);
    assert!(apply_changed(BrowserWriter::ClearHistory, &mut history));
    assert_eq!(
        pane(&history).browser_url.as_deref(),
        Some("https://two.example")
    );
    assert_eq!(pane(&history).browser_back_history, None);
    assert_eq!(pane(&history).browser_forward_history, None);

    let mut defaults = initial();
    assert!(apply_toggle_browser_omnibar(&mut defaults, "surface-1"));
    assert_eq!(pane(&defaults).browser_omnibar_visible, Some(false));
    assert!(apply_toggle_browser_focus_mode(&mut defaults, "surface-1"));
    assert_eq!(pane(&defaults).browser_focus_mode_active, Some(true));
    assert!(apply_toggle_browser_developer_tools(
        &mut defaults,
        "surface-1"
    ));
    assert_eq!(
        pane(&defaults).browser_developer_tools_panel.as_deref(),
        Some("inspector")
    );
    assert!(apply_set_browser_zoom(&mut defaults, "surface-1", 99.0));
    assert_eq!(pane(&defaults).browser_page_zoom, Some(3.0));
}

#[test]
fn all_eight_same_or_missing_browser_writes_are_exact_zero_operation_noops() {
    for writer in WRITERS {
        let before = seed_for_changed(writer);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let returned = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_noop(writer, candidate)
        })
        .unwrap();
        assert_eq!(returned, before, "{}", writer.helper());
        assert_eq!(*authority.lock().unwrap(), before, "{}", writer.helper());
        assert!(publication.calls.is_empty(), "{}", writer.helper());
        assert!(publication.events.is_empty(), "{}", writer.helper());
    }
}

#[test]
fn all_eight_browser_persist_failures_leak_no_candidate_or_publication() {
    for writer in WRITERS {
        let before = seed_for_changed(writer);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        publication.persist_error = Some("injected browser persistence failure".into());
        assert_eq!(
            transact_snapshot_if_changed(&authority, &mut publication, |candidate| apply_changed(
                writer, candidate
            ))
            .unwrap_err(),
            "injected browser persistence failure"
        );
        assert_eq!(publication.calls, ["persist"], "{}", writer.helper());
        assert_eq!(*authority.lock().unwrap(), before, "{}", writer.helper());
        assert_eq!(publication.baseline, before, "{}", writer.helper());
        assert!(publication.events.is_empty(), "{}", writer.helper());
    }
}

fn function_source<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("missing {signature}"));
    let tail = &source[start..];
    let body_start = tail.find('{').unwrap();
    let mut depth = 0;
    for (offset, character) in tail[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..body_start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {signature}")
}

#[test]
fn mutator_wrappers_public_duplicates_and_sockets_share_one_fallible_gate() {
    let session = include_str!("../session.rs");
    let mutator = function_source(session, "fn mutate_browser_for_control<F>(");
    assert!(mutator.contains("Result<AppSessionSnapshot, String>"));
    assert!(mutator.contains("state.transact_snapshot_if_changed(app,"));
    assert!(!mutator.contains("snapshot.lock()") && !mutator.contains("notify_session_changed("));

    let socket = include_str!("../control_socket.rs");
    for writer in WRITERS {
        let helper = function_source(session, &format!("pub(crate) fn {}(", writer.helper()));
        assert!(
            helper.contains("Result<AppSessionSnapshot, String>"),
            "{}",
            writer.helper()
        );
        assert_eq!(
            helper.matches("mutate_browser_for_control(").count(),
            1,
            "{}",
            writer.helper()
        );
        assert!(!helper.contains("snapshot.lock()") && !helper.contains("notify_session_changed("));
        if matches!(writer, BrowserWriter::ShowDeveloperTools) {
            assert!(helper.contains("\"console\" | \"react\""));
            assert!(helper.contains("\"inspector\""));
        }

        if let Some(public) = writer.public() {
            let command = function_source(session, &format!("pub fn {public}("));
            assert!(
                command.contains("Result<AppSessionSnapshot, String>"),
                "{public}"
            );
            assert_eq!(command.matches(writer.helper()).count(), 1, "{public}");
            assert!(command.contains("Ok(snapshot)"), "{public}");
            assert!(
                !command.contains("apply_")
                    && !command.contains("snapshot.lock()")
                    && !command.contains("notify_session_changed("),
                "{public}"
            );
        }

        let route = function_source(socket, &format!("fn {}(", writer.route()));
        assert_eq!(
            route.matches(writer.helper()).count(),
            1,
            "{}",
            writer.route()
        );
        assert!(
            route.contains("surface_list_from_params("),
            "{}",
            writer.route()
        );
        assert!(route.contains("Err(message)"), "{}", writer.route());
        assert!(route.contains("\"internal\""), "{}", writer.route());
    }

    let strict_zoom = function_source(session, "pub fn session_set_browser_zoom(");
    let strict_zoom_compact: String = strict_zoom
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    assert!(strict_zoom.contains("Result<AppSessionSnapshot, String>"));
    assert!(strict_zoom.contains("apply_set_browser_zoom("));
    assert!(!strict_zoom_compact.contains(".snapshot.lock()"));
    assert!(!strict_zoom.contains("notify_session_changed("));
    assert!(strict_zoom.contains("unable to set browser zoom for pane"));
    assert!(!strict_zoom.contains("set_browser_zoom_for_control("));
}
