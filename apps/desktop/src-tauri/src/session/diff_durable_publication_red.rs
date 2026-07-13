//! Durable publication and starter-token ownership for the diff surface.

use super::*;

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

fn diff_transaction(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    panel_id: &str,
    token: &str,
    request_path: &str,
    registered: bool,
) -> Result<Option<AppSessionSnapshot>, String> {
    let Some(normalized_request_path) = normalize_diff_request_path(request_path) else {
        return Ok(None);
    };
    if !registered {
        return Ok(None);
    }
    let (resolved, snapshot) =
        transact_value_if_changed_snapshot(authority, publication, |candidate| {
            let resolved =
                apply_open_diff_viewer(candidate, panel_id, token, &normalized_request_path);
            Ok::<_, std::convert::Infallible>((resolved, resolved))
        })
        .map_err(|error| match error {
            PaneTopologyControlError::Publication(error) => error,
            PaneTopologyControlError::Operation(error) => match error {},
        })?;
    Ok(resolved.then_some(snapshot))
}

#[test]
fn valid_diff_open_and_normalized_repeat_are_durably_published() {
    let before = initial_snapshot("surface-1");
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let opened = diff_transaction(
        &authority,
        &mut publication,
        "surface-1",
        "tok-abcdef0123456789",
        " index.html ",
        true,
    )
    .unwrap()
    .expect("valid diff open");
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);

    let authority = GatedSnapshot::new(opened.clone());
    let mut publication = RecordingPublication::new(&opened);
    assert_eq!(
        diff_transaction(
            &authority,
            &mut publication,
            "surface-1",
            " tok-abcdef0123456789 ",
            "/index.html",
            true,
        )
        .unwrap(),
        Some(opened)
    );
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
}

#[test]
fn validation_registry_and_panel_misses_return_none_without_publication() {
    let before = initial_snapshot("surface-1");
    for (panel_id, token, path, registered) in [
        ("surface-1", "tok-abcdef0123456789", "/../bad.html", true),
        ("surface-1", "tok-abcdef0123456789", "/index.html", false),
        ("missing", "tok-abcdef0123456789", "/index.html", true),
        ("surface-1", "short", "/index.html", true),
    ] {
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        assert_eq!(
            diff_transaction(
                &authority,
                &mut publication,
                panel_id,
                token,
                path,
                registered,
            )
            .unwrap(),
            None
        );
        assert!(publication.calls.is_empty());
        assert_eq!(*authority.lock().unwrap(), before);
    }
}

#[test]
fn persistence_failure_rolls_back_diff_authority_baseline_and_event() {
    let before = initial_snapshot("surface-1");
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("injected diff persistence failure".into());
    assert_eq!(
        diff_transaction(
            &authority,
            &mut publication,
            "surface-1",
            "tok-abcdef0123456789",
            "/index.html",
            true,
        ),
        Err("injected diff persistence failure".into())
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), before);
    assert_eq!(publication.baseline, before);
    assert!(publication.events.is_empty());
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
fn production_opener_and_tauri_route_use_fallible_optional_publication() {
    let session = include_str!("../session.rs");
    let opener = function_source(session, "pub(crate) fn open_diff_viewer_in_panel(");
    assert!(opener.contains("Result<Option<AppSessionSnapshot>, String>"));
    assert!(opener.contains("transact_value_if_changed("));
    assert!(!opener.contains("snapshot.lock()"));
    assert!(!opener.contains("notify_session_changed("));
    let validate = opener.find("normalize_diff_request_path(").unwrap();
    let registry = opener.find("has_registered_request(").unwrap();
    let transaction = opener.find("transact_value_if_changed(").unwrap();
    assert!(validate < registry && registry < transaction);

    let tauri = function_source(session, "pub fn session_open_diff_viewer(");
    assert!(tauri.contains("Result<AppSessionSnapshot, String>"));
    assert!(tauri.contains("open_diff_viewer_in_panel("));
    assert!(tauri.contains('?'));
    assert!(tauri.contains("unable to open diff viewer in pane"));
    assert!(!tauri.contains("unregister_starter_session("));
}

#[test]
fn socket_route_tracks_owned_starters_and_compensates_only_failure_paths() {
    let socket = include_str!("../control_socket.rs");
    let route = function_source(socket, "fn surface_open_diff(");
    assert!(route.contains("create_starter_session("));
    assert!(route.contains("open_diff_viewer_in_panel("));
    assert!(route.contains("Ok(Some(snapshot))"));
    assert!(route.contains("Ok(None)"));
    assert!(route.contains("Err(message)"));
    assert!(route.contains("\"not_found\""));
    assert!(route.contains("\"internal\""));
    assert!(route.contains("unregister_starter_session("));

    let created = route.find("create_starter_session(").unwrap();
    let opened = route.find("open_diff_viewer_in_panel(").unwrap();
    let first_unregister = route.find("unregister_starter_session(").unwrap();
    assert!(created < opened && opened < first_unregister);
    assert!(route.matches("unregister_starter_session(").count() >= 2);

    // Ownership must be represented separately from the caller-supplied token;
    // otherwise a publication failure could revoke a token the caller owns.
    assert!(route.contains("created_token") || route.contains("owned_token"));
    let supplied_arm = &route[..created];
    assert!(!supplied_arm.contains("unregister_starter_session("));
}
