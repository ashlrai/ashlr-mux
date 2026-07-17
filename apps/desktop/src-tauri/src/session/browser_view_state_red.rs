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
