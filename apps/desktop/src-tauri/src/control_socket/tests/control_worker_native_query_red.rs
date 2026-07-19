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
