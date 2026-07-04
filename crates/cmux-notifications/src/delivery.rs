//! Pure decision core extracted from the macOS notification delivery shell.
//!
//! Direct port of the deterministic, side-effect-free logic in
//! `Packages/macOS/CmuxNotifications/Sources/CmuxNotifications/NotificationDeliveryCoordinator.swift`
//! (`@MainActor @Observable final class NotificationDeliveryCoordinator`) plus
//! its plain-value collaborators:
//! - `NotificationDeliveryResponse.swift` (the already-flat response value)
//! - `NotificationNavClickAction.swift` (`init?(userInfo:)`)
//! - `NotificationFeedDecision` / `NotificationFeedPermissionMode` /
//!   `NotificationFeedExitPlanMode` / `NotificationFeedPermissionCapabilities`
//! - `TerminalNotificationDeliveryIdentifiers` / `NotificationDeliveryActionTitles`
//!
//! # What is intentionally left out (the `@MainActor`/`UserNotifications` shell)
//! The coordinator's injected seams are *not* ported here; instead their
//! effects are returned as data:
//! - `UserNotificationCenterConfiguring.setNotificationCategories` /
//!   `setDelegate` — the delegate assignment is a pure side effect with no
//!   decision content, so `configureUserNotifications` (`:44-47`) is dropped.
//!   The category *composition* it installs is returned by
//!   [`NotificationDeliveryCore::notification_categories`] as
//!   [`NotificationCategoryData`] instead of `UNNotificationCategory`.
//! - `NotificationDeliveryTerminalNavigating` (open / performClickAction /
//!   markNotificationRead), `NotificationFeedReplying.deliverReply`, and
//!   `NotificationApplicationActivating.activateApplication` — all become
//!   [`Outcome`] variants. The one *read* seam,
//!   `NotificationFeedReplying.permissionCapabilities(requestId:)` (`:226`), is
//!   injected into [`NotificationDeliveryCore::handle`] as a lookup closure.
//! - `UNNotification` / `UNNotificationResponse`: presentation reads only
//!   `content.sound != nil` (`:52`) and the response is already flattened by
//!   `NotificationDeliveryResponse.init(_:)` — modelled directly by
//!   [`NotificationDeliveryResponse`] with a typed [`UserInfo`] map. Swift's
//!   `userInfo` is `[AnyHashable: Any]` and every read is `as? String`
//!   (`:181,257,262,268,289`), so a `String`→`String` map is faithful.

use std::collections::BTreeMap;

use uuid::Uuid;

/// The `UNNotificationDefaultActionIdentifier` system constant value.
///
/// Swift references the `UserNotifications` symbol
/// `UNNotificationDefaultActionIdentifier`
/// (`NotificationDeliveryCoordinator.swift:214,256`). The app forwards this
/// exact string through `NotificationDeliveryResponse.actionIdentifier`, so the
/// Windows port pins the documented constant value here.
pub const DEFAULT_ACTION_IDENTIFIER: &str = "com.apple.UNNotificationDefaultActionIdentifier";

/// The `UNNotificationDismissActionIdentifier` system constant value.
///
/// Mirrors `UNNotificationDismissActionIdentifier`
/// (`NotificationDeliveryCoordinator.swift:213,276`).
pub const DISMISS_ACTION_IDENTIFIER: &str = "com.apple.UNNotificationDismissActionIdentifier";

// ---------------------------------------------------------------------------
// Feed decision value types (NotificationFeedDecision.swift et al.)
// ---------------------------------------------------------------------------

/// Permission modes returnable from a feed permission notification action.
///
/// Port of `NotificationFeedPermissionMode` (`String` raw enum). Raw values are
/// preserved via [`NotificationFeedPermissionMode::raw_value`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationFeedPermissionMode {
    /// Allow the single requested action.
    Once,
    /// Allow the requested action for the current session.
    Always,
    /// Allow all matching actions.
    All,
    /// Bypass permissions for the current request family.
    Bypass,
    /// Deny the requested action.
    Deny,
}

impl NotificationFeedPermissionMode {
    /// The Swift `String` raw value for this mode.
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::All => "all",
            Self::Bypass => "bypass",
            Self::Deny => "deny",
        }
    }
}

/// Exit-plan modes returnable from a feed exit-plan notification action.
///
/// Port of `NotificationFeedExitPlanMode` (`String` raw enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationFeedExitPlanMode {
    /// Accept the plan through Ultraplan.
    Ultraplan,
    /// Bypass permissions while accepting the plan.
    BypassPermissions,
    /// Accept automatically.
    AutoAccept,
    /// Accept manually.
    Manual,
    /// Deny the plan.
    Deny,
}

impl NotificationFeedExitPlanMode {
    /// The Swift `String` raw value for this mode.
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Ultraplan => "ultraplan",
            Self::BypassPermissions => "bypassPermissions",
            Self::AutoAccept => "autoAccept",
            Self::Manual => "manual",
            Self::Deny => "deny",
        }
    }
}

/// A feed decision produced from an OS notification action.
///
/// Port of `NotificationFeedDecision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationFeedDecision {
    /// A permission decision.
    Permission(NotificationFeedPermissionMode),
    /// An exit-plan decision.
    ExitPlan(NotificationFeedExitPlanMode),
}

/// Permission actions supported by the pending feed request that owns a
/// notification response.
///
/// Port of `NotificationFeedPermissionCapabilities`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationFeedPermissionCapabilities {
    /// Whether the request supports the "once" permission mode.
    pub supports_once: bool,
    /// Whether the request supports the "always" permission mode.
    pub supports_always: bool,
    /// Whether the request supports the "all" permission mode.
    pub supports_all: bool,
}

// ---------------------------------------------------------------------------
// NotificationNavClickAction (NotificationNavClickAction.swift)
// ---------------------------------------------------------------------------

