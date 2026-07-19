//! Regression coverage for native window queries on the control worker.

use super::*;

#[test]
fn resolved_active_pointer_avoids_native_focus_fallback_after_startup() {
    let pointer = ControlActiveWindowState::default();
    pointer.set_startup_fallback("window-2");
    pointer.set("window-1");
    assert_eq!(
        pointer.resolved_current(),
        None,
        "startup focus must be resolved before the stored pointer is authoritative"
    );
    assert_eq!(
        pointer.resolve_startup(Some("window-1".into())).as_deref(),
        Some("window-1")
    );
    assert_eq!(
        pointer.resolved_current().as_deref(),
        Some("window-1"),
        "later routing must use the stored pointer without querying native focus"
    );
}

#[test]
fn key_history_survives_repeat_focus_and_is_cleared_after_close() {
    let pointer = ControlActiveWindowState::default();
    pointer.set_startup_fallback("window-1");
    assert_eq!(pointer.resolve_startup(None).as_deref(), Some("window-1"));

    pointer.set_key("window-2");
    assert_eq!(
        pointer.key_history(),
        (Some("window-2".into()), Some("window-1".into()))
    );
    pointer.set_key("window-2");
    assert_eq!(pointer.key_history().1.as_deref(), Some("window-1"));

    pointer.close_key("window-2", Some("window-1"));
    assert_eq!(pointer.key_history(), (Some("window-1".into()), None));
    assert_eq!(pointer.resolved_current().as_deref(), Some("window-1"));
}
