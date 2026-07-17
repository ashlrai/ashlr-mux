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