/// A notification click action the coordinator dispatches without knowing how
/// it is performed.
///
/// Port of `NotificationNavClickAction` (single case mirrors the app-target
/// `TerminalNotificationClickAction`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationNavClickAction {
    /// Reveal the file at `path` in Finder. Mirrors the app-target
    /// reveal-in-Finder action.
    RevealInFinder {
        /// The file path to reveal (preserved verbatim, untrimmed).
        path: String,
    },
}

impl NotificationNavClickAction {
    const KIND_USER_INFO_KEY: &'static str = "cmuxClickAction";
    const REVEAL_IN_FINDER_PATH_USER_INFO_KEY: &'static str = "cmuxRevealInFinderPath";
    const REVEAL_IN_FINDER_KIND: &'static str = "revealInFinder";

    /// Creates a click action from terminal notification `user_info`, preserving
    /// the app-target wire keys.
    ///
    /// Port of `NotificationNavClickAction.init?(userInfo:)`
    /// (`NotificationNavClickAction.swift:18-28`). The reveal path must be
    /// non-empty after trimming whitespace and newlines, but the *stored* value
    /// is the original untrimmed string (matching `self = .revealInFinder(path: path)`).
    pub fn from_user_info(user_info: &UserInfo) -> Option<Self> {
        let kind = user_info.get(Self::KIND_USER_INFO_KEY)?;
        match kind {
            Self::REVEAL_IN_FINDER_KIND => {
                let path = user_info.get(Self::REVEAL_IN_FINDER_PATH_USER_INFO_KEY)?;
                // Swift: `!path.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty`.
                if path.trim_matches(|c: char| c.is_whitespace()).is_empty() {
                    return None;
                }
                Some(Self::RevealInFinder {
                    path: path.to_string(),
                })
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Response value + typed userInfo (NotificationDeliveryResponse.swift)
// ---------------------------------------------------------------------------

/// Typed replacement for Swift's `[AnyHashable: Any]` `userInfo` at the app
/// boundary. Every coordinator read is `as? String`, so this stores string
/// values only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserInfo(BTreeMap<String, String>);

impl UserInfo {
    /// An empty `userInfo`.
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Inserts a key/value pair, returning `self` for chaining.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }

    /// Inserts a key/value pair.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), value.into());
    }

    /// Looks up a string value (mirrors `userInfo[key] as? String`).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}

impl<const N: usize> From<[(&str, &str); N]> for UserInfo {
    fn from(pairs: [(&str, &str); N]) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }
}

/// The flattened notification response the coordinator acts on.
///
/// Port of `NotificationDeliveryResponse` (already a plain value in Swift; its
/// `UNNotificationResponse` initializer is a boundary concern left to the app).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDeliveryResponse {
    /// `content.categoryIdentifier`.
    pub category_identifier: String,
    /// `response.actionIdentifier`.
    pub action_identifier: String,
    /// `request.identifier`.
    pub request_identifier: String,
    /// `content.userInfo`.
    pub user_info: UserInfo,
}

// ---------------------------------------------------------------------------
// Injected identifiers/titles (TerminalNotificationDeliveryIdentifiers.swift,
// NotificationDeliveryActionTitles.swift)
// ---------------------------------------------------------------------------

/// Stable terminal-notification identifiers supplied by the app target.
///
/// Port of `TerminalNotificationDeliveryIdentifiers`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalNotificationDeliveryIdentifiers {
    /// The category identifier used by terminal notifications.
    pub category_identifier: String,
    /// The explicit "show" action identifier used by terminal notifications.
    pub show_action_identifier: String,
}

/// Localized action titles used when composing OS notification categories.
///
/// Port of `NotificationDeliveryActionTitles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDeliveryActionTitles {
    /// Title for opening a terminal notification.
    pub show: String,
    /// Title for allowing a permission request once.
    pub feed_permission_allow_once: String,
    /// Title for allowing a permission request persistently.
    pub feed_permission_always: String,
    /// Title for allowing every matching permission request.
    pub feed_permission_all: String,
    /// Title for denying a permission request.
    pub feed_permission_deny: String,
    /// Title for accepting an exit plan with Ultraplan.
    pub feed_exit_plan_ultraplan: String,
    /// Title for accepting an exit plan manually.
    pub feed_exit_plan_manual: String,
    /// Title for accepting an exit plan automatically.
    pub feed_exit_plan_auto_accept: String,
    /// Title for opening a feed question in the app.
    pub feed_question_reply: String,
}

// ---------------------------------------------------------------------------
// Category composition data (replaces UNNotificationCategory/UNNotificationAction)
// ---------------------------------------------------------------------------

/// A single notification action, as data.
///
/// Replaces `UNNotificationAction`; only the option flags the coordinator
/// actually sets are modelled (`.destructive`, `.foreground`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationActionData {
    /// The action identifier.
    pub identifier: String,
    /// The localized action title.
    pub title: String,
    /// Whether the action carries `.destructive`.
    pub destructive: bool,
    /// Whether the action carries `.foreground`.
    pub foreground: bool,
}

impl NotificationActionData {
    fn plain(identifier: &str, title: &str) -> Self {
        Self {
            identifier: identifier.to_string(),
            title: title.to_string(),
            destructive: false,
            foreground: false,
        }
    }
}

/// A notification category, as data.
///
/// Replaces `UNNotificationCategory`; only the one category option the
/// coordinator sets is modelled (`.customDismissAction`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCategoryData {
    /// The category identifier.
    pub identifier: String,
    /// The category's ordered actions.
    pub actions: Vec<NotificationActionData>,
    /// Whether the category carries `.customDismissAction`.
    pub custom_dismiss_action: bool,
}

// ---------------------------------------------------------------------------
// Presentation options (:60-66)
// ---------------------------------------------------------------------------

