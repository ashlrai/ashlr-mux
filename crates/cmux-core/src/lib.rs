pub mod launch_arguments;
pub mod notifications;
pub mod session;
pub mod session_ops;
pub mod shortcuts;
pub mod shortcuts_action;
pub mod surface_lifecycle;
pub mod window_display;

pub use shortcuts_action::Action;

// Re-export the notification-store public surface so the future `cmux-notify`
// delivery crate can consume it, mirroring how `Action` is re-exported.
pub use notifications::badge::dock_badge_label;
pub use notifications::policy::{
    delivery_decision, has_any_notification_effect, should_suppress_external_delivery,
    DeliveryDecision, TerminalNotificationPolicyEffects,
};
pub use notifications::sound::NotificationSound;
pub use notifications::{
    build_sidebar_unread_summaries, cached_delivery_authorization_decision, fallback_effects,
    make_menu_snapshot, plain_title, state_hint_kind, AddNotificationRequest, ApplyOutcome,
    DismissedTombstoneRing, NotificationAuthorizationState, NotificationClickAction,
    NotificationGates, NotificationMenuSnapshot, NotificationRequest, NotificationState,
    NotificationStore, SidebarApplyChanges, SidebarSurfaceUnreadKey, SidebarUnreadModel,
    SidebarWorkspaceUnreadSummary, StateHintKind, SupersededPhoneDismissBuffer,
    TerminalNotification, DEFAULT_INLINE_NOTIFICATION_LIMIT,
};

pub const CMUX_PLATFORM: &str = "windows-m1-core";

pub fn milestone() -> &'static str {
    "M1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_m1_marker() {
        assert_eq!(milestone(), "M1");
        assert_eq!(CMUX_PLATFORM, "windows-m1-core");
    }
}
