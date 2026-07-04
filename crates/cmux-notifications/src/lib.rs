//! Pure, headless port of the macOS cmux notification delivery core.
//!
//! Mirrors the deterministic decisions of
//! `Packages/macOS/CmuxNotifications` — foreground presentation options,
//! OS-category composition, feed permission negotiation, and notification
//! response routing — with every `@MainActor`/`UserNotifications` side effect
//! inverted into returned data. See [`delivery`] for the port map.

mod delivery;

pub use delivery::{
    feed_permission_notification_decision, presentation_options, NotificationActionData,
    NotificationCategoryData, NotificationDeliveryActionTitles, NotificationDeliveryCore,
    NotificationDeliveryResponse, NotificationFeedDecision, NotificationFeedExitPlanMode,
    NotificationFeedPermissionCapabilities, NotificationFeedPermissionMode,
    NotificationNavClickAction, Outcome, PresentationOptions,
    TerminalNotificationDeliveryIdentifiers, UserInfo, DEFAULT_ACTION_IDENTIFIER,
    DISMISS_ACTION_IDENTIFIER,
};