/// Foreground presentation options.
///
/// Replaces `UNNotificationPresentationOptions`; the coordinator only ever
/// produces `.banner`, `.list`, and (conditionally) `.sound`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationOptions {
    /// The `.banner` option (always set).
    pub banner: bool,
    /// The `.list` option (always set).
    pub list: bool,
    /// The `.sound` option (set only when the notification has sound).
    pub sound: bool,
}

/// Presentation options for a notification delivered in the foreground.
///
/// Port of `presentationOptions(notificationHasSound:)`
/// (`NotificationDeliveryCoordinator.swift:60-66`): `[.banner, .list]` plus
/// `.sound` when the notification has sound.
pub fn presentation_options(notification_has_sound: bool) -> PresentationOptions {
    PresentationOptions {
        banner: true,
        list: true,
        sound: notification_has_sound,
    }
}

// ---------------------------------------------------------------------------
// Feed permission negotiation (:222-252)
// ---------------------------------------------------------------------------

/// Negotiates a feed permission decision against the request's capabilities.
///
/// Port of `feedPermissionNotificationDecision(requestId:requestedMode:)`
/// (`NotificationDeliveryCoordinator.swift:222-252`). The `requestId` lookup
/// seam is inverted: the caller resolves `capabilities` (via
/// `NotificationFeedReplying.permissionCapabilities`) and passes it in.
///
/// - `capabilities == None` ⇒ `Some(Permission(requested_mode))` (no pending
///   request to constrain the choice).
/// - `.once` requires `supports_once`, else `None` (consumed, no reply).
/// - `.always` falls back to `.once` when `supports_always` is false but
///   `supports_once` is true; otherwise requires `supports_always`, else `None`.
/// - `.all` requires `supports_all`, else `None`.
/// - any other mode with capabilities present ⇒ `Some(Permission(mode))`
///   (Swift `default:` arm; unreached by the three permission actions).
pub fn feed_permission_notification_decision(
    requested_mode: NotificationFeedPermissionMode,
    capabilities: Option<&NotificationFeedPermissionCapabilities>,
) -> Option<NotificationFeedDecision> {
    let Some(capabilities) = capabilities else {
        return Some(NotificationFeedDecision::Permission(requested_mode));
    };

    use NotificationFeedPermissionMode as Mode;
    match requested_mode {
        Mode::Once => {
            if capabilities.supports_once {
                Some(NotificationFeedDecision::Permission(Mode::Once))
            } else {
                None
            }
        }
        Mode::Always => {
            if capabilities.supports_always {
                Some(NotificationFeedDecision::Permission(Mode::Always))
            } else if capabilities.supports_once {
                Some(NotificationFeedDecision::Permission(Mode::Once))
            } else {
                None
            }
        }
        Mode::All => {
            if capabilities.supports_all {
                Some(NotificationFeedDecision::Permission(Mode::All))
            } else {
                None
            }
        }
        other => Some(NotificationFeedDecision::Permission(other)),
    }
}

// ---------------------------------------------------------------------------
// Response handling outcome (:68-73, :174-294)
// ---------------------------------------------------------------------------

/// The decision produced by [`NotificationDeliveryCore::handle`].
///
/// Each variant replaces a coordinator side-effect seam call, so the shell can
/// perform exactly one effect from pure data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No effect (Swift `break`/consumed-without-reply/failed-guard paths).
    None,
    /// `NotificationFeedReplying.deliverReply(requestId:decision:)` (`:190-210`).
    DeliverReply {
        /// The pending feed request id.
        request_id: String,
        /// The negotiated decision.
        decision: NotificationFeedDecision,
    },
    /// `NotificationApplicationActivating.activateApplication()` (`:212,215`).
    ActivateApp,
    /// `NotificationDeliveryTerminalNavigating.performClickAction(_:)`
    /// followed, on success, by `markNotificationRead(id:)` (`:268-273`).
    ///
    /// # Divergence (documented)
    /// Swift gates the `markNotificationRead` on the `Bool` returned by
    /// `performClickAction` (`if didPerform, let notificationId`). That runtime
    /// result is not observable in the pure core, so this variant carries the
    /// `notification_id` and the caller must mark it read *only if* the click
    /// action actually performs. The oracle test's fake returns `true`
    /// (`performSucceeds = true`), so the pinned expectation carries the id.
    PerformClickAction {
        /// The click action to perform.
        action: NotificationNavClickAction,
        /// The notification id to mark read on success, if resolvable.
        notification_id: Option<Uuid>,
    },
    /// `NotificationDeliveryTerminalNavigating.open(tabId:surfaceId:notificationId:)`
    /// (`:275`). The Swift `Bool` result is discarded (`_ =`).
    OpenTab {
        /// The target tab.
        tab_id: Uuid,
        /// The optional target surface.
        surface_id: Option<Uuid>,
        /// The optional notification id.
        notification_id: Option<Uuid>,
    },
    /// `NotificationDeliveryTerminalNavigating.markNotificationRead(id:)`
    /// (`:277-278`, dismiss path).
    MarkRead {
        /// The notification id to mark read.
        notification_id: Uuid,
    },
}

/// The stateful pure core: holds the app-supplied identifiers and titles and
/// mirrors the coordinator's non-shell methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDeliveryCore {
    identifiers: TerminalNotificationDeliveryIdentifiers,
    titles: NotificationDeliveryActionTitles,
}

impl NotificationDeliveryCore {
    /// Creates the delivery core from the app-supplied identifiers and titles.
    pub fn new(
        identifiers: TerminalNotificationDeliveryIdentifiers,
        titles: NotificationDeliveryActionTitles,
    ) -> Self {
        Self { identifiers, titles }
    }

    /// The feed permission category identifiers.
    ///
    /// Port of `feedPermissionNotificationCategoryIds()`
    /// (`NotificationDeliveryCoordinator.swift:160-172`).
    pub fn feed_permission_notification_category_ids() -> [&'static str; 9] {
        [
            "CMUXFeedPermission",
            "CMUXFeedPermissionDeny",
            "CMUXFeedPermissionOnce",
            "CMUXFeedPermissionAlways",
            "CMUXFeedPermissionAll",
            "CMUXFeedPermissionOnceAlways",
            "CMUXFeedPermissionOnceAll",
            "CMUXFeedPermissionAlwaysAll",
            "CMUXFeedPermissionOnceAlwaysAll",
        ]
    }

