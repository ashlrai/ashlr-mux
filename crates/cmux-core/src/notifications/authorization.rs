//! Notification authorization state and the cached delivery decision.
//!
//! Ported from `cmux/Sources/TerminalNotificationStore.swift` 113-144
//! (`NotificationAuthorizationState`) and
//! `cmux/Sources/TerminalNotificationQueue.swift` 421-454
//! (`cachedDeliveryAuthorizationDecision` + `fallbackEffects`).
//!
//! All OS interaction is excluded: mapping a live `UNAuthorizationStatus`, the
//! `NSApp.isActive` read, and playing the fallback sound are the delivery seam.
//! These are the pure decisions the seam consumes. `statusLabel` returns the
//! same English strings as Swift; localization is the host's concern.

use super::policy::TerminalNotificationPolicyEffects;

/// The app's current notification authorization, decoupled from the OS
/// `UNAuthorizationStatus` enum.
///
/// Verbatim port of Swift `NotificationAuthorizationState`
/// (`TerminalNotificationStore.swift` 113-144).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationAuthorizationState {
    Unknown,
    NotDetermined,
    Authorized,
    Denied,
    Provisional,
    Ephemeral,
}

impl NotificationAuthorizationState {
    /// Whether the OS will deliver a banner in this state.
    ///
    /// Verbatim port of Swift `allowsDelivery`
    /// (`TerminalNotificationStore.swift` 136-143).
    pub fn allows_delivery(self) -> bool {
        matches!(
            self,
            Self::Authorized | Self::Provisional | Self::Ephemeral
        )
    }

    /// The settings-row status label.
    ///
    /// Verbatim port of Swift `statusLabel`
    /// (`TerminalNotificationStore.swift` 121-134). Returns the English text;
    /// the host localizes.
    pub fn status_label(self) -> &'static str {
        match self {
            Self::Unknown | Self::NotDetermined => "Not Requested",
            Self::Authorized => "Allowed",
            Self::Denied => "Denied",
            Self::Provisional => "Deliver Quietly",
            Self::Ephemeral => "Temporary",
        }
    }
}

/// The cached delivery decision for a state without re-querying the OS:
/// `Some(false)` forces suppression, `Some(true)` forces delivery, `None` means
/// "no cached answer — fall through to the live authorization query".
///
/// Verbatim port of Swift `cachedDeliveryAuthorizationDecision(for:isAppActive:)`
/// (`TerminalNotificationQueue.swift` 421-435). Only `.notDetermined` consults
/// `is_app_active` (defer the first prompt while inactive).
pub fn cached_delivery_authorization_decision(
    state: NotificationAuthorizationState,
    is_app_active: bool,
) -> Option<bool> {
    match state {
        NotificationAuthorizationState::Authorized
        | NotificationAuthorizationState::Provisional
        | NotificationAuthorizationState::Ephemeral => None,
        NotificationAuthorizationState::Denied => Some(false),
        NotificationAuthorizationState::NotDetermined => {
            if is_app_active {
                None
            } else {
                Some(false)
            }
        }
        NotificationAuthorizationState::Unknown => None,
    }
}

/// Effects for the out-of-band fallback path (cmux plays feedback itself because
/// the OS will not deliver the banner). A user who explicitly denied cmux
/// notifications asked for silence, so the fallback sound is stripped for
/// `.denied` — and only `.denied`.
///
/// Verbatim port of Swift `fallbackEffects(_:authorizationState:)`
/// (`TerminalNotificationQueue.swift` 446-454).
pub fn fallback_effects(
    effects: TerminalNotificationPolicyEffects,
    authorization_state: NotificationAuthorizationState,
) -> TerminalNotificationPolicyEffects {
    if authorization_state != NotificationAuthorizationState::Denied {
        return effects;
    }
    let mut silenced = effects;
    silenced.sound = false;
    silenced
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: `testNotificationAuthorizationStateDeliveryCapability`.
    #[test]
    fn allows_delivery_matches_swift() {
        assert!(!NotificationAuthorizationState::Unknown.allows_delivery());
        assert!(!NotificationAuthorizationState::NotDetermined.allows_delivery());
        assert!(!NotificationAuthorizationState::Denied.allows_delivery());
        assert!(NotificationAuthorizationState::Authorized.allows_delivery());
        assert!(NotificationAuthorizationState::Provisional.allows_delivery());
        assert!(NotificationAuthorizationState::Ephemeral.allows_delivery());
    }

    #[test]
    fn status_label_matches_swift() {
        assert_eq!(
            NotificationAuthorizationState::Unknown.status_label(),
            "Not Requested"
        );
        assert_eq!(
            NotificationAuthorizationState::NotDetermined.status_label(),
            "Not Requested"
        );
        assert_eq!(
            NotificationAuthorizationState::Authorized.status_label(),
            "Allowed"
        );
        assert_eq!(
            NotificationAuthorizationState::Denied.status_label(),
            "Denied"
        );
        assert_eq!(
            NotificationAuthorizationState::Provisional.status_label(),
            "Deliver Quietly"
        );
        assert_eq!(
            NotificationAuthorizationState::Ephemeral.status_label(),
            "Temporary"
        );
    }

    /// Oracle: `testNotificationDeliveryAuthorizationUsesCachedTerminalStates`.
    #[test]
    fn cached_delivery_decision_matrix() {
        use NotificationAuthorizationState::*;
        assert_eq!(cached_delivery_authorization_decision(Unknown, false), None);
        assert_eq!(
            cached_delivery_authorization_decision(NotDetermined, true),
            None
        );
        assert_eq!(
            cached_delivery_authorization_decision(NotDetermined, false),
            Some(false)
        );
        assert_eq!(
            cached_delivery_authorization_decision(Denied, false),
            Some(false)
        );
        assert_eq!(
            cached_delivery_authorization_decision(Authorized, false),
            None
        );
        assert_eq!(
            cached_delivery_authorization_decision(Provisional, false),
            None
        );
        assert_eq!(
            cached_delivery_authorization_decision(Ephemeral, false),
            None
        );
    }

    fn effects_with_sound() -> TerminalNotificationPolicyEffects {
        TerminalNotificationPolicyEffects {
            sound: true,
            ..TerminalNotificationPolicyEffects::default()
        }
    }

    /// Oracle: `deniedAuthorizationStripsFallbackSound`.
    #[test]
    fn denied_strips_fallback_sound() {
        let denied =
            fallback_effects(effects_with_sound(), NotificationAuthorizationState::Denied);
        assert!(!denied.sound);
    }

    /// Oracle: `deniedAuthorizationLeavesOtherEffectsIntact`.
    #[test]
    fn denied_leaves_other_effects_intact() {
        let effects = effects_with_sound();
        let denied = fallback_effects(effects, NotificationAuthorizationState::Denied);
        assert_eq!(denied.command, effects.command);
        assert_eq!(denied.record, effects.record);
        assert_eq!(denied.desktop, effects.desktop);
        assert_eq!(denied.mark_unread, effects.mark_unread);
    }

    /// Oracle: `otherAuthorizationStatesKeepFallbackSound`.
    #[test]
    fn other_states_keep_fallback_sound() {
        use NotificationAuthorizationState::*;
        for state in [NotDetermined, Unknown, Authorized, Provisional, Ephemeral] {
            assert!(fallback_effects(effects_with_sound(), state).sound);
        }
    }
}