    /// Composes every terminal and feed notification category as data.
    ///
    /// Port of `notificationCategories()`
    /// (`NotificationDeliveryCoordinator.swift:75-158`). Swift returns a
    /// `Set<UNNotificationCategory>`; the identifiers are all unique, so this
    /// returns a `Vec` in the Swift construction order
    /// (`[terminal, exitPlan, question] + permissionCategories`).
    pub fn notification_categories(&self) -> Vec<NotificationCategoryData> {
        let titles = &self.titles;

        let terminal_category = NotificationCategoryData {
            identifier: self.identifiers.category_identifier.clone(),
            actions: vec![NotificationActionData::plain(
                &self.identifiers.show_action_identifier,
                &titles.show,
            )],
            custom_dismiss_action: true,
        };

        let permission_once =
            NotificationActionData::plain("feed.permission.once", &titles.feed_permission_allow_once);
        let permission_always =
            NotificationActionData::plain("feed.permission.always", &titles.feed_permission_always);
        let permission_all =
            NotificationActionData::plain("feed.permission.all", &titles.feed_permission_all);
        let permission_deny = NotificationActionData {
            identifier: "feed.permission.deny".to_string(),
            title: titles.feed_permission_deny.clone(),
            destructive: true,
            foreground: false,
        };

        let permission_categories =
            Self::feed_permission_notification_category_ids()
                .into_iter()
                .map(|category_id| {
                    let mut actions: Vec<NotificationActionData> = Vec::new();
                    if category_id.contains("Once") || category_id == "CMUXFeedPermission" {
                        actions.push(permission_once.clone());
                    }
                    if category_id.contains("Always") || category_id == "CMUXFeedPermission" {
                        actions.push(permission_always.clone());
                    }
                    if category_id.contains("All") {
                        actions.push(permission_all.clone());
                    }
                    actions.push(permission_deny.clone());
                    NotificationCategoryData {
                        identifier: category_id.to_string(),
                        actions,
                        custom_dismiss_action: false,
                    }
                });

        let exit_plan_category = NotificationCategoryData {
            identifier: "CMUXFeedExitPlan".to_string(),
            actions: vec![
                NotificationActionData::plain(
                    "feed.exit_plan.ultraplan",
                    &titles.feed_exit_plan_ultraplan,
                ),
                NotificationActionData::plain("feed.exit_plan.manual", &titles.feed_exit_plan_manual),
                NotificationActionData::plain(
                    "feed.exit_plan.autoAccept",
                    &titles.feed_exit_plan_auto_accept,
                ),
            ],
            custom_dismiss_action: false,
        };
        let question_category = NotificationCategoryData {
            identifier: "CMUXFeedQuestion".to_string(),
            actions: vec![NotificationActionData {
                identifier: "feed.question.open".to_string(),
                title: titles.feed_question_reply.clone(),
                destructive: false,
                foreground: true,
            }],
            custom_dismiss_action: false,
        };

        let mut categories = vec![terminal_category, exit_plan_category, question_category];
        categories.extend(permission_categories);
        categories
    }

    /// Handles a notification response, returning the effect to perform.
    ///
    /// Port of `handle(_:)` (`NotificationDeliveryCoordinator.swift:68-73`):
    /// feed responses are tried first, and terminal routing runs only when the
    /// response is not a feed response. `permission_capabilities` inverts the
    /// `NotificationFeedReplying.permissionCapabilities(requestId:)` read seam.
    pub fn handle<F>(
        &self,
        response: &NotificationDeliveryResponse,
        permission_capabilities: F,
    ) -> Outcome
    where
        F: Fn(&str) -> Option<NotificationFeedPermissionCapabilities>,
    {
        if let Some(outcome) = handle_feed(response, &permission_capabilities) {
            return outcome;
        }
        self.handle_terminal(response)
    }

    /// Terminal-notification routing.
    ///
    /// Port of `handleTerminalNotificationResponse(_:)`
    /// (`NotificationDeliveryCoordinator.swift:254-283`).
    fn handle_terminal(&self, response: &NotificationDeliveryResponse) -> Outcome {
        let action = response.action_identifier.as_str();
        if action == DEFAULT_ACTION_IDENTIFIER
            || action == self.identifiers.show_action_identifier
        {
            // `guard let tabId ...` — bail before the click-action check.
            let Some(tab_id) = response
                .user_info
                .get("tabId")
                .and_then(parse_notification_uuid)
            else {
                return Outcome::None;
            };
            let surface_id = response
                .user_info
                .get("surfaceId")
                .and_then(parse_notification_uuid);
            let notification_id = notification_id(response);
            if let Some(action) = NotificationNavClickAction::from_user_info(&response.user_info) {
                return Outcome::PerformClickAction {
                    action,
                    notification_id,
                };
            }
            Outcome::OpenTab {
                tab_id,
                surface_id,
                notification_id,
            }
        } else if action == DISMISS_ACTION_IDENTIFIER {
            match notification_id(response) {
                Some(notification_id) => Outcome::MarkRead { notification_id },
                None => Outcome::None,
            }
        } else {
            Outcome::None
        }
    }
}

/// Feed-notification routing.
///
/// Port of `handleFeedNotificationResponse(_:)`
/// (`NotificationDeliveryCoordinator.swift:174-220`). Returns `None` when the
/// response is not a feed response (Swift `return false`, fall through to
/// terminal routing); `Some(Outcome::None)` when the feed path consumes the
/// response without an effect (Swift `return true` with no seam call).
fn handle_feed<F>(response: &NotificationDeliveryResponse, permission_capabilities: &F) -> Option<Outcome>
where
    F: Fn(&str) -> Option<NotificationFeedPermissionCapabilities>,
{
    let category = response.category_identifier.as_str();
    let is_feed = category.starts_with("CMUXFeedPermission")
        || category == "CMUXFeedExitPlan"
        || category == "CMUXFeedQuestion";
    if !is_feed {
        return None;
    }

    // `guard let requestId = userInfo["requestId"] as? String else { return true }`.
    let Some(request_id) = response.user_info.get("requestId").map(str::to_string) else {
        return Some(Outcome::None);
    };

    use NotificationFeedExitPlanMode as Exit;
    use NotificationFeedPermissionMode as Perm;
    let outcome = match response.action_identifier.as_str() {
        "feed.permission.once" => {
            permission_reply(request_id, Perm::Once, permission_capabilities)
        }
        "feed.permission.always" => {
            permission_reply(request_id, Perm::Always, permission_capabilities)
        }
        "feed.permission.all" => {
            permission_reply(request_id, Perm::All, permission_capabilities)
        }
        "feed.permission.deny" => Outcome::DeliverReply {
            request_id,
            decision: NotificationFeedDecision::Permission(Perm::Deny),
        },
        "feed.exit_plan.ultraplan" => Outcome::DeliverReply {
            request_id,
            decision: NotificationFeedDecision::ExitPlan(Exit::Ultraplan),
        },
        "feed.exit_plan.bypassPermissions" => Outcome::DeliverReply {
            request_id,
            decision: NotificationFeedDecision::ExitPlan(Exit::BypassPermissions),
        },
        "feed.exit_plan.autoAccept" => Outcome::DeliverReply {
            request_id,
            decision: NotificationFeedDecision::ExitPlan(Exit::AutoAccept),
        },
        "feed.exit_plan.manual" => Outcome::DeliverReply {
            request_id,
            decision: NotificationFeedDecision::ExitPlan(Exit::Manual),
        },
        "feed.question.open" => Outcome::ActivateApp,
        other => {
            if other == DEFAULT_ACTION_IDENTIFIER || other == DISMISS_ACTION_IDENTIFIER {
                Outcome::ActivateApp
            } else {
                Outcome::None
            }
        }
    };
    Some(outcome)
}

/// Resolves a permission decision and wraps it as a reply, or `Outcome::None`
/// when the mode is unsupported (Swift `guard let decision ... else return true`).
fn permission_reply<F>(
    request_id: String,
    mode: NotificationFeedPermissionMode,
    permission_capabilities: &F,
) -> Outcome
where
    F: Fn(&str) -> Option<NotificationFeedPermissionCapabilities>,
{
    let capabilities = permission_capabilities(&request_id);
    match feed_permission_notification_decision(mode, capabilities.as_ref()) {
        Some(decision) => Outcome::DeliverReply {
            request_id,
            decision,
        },
        None => Outcome::None,
    }
}

/// Resolves the notification id for a response.
///
/// Port of `notificationId(_:)`
/// (`NotificationDeliveryCoordinator.swift:285-294`): prefer the request
/// identifier parsed as a UUID, else the `notificationId` `userInfo` string.
fn notification_id(response: &NotificationDeliveryResponse) -> Option<Uuid> {
    if let Some(id) = parse_notification_uuid(&response.request_identifier) {
        return Some(id);
    }
    response
        .user_info
        .get("notificationId")
        .and_then(parse_notification_uuid)
}

/// Parses a UUID with Swift `UUID(uuidString:)` strictness.
///
/// Swift's initializer accepts *only* the 36-character hyphenated
/// `8-4-4-4-12` form (case-insensitive). `uuid::Uuid::parse_str` is more
/// lenient (it also accepts the 32-char simple, braced, and URN forms), so the
/// length guard restores parity: only the hyphenated form is 36 characters.
fn parse_notification_uuid(value: &str) -> Option<Uuid> {
    if value.len() != 36 {
        return None;
    }
    Uuid::parse_str(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identifiers() -> TerminalNotificationDeliveryIdentifiers {
        TerminalNotificationDeliveryIdentifiers {
            category_identifier: "terminal.category".to_string(),
            show_action_identifier: "terminal.show".to_string(),
        }
    }

    fn titles() -> NotificationDeliveryActionTitles {
        NotificationDeliveryActionTitles {
            show: "Show".to_string(),
            feed_permission_allow_once: "Allow Once".to_string(),
            feed_permission_always: "Always".to_string(),
            feed_permission_all: "All tools".to_string(),
            feed_permission_deny: "Deny".to_string(),
            feed_exit_plan_ultraplan: "Ultraplan".to_string(),
            feed_exit_plan_manual: "Manual".to_string(),
            feed_exit_plan_auto_accept: "Auto".to_string(),
            feed_question_reply: "Reply".to_string(),
        }
    }

    fn core() -> NotificationDeliveryCore {
        NotificationDeliveryCore::new(identifiers(), titles())
    }

    fn no_capabilities(_: &str) -> Option<NotificationFeedPermissionCapabilities> {
        None
    }

    fn categories_by_id(
        core: &NotificationDeliveryCore,
    ) -> std::collections::HashMap<String, NotificationCategoryData> {
        core.notification_categories()
            .into_iter()
            .map(|c| (c.identifier.clone(), c))
            .collect()
    }

    fn action_ids(category: &NotificationCategoryData) -> Vec<&str> {
        category.actions.iter().map(|a| a.identifier.as_str()).collect()
    }

    // --- configure installs terminal and Feed categories (oracle, :82-119) ---
    // The delegate assignment is a dropped shell side effect; we assert the
    // composed category data instead.
    #[test]
    fn configure_installs_categories() {
        let core = core();
        let categories = categories_by_id(&core);

        let terminal = &categories["terminal.category"];
        assert_eq!(action_ids(terminal), ["terminal.show"]);
        assert_eq!(
            terminal.actions.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(),
            ["Show"]
        );
        assert!(terminal.custom_dismiss_action);

        let full_permission = &categories["CMUXFeedPermissionOnceAlwaysAll"];
        assert_eq!(
            action_ids(full_permission),
            [
                "feed.permission.once",
                "feed.permission.always",
                "feed.permission.all",
                "feed.permission.deny",
            ]
        );
        assert!(full_permission.actions.last().unwrap().destructive);

        let deny_only = &categories["CMUXFeedPermissionDeny"];
        assert_eq!(action_ids(deny_only), ["feed.permission.deny"]);

        let exit_plan = &categories["CMUXFeedExitPlan"];
        assert_eq!(
            action_ids(exit_plan),
            [
                "feed.exit_plan.ultraplan",
                "feed.exit_plan.manual",
                "feed.exit_plan.autoAccept",
            ]
        );

        let question = &categories["CMUXFeedQuestion"];
        assert_eq!(action_ids(question), ["feed.question.open"]);
        assert!(question.actions.first().unwrap().foreground);
    }

    // --- category composition table for all nine permission ids (:105-172) ---
    #[test]
    fn permission_category_action_sets() {
        let core = core();
        let categories = categories_by_id(&core);
        let cases: [(&str, &[&str]); 9] = [
            ("CMUXFeedPermission", &["feed.permission.once", "feed.permission.always", "feed.permission.deny"]),
            ("CMUXFeedPermissionDeny", &["feed.permission.deny"]),
            ("CMUXFeedPermissionOnce", &["feed.permission.once", "feed.permission.deny"]),
            ("CMUXFeedPermissionAlways", &["feed.permission.always", "feed.permission.deny"]),
            ("CMUXFeedPermissionAll", &["feed.permission.all", "feed.permission.deny"]),
            ("CMUXFeedPermissionOnceAlways", &["feed.permission.once", "feed.permission.always", "feed.permission.deny"]),
            ("CMUXFeedPermissionOnceAll", &["feed.permission.once", "feed.permission.all", "feed.permission.deny"]),
            ("CMUXFeedPermissionAlwaysAll", &["feed.permission.always", "feed.permission.all", "feed.permission.deny"]),
            ("CMUXFeedPermissionOnceAlwaysAll", &["feed.permission.once", "feed.permission.always", "feed.permission.all", "feed.permission.deny"]),
        ];
        for (id, expected) in cases {
            assert_eq!(action_ids(&categories[id]), expected, "category {id}");
        }
    }

    // --- presentation options (oracle, :121-134) ---
    #[test]
    fn presentation_options_include_sound_only_with_sound() {
        let quiet = presentation_options(false);
        assert!(quiet.banner);
        assert!(quiet.list);
        assert!(!quiet.sound);

        let audible = presentation_options(true);
        assert!(audible.banner);
        assert!(audible.list);
        assert!(audible.sound);
    }

    // --- feed permission negotiation table (:222-252) ---
    #[test]
    fn feed_permission_decision_table() {
        use NotificationFeedDecision::Permission;
        use NotificationFeedPermissionMode as M;

        // No capabilities => passthrough of the requested mode.
        assert_eq!(
            feed_permission_notification_decision(M::Once, None),
            Some(Permission(M::Once))
        );
        assert_eq!(
            feed_permission_notification_decision(M::Bypass, None),
            Some(Permission(M::Bypass))
        );

        let caps = |once, always, all| NotificationFeedPermissionCapabilities {
            supports_once: once,
            supports_always: always,
            supports_all: all,
        };

        // once
        assert_eq!(
            feed_permission_notification_decision(M::Once, Some(&caps(true, false, false))),
            Some(Permission(M::Once))
        );
        assert_eq!(
            feed_permission_notification_decision(M::Once, Some(&caps(false, true, true))),
            None
        );

        // always -> falls back to once, else self, else none
        assert_eq!(
            feed_permission_notification_decision(M::Always, Some(&caps(false, true, false))),
            Some(Permission(M::Always))
        );
        assert_eq!(
            feed_permission_notification_decision(M::Always, Some(&caps(true, false, false))),
            Some(Permission(M::Once))
        );
        assert_eq!(
            feed_permission_notification_decision(M::Always, Some(&caps(false, false, false))),
            None
        );

        // all
        assert_eq!(
            feed_permission_notification_decision(M::All, Some(&caps(false, false, true))),
            Some(Permission(M::All))
        );
        assert_eq!(
            feed_permission_notification_decision(M::All, Some(&caps(true, true, false))),
            None
        );

        // default arm (with capabilities present) passes the mode through
        assert_eq!(
            feed_permission_notification_decision(M::Deny, Some(&caps(false, false, false))),
            Some(Permission(M::Deny))
        );
    }

    // --- oracle: always falls back to once when always unsupported (:136-154) ---
    #[test]
    fn feed_permission_always_falls_back_to_once() {
        let core = core();
        let caps = |id: &str| {
            (id == "req-1").then_some(NotificationFeedPermissionCapabilities {
                supports_once: true,
                supports_always: false,
                supports_all: false,
            })
        };
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "CMUXFeedPermissionOnceAlways".to_string(),
                action_identifier: "feed.permission.always".to_string(),
                request_identifier: "feed.req-1".to_string(),
                user_info: UserInfo::from([("requestId", "req-1")]),
            },
            caps,
        );
        assert_eq!(
            outcome,
            Outcome::DeliverReply {
                request_id: "req-1".to_string(),
                decision: NotificationFeedDecision::Permission(NotificationFeedPermissionMode::Once),
            }
        );
    }

    // --- oracle: unsupported mode consumed without reply (:156-174) ---
    #[test]
    fn feed_permission_unsupported_mode_does_not_reply() {
        let core = core();
        let caps = |id: &str| {
            (id == "req-1").then_some(NotificationFeedPermissionCapabilities {
                supports_once: true,
                supports_always: true,
                supports_all: false,
            })
        };
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "CMUXFeedPermissionAll".to_string(),
                action_identifier: "feed.permission.all".to_string(),
                request_identifier: "feed.req-1".to_string(),
                user_info: UserInfo::from([("requestId", "req-1")]),
            },
            caps,
        );
        assert_eq!(outcome, Outcome::None);
    }

    // --- oracle: feed response without request id is consumed (:176-191) ---
    #[test]
    fn feed_missing_request_id_is_consumed() {
        let core = core();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "CMUXFeedQuestion".to_string(),
                action_identifier: DEFAULT_ACTION_IDENTIFIER.to_string(),
                request_identifier: "feed.missing".to_string(),
                user_info: UserInfo::from([("tabId", "1D9C0E90-1111-2222-3333-444455556666")]),
            },
            no_capabilities,
        );
        // Consumed by the feed path (not routed to terminal open).
        assert_eq!(outcome, Outcome::None);
    }

    // --- oracle: feed question default response activates the app (:193-206) ---
    #[test]
    fn feed_question_default_activates_app() {
        let core = core();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "CMUXFeedQuestion".to_string(),
                action_identifier: DEFAULT_ACTION_IDENTIFIER.to_string(),
                request_identifier: "feed.req-2".to_string(),
                user_info: UserInfo::from([("requestId", "req-2")]),
            },
            no_capabilities,
        );
        assert_eq!(outcome, Outcome::ActivateApp);
    }

    // --- exit-plan actions deliver exit-plan replies (:203-210) ---
    #[test]
    fn feed_exit_plan_actions_deliver_replies() {
        let core = core();
        let cases = [
            ("feed.exit_plan.ultraplan", NotificationFeedExitPlanMode::Ultraplan),
            ("feed.exit_plan.bypassPermissions", NotificationFeedExitPlanMode::BypassPermissions),
            ("feed.exit_plan.autoAccept", NotificationFeedExitPlanMode::AutoAccept),
            ("feed.exit_plan.manual", NotificationFeedExitPlanMode::Manual),
        ];
        for (action, mode) in cases {
            let outcome = core.handle(
                &NotificationDeliveryResponse {
                    category_identifier: "CMUXFeedExitPlan".to_string(),
                    action_identifier: action.to_string(),
                    request_identifier: "feed.req-3".to_string(),
                    user_info: UserInfo::from([("requestId", "req-3")]),
                },
                no_capabilities,
            );
            assert_eq!(
                outcome,
                Outcome::DeliverReply {
                    request_id: "req-3".to_string(),
                    decision: NotificationFeedDecision::ExitPlan(mode),
                },
                "action {action}"
            );
        }
    }

    // --- feed.permission.deny delivers a deny reply (:201-202) ---
    #[test]
    fn feed_permission_deny_delivers_deny() {
        let core = core();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "CMUXFeedPermission".to_string(),
                action_identifier: "feed.permission.deny".to_string(),
                request_identifier: "feed.req-4".to_string(),
                user_info: UserInfo::from([("requestId", "req-4")]),
            },
            no_capabilities,
        );
        assert_eq!(
            outcome,
            Outcome::DeliverReply {
                request_id: "req-4".to_string(),
                decision: NotificationFeedDecision::Permission(NotificationFeedPermissionMode::Deny),
            }
        );
    }

    // --- unknown feed action is consumed with no effect (:216-217) ---
    #[test]
    fn feed_unknown_action_is_consumed() {
        let core = core();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "CMUXFeedPermission".to_string(),
                action_identifier: "feed.permission.unknown".to_string(),
                request_identifier: "feed.req-5".to_string(),
                user_info: UserInfo::from([("requestId", "req-5")]),
            },
            no_capabilities,
        );
        assert_eq!(outcome, Outcome::None);
    }

    // --- oracle: terminal default click action performs and marks read (:208-229) ---
    #[test]
    fn terminal_default_click_action_performs_and_marks_read() {
        let core = core();
        let notification_id = Uuid::new_v4();
        let tab_id = Uuid::new_v4();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "terminal.category".to_string(),
                action_identifier: DEFAULT_ACTION_IDENTIFIER.to_string(),
                request_identifier: notification_id.to_string(),
                user_info: UserInfo::from([
                    ("tabId", tab_id.to_string().as_str()),
                    ("cmuxClickAction", "revealInFinder"),
                    ("cmuxRevealInFinderPath", "/tmp/report.txt"),
                ]),
            },
            no_capabilities,
        );
        assert_eq!(
            outcome,
            Outcome::PerformClickAction {
                action: NotificationNavClickAction::RevealInFinder {
                    path: "/tmp/report.txt".to_string()
                },
                notification_id: Some(notification_id),
            }
        );
    }

    // --- oracle: terminal default opens tab/surface w/ notificationId fallback (:231-252) ---
    #[test]
    fn terminal_default_opens_target() {
        let core = core();
        let tab_id = Uuid::new_v4();
        let surface_id = Uuid::new_v4();
        let notification_id = Uuid::new_v4();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "terminal.category".to_string(),
                action_identifier: "terminal.show".to_string(),
                request_identifier: "not-a-uuid".to_string(),
                user_info: UserInfo::from([
                    ("tabId", tab_id.to_string().as_str()),
                    ("surfaceId", surface_id.to_string().as_str()),
                    ("notificationId", notification_id.to_string().as_str()),
                ]),
            },
            no_capabilities,
        );
        assert_eq!(
            outcome,
            Outcome::OpenTab {
                tab_id,
                surface_id: Some(surface_id),
                notification_id: Some(notification_id),
            }
        );
    }

    // --- terminal default with missing/invalid tab id bails (:257-260) ---
    #[test]
    fn terminal_default_without_tab_id_is_none() {
        let core = core();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "terminal.category".to_string(),
                action_identifier: DEFAULT_ACTION_IDENTIFIER.to_string(),
                request_identifier: Uuid::new_v4().to_string(),
                // Click action present but tabId guard fails first.
                user_info: UserInfo::from([
                    ("cmuxClickAction", "revealInFinder"),
                    ("cmuxRevealInFinderPath", "/tmp/x"),
                ]),
            },
            no_capabilities,
        );
        assert_eq!(outcome, Outcome::None);
    }

    // --- oracle: terminal dismiss marks read via request identifier (:254-270) ---
    #[test]
    fn terminal_dismiss_marks_read() {
        let core = core();
        let notification_id = Uuid::new_v4();
        let tab_id = Uuid::new_v4();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "terminal.category".to_string(),
                action_identifier: DISMISS_ACTION_IDENTIFIER.to_string(),
                request_identifier: notification_id.to_string(),
                user_info: UserInfo::from([("tabId", tab_id.to_string().as_str())]),
            },
            no_capabilities,
        );
        assert_eq!(outcome, Outcome::MarkRead { notification_id });
    }

    // --- oracle: terminal dismiss marks read without a tab id (:272-287) ---
    #[test]
    fn terminal_dismiss_marks_read_without_tab_id() {
        let core = core();
        let notification_id = Uuid::new_v4();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "terminal.category".to_string(),
                action_identifier: DISMISS_ACTION_IDENTIFIER.to_string(),
                request_identifier: notification_id.to_string(),
                user_info: UserInfo::new(),
            },
            no_capabilities,
        );
        assert_eq!(outcome, Outcome::MarkRead { notification_id });
    }

    // --- terminal dismiss with unresolvable id yields no effect (:277-279) ---
    #[test]
    fn terminal_dismiss_without_resolvable_id_is_none() {
        let core = core();
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "terminal.category".to_string(),
                action_identifier: DISMISS_ACTION_IDENTIFIER.to_string(),
                request_identifier: "not-a-uuid".to_string(),
                user_info: UserInfo::new(),
            },
            no_capabilities,
        );
        assert_eq!(outcome, Outcome::None);
    }

    // --- notificationId fallback resolution (:285-294) ---
    #[test]
    fn notification_id_falls_back_to_user_info() {
        let fallback = Uuid::new_v4();
        let response = NotificationDeliveryResponse {
            category_identifier: "terminal.category".to_string(),
            action_identifier: "terminal.show".to_string(),
            request_identifier: "not-a-uuid".to_string(),
            user_info: UserInfo::from([("notificationId", fallback.to_string().as_str())]),
        };
        assert_eq!(notification_id(&response), Some(fallback));

        // Request identifier wins when it parses.
        let primary = Uuid::new_v4();
        let response = NotificationDeliveryResponse {
            request_identifier: primary.to_string(),
            user_info: UserInfo::from([("notificationId", fallback.to_string().as_str())]),
            ..response
        };
        assert_eq!(notification_id(&response), Some(primary));

        // Neither resolvable.
        let response = NotificationDeliveryResponse {
            request_identifier: "nope".to_string(),
            user_info: UserInfo::new(),
            ..response
        };
        assert_eq!(notification_id(&response), None);
    }

    // --- NavClickAction parsing edge cases (NotificationNavClickAction.swift:18-28) ---
    #[test]
    fn nav_click_action_parsing() {
        // Missing kind.
        assert_eq!(
            NotificationNavClickAction::from_user_info(&UserInfo::new()),
            None
        );
        // Unknown kind.
        assert_eq!(
            NotificationNavClickAction::from_user_info(&UserInfo::from([(
                "cmuxClickAction",
                "openURL"
            )])),
            None
        );
        // Missing path.
        assert_eq!(
            NotificationNavClickAction::from_user_info(&UserInfo::from([(
                "cmuxClickAction",
                "revealInFinder"
            )])),
            None
        );
        // Whitespace-only path is rejected.
        assert_eq!(
            NotificationNavClickAction::from_user_info(&UserInfo::from([
                ("cmuxClickAction", "revealInFinder"),
                ("cmuxRevealInFinderPath", "   \n\t "),
            ])),
            None
        );
        // Valid, and the surrounding whitespace is preserved (Swift stores the
        // original untrimmed path).
        assert_eq!(
            NotificationNavClickAction::from_user_info(&UserInfo::from([
                ("cmuxClickAction", "revealInFinder"),
                ("cmuxRevealInFinderPath", "  /tmp/report.txt  "),
            ])),
            Some(NotificationNavClickAction::RevealInFinder {
                path: "  /tmp/report.txt  ".to_string()
            })
        );
    }

    // --- non-feed category falls through to terminal routing (:174-179) ---
    #[test]
    fn non_feed_category_falls_through_to_terminal() {
        let core = core();
        let tab_id = Uuid::new_v4();
        // A category that is neither terminal nor feed, with the terminal show
        // action, still routes through terminal handling.
        let outcome = core.handle(
            &NotificationDeliveryResponse {
                category_identifier: "some.other.category".to_string(),
                action_identifier: "terminal.show".to_string(),
                request_identifier: "not-a-uuid".to_string(),
                user_info: UserInfo::from([("tabId", tab_id.to_string().as_str())]),
            },
            no_capabilities,
        );
        assert_eq!(
            outcome,
            Outcome::OpenTab {
                tab_id,
                surface_id: None,
                notification_id: None,
            }
        );
    }

    // --- strict UUID parsing rejects non-hyphenated forms (Swift UUID(uuidString:)) ---
    #[test]
    fn parse_uuid_is_strict() {
        let id = Uuid::new_v4();
        // Hyphenated form parses.
        assert_eq!(parse_notification_uuid(&id.to_string()), Some(id));
        // Simple (32-char) form is rejected, matching Swift.
        assert_eq!(parse_notification_uuid(&id.simple().to_string()), None);
        // Braced form is rejected.
        assert_eq!(parse_notification_uuid(&format!("{{{id}}}")), None);
    }
}
