//! Pure notification-policy data models and the delivery-gating decision.
//!
//! Ported from `cmux/Sources/TerminalNotificationPolicy.swift` (the envelope /
//! payload / context / effects serde shapes + patch-merge) and the gating
//! predicates extracted from `TerminalNotificationStore.swift`
//! (`shouldSuppressExternalDelivery`, `deliverNotificationSideEffects`,
//! `hasAnyNotificationEffect`, lines 1062-1300).
//!
//! Everything here is pure: no OS delivery, no subprocess hook engine. The
//! `posix_spawn` hook engine (`NotificationHookProcessRun`) is M8
//! process-supervisor territory and is intentionally excluded — only the
//! envelope JSON patch-merge data model is ported.

use serde::{Deserialize, Deserializer, Serialize};

/// The notification payload an external policy hook can rewrite.
///
/// Mirrors Swift `TerminalNotificationPolicyPayload`
/// (`TerminalNotificationPolicy.swift` 5-11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalNotificationPolicyPayload {
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
    #[serde(rename = "surfaceId", default, skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub body: String,
}

/// The read-only context handed to a policy hook.
///
/// Mirrors Swift `TerminalNotificationPolicyContext`
/// (`TerminalNotificationPolicy.swift` 13-19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalNotificationPolicyContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(rename = "configPath", default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(rename = "hookId", default, skip_serializing_if = "Option::is_none")]
    pub hook_id: Option<String>,
    #[serde(rename = "appFocused")]
    pub app_focused: bool,
    #[serde(rename = "focusedPanel")]
    pub focused_panel: bool,
}

/// The seven notification effect toggles. Every field defaults to `true`, and a
/// missing key in decoded JSON also resolves to `true` — matching Swift's
/// `decodeIfPresent(...) ?? true` semantics (`TerminalNotificationPolicy.swift`
/// 21-52).
///
/// DEVIATION: an explicit JSON `null` for one of these bools is a decode error
/// here (a Rust `bool` cannot be `null`), whereas Swift's `decodeIfPresent`
/// would treat `null` as absent and fall back to `true`. Hooks emit either the
/// key with a boolean or omit it entirely, so this edge is unreachable in
/// practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalNotificationPolicyEffects {
    pub record: bool,
    #[serde(rename = "markUnread")]
    pub mark_unread: bool,
    #[serde(rename = "reorderWorkspace")]
    pub reorder_workspace: bool,
    pub desktop: bool,
    pub sound: bool,
    pub command: bool,
    #[serde(rename = "paneFlash")]
    pub pane_flash: bool,
}

impl Default for TerminalNotificationPolicyEffects {
    fn default() -> Self {
        Self {
            record: true,
            mark_unread: true,
            reorder_workspace: true,
            desktop: true,
            sound: true,
            command: true,
            pane_flash: true,
        }
    }
}

/// The full envelope a policy hook receives and may patch.
///
/// Mirrors Swift `TerminalNotificationPolicyEnvelope`
/// (`TerminalNotificationPolicy.swift` 180-186).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalNotificationPolicyEnvelope {
    #[serde(default = "default_envelope_version")]
    pub version: i64,
    pub notification: TerminalNotificationPolicyPayload,
    pub context: TerminalNotificationPolicyContext,
    #[serde(default)]
    pub effects: TerminalNotificationPolicyEffects,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<bool>,
}

fn default_envelope_version() -> i64 {
    1
}

/// The immutable request the store assembles before evaluating policy hooks.
///
/// Mirrors Swift `TerminalNotificationPolicyRequest`
/// (`TerminalNotificationPolicy.swift` 188-220). Ids are `String` (not `UUID`)
/// to match the Rust notification model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalNotificationPolicyRequest {
    pub tab_id: String,
    pub surface_id: Option<String>,
    pub panel_id: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub cwd: Option<String>,
    pub is_app_focused: bool,
    pub is_focused_panel: bool,
}

/// A policy-hook failure descriptor.
///
/// Mirrors Swift `TerminalNotificationPolicyFailure`
/// (`TerminalNotificationPolicy.swift` 222-226).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TerminalNotificationPolicyFailure {
    pub hook_id: String,
    pub source_path: Option<String>,
    pub message: String,
}

// --- Patch-merge structs (decode-only) -------------------------------------
//
// A hook may emit a partial envelope; absent keys leave the prior value
// untouched, present keys overwrite, and (for nullable fields) an explicit
// `null` overwrites with `nil`. Swift expresses this with
// `decodeIfNonNullValuePresent` / `decodeNullableValueIfPresent`
// (`TerminalNotificationPolicy.swift` 931-947). In Rust the nullable fields use
// the `Option<Option<T>>` "double option" pattern: outer `None` = key absent,
// `Some(None)` = explicit null, `Some(Some(v))` = value.

fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::<T>::deserialize(de)?))
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TerminalNotificationPolicyEffectsPatch {
    #[serde(default)]
    pub record: Option<bool>,
    #[serde(rename = "markUnread", default)]
    pub mark_unread: Option<bool>,
    #[serde(rename = "reorderWorkspace", default)]
    pub reorder_workspace: Option<bool>,
    #[serde(default)]
    pub desktop: Option<bool>,
    #[serde(default)]
    pub sound: Option<bool>,
    #[serde(default)]
    pub command: Option<bool>,
    #[serde(rename = "paneFlash", default)]
    pub pane_flash: Option<bool>,
}

impl TerminalNotificationPolicyEffectsPatch {
    pub fn merged_into(
        &self,
        mut effects: TerminalNotificationPolicyEffects,
    ) -> TerminalNotificationPolicyEffects {
        if let Some(v) = self.record {
            effects.record = v;
        }
        if let Some(v) = self.mark_unread {
            effects.mark_unread = v;
        }
        if let Some(v) = self.reorder_workspace {
            effects.reorder_workspace = v;
        }
        if let Some(v) = self.desktop {
            effects.desktop = v;
        }
        if let Some(v) = self.sound {
            effects.sound = v;
        }
        if let Some(v) = self.command {
            effects.command = v;
        }
        if let Some(v) = self.pane_flash {
            effects.pane_flash = v;
        }
        effects
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TerminalNotificationPolicyPayloadPatch {
    #[serde(rename = "workspaceId", default)]
    pub workspace_id: Option<String>,
    #[serde(rename = "surfaceId", default, deserialize_with = "double_option")]
    pub surface_id: Option<Option<String>>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
}

impl TerminalNotificationPolicyPayloadPatch {
    pub fn merged_into(
        &self,
        mut payload: TerminalNotificationPolicyPayload,
    ) -> TerminalNotificationPolicyPayload {
        if let Some(v) = &self.workspace_id {
            payload.workspace_id = v.clone();
        }
        if let Some(v) = &self.surface_id {
            payload.surface_id = v.clone();
        }
        if let Some(v) = &self.title {
            payload.title = v.clone();
        }
        if let Some(v) = &self.subtitle {
            payload.subtitle = v.clone();
        }
        if let Some(v) = &self.body {
            payload.body = v.clone();
        }
        payload
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TerminalNotificationPolicyContextPatch {
    #[serde(default, deserialize_with = "double_option")]
    pub cwd: Option<Option<String>>,
    #[serde(rename = "configPath", default, deserialize_with = "double_option")]
    pub config_path: Option<Option<String>>,
    #[serde(rename = "hookId", default, deserialize_with = "double_option")]
    pub hook_id: Option<Option<String>>,
    #[serde(rename = "appFocused", default)]
    pub app_focused: Option<bool>,
    #[serde(rename = "focusedPanel", default)]
    pub focused_panel: Option<bool>,
}

impl TerminalNotificationPolicyContextPatch {
    pub fn merged_into(
        &self,
        mut context: TerminalNotificationPolicyContext,
    ) -> TerminalNotificationPolicyContext {
        if let Some(v) = &self.cwd {
            context.cwd = v.clone();
        }
        if let Some(v) = &self.config_path {
            context.config_path = v.clone();
        }
        if let Some(v) = &self.hook_id {
            context.hook_id = v.clone();
        }
        if let Some(v) = self.app_focused {
            context.app_focused = v;
        }
        if let Some(v) = self.focused_panel {
            context.focused_panel = v;
        }
        context
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TerminalNotificationPolicyEnvelopePatch {
    #[serde(default)]
    pub version: Option<i64>,
    #[serde(default)]
    pub notification: Option<TerminalNotificationPolicyPayloadPatch>,
    #[serde(default)]
    pub context: Option<TerminalNotificationPolicyContextPatch>,
    #[serde(default)]
    pub effects: Option<TerminalNotificationPolicyEffectsPatch>,
    #[serde(default)]
    pub stop: Option<bool>,
}

impl TerminalNotificationPolicyEnvelopePatch {
    pub fn merged_into(
        &self,
        envelope: TerminalNotificationPolicyEnvelope,
    ) -> TerminalNotificationPolicyEnvelope {
        TerminalNotificationPolicyEnvelope {
            version: self.version.unwrap_or(envelope.version),
            notification: self
                .notification
                .as_ref()
                .map(|patch| patch.merged_into(envelope.notification.clone()))
                .unwrap_or(envelope.notification),
            context: self
                .context
                .as_ref()
                .map(|patch| patch.merged_into(envelope.context.clone()))
                .unwrap_or(envelope.context),
            effects: self
                .effects
                .as_ref()
                .map(|patch| patch.merged_into(envelope.effects))
                .unwrap_or(envelope.effects),
            stop: self.stop.or(envelope.stop),
        }
    }
}

// --- Delivery gating decision ----------------------------------------------

/// What the store decided to do with a notification's external delivery, with
/// all OS calls left to the deferred delivery seam (M10 GUI track).
///
/// Mirrors the branching in Swift `deliverNotificationSideEffects`
/// (`TerminalNotificationStore.swift` 1249-1296) without performing any of the
/// actual toast / sound / push side effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryDecision {
    /// Deliver a desktop notification (toast + optional sound/command).
    Desktop,
    /// Suppress external delivery; play the quiet "suppressed" feedback only.
    Suppressed,
    /// No deliverable effect at all — nothing to surface.
    None,
}

/// Whether the app is focused on exactly this notification's tab+surface, so the
/// external (desktop) delivery should be suppressed in favor of the in-app
/// indicator.
///
/// Mirrors Swift `shouldSuppressExternalDelivery`
/// (`TerminalNotificationStore.swift` 1239-1247) with the focus booleans
/// injected (the `AppDelegate`/`tabManager` lookups are the caller's job).
pub fn should_suppress_external_delivery(
    is_app_focused: bool,
    is_active_tab: bool,
    is_focused_surface: bool,
) -> bool {
    is_app_focused && is_active_tab && is_focused_surface
}

/// Whether any notification effect at all is requested.
///
/// Mirrors Swift `hasAnyNotificationEffect`
/// (`TerminalNotificationStore.swift` 1298-1300).
pub fn has_any_notification_effect(effects: &TerminalNotificationPolicyEffects) -> bool {
    effects.record
        || effects.desktop
        || effects.sound
        || effects.command
        || effects.reorder_workspace
        || effects.mark_unread
}

/// The pure delivery decision for the side-effect lane.
///
/// Mirrors the guard + branch at the top of Swift
/// `deliverNotificationSideEffects` (`TerminalNotificationStore.swift`
/// 1249-1270): no deliverable effect → `None`; suppressed → `Suppressed`;
/// otherwise → `Desktop`.
pub fn delivery_decision(
    effects: &TerminalNotificationPolicyEffects,
    should_suppress_external_delivery: bool,
) -> DeliveryDecision {
    if !(effects.desktop || effects.sound || effects.command) {
        return DeliveryDecision::None;
    }
    if should_suppress_external_delivery {
        DeliveryDecision::Suppressed
    } else {
        DeliveryDecision::Desktop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_default_is_all_true() {
        let effects = TerminalNotificationPolicyEffects::default();
        assert!(effects.record);
        assert!(effects.mark_unread);
        assert!(effects.reorder_workspace);
        assert!(effects.desktop);
        assert!(effects.sound);
        assert!(effects.command);
        assert!(effects.pane_flash);
    }

    #[test]
    fn effects_missing_keys_decode_to_true() {
        let effects: TerminalNotificationPolicyEffects =
            serde_json::from_str(r#"{"desktop": false}"#).unwrap();
        assert!(!effects.desktop);
        // every other key defaults to true
        assert!(effects.record);
        assert!(effects.sound);
        assert!(effects.pane_flash);
    }

    #[test]
    fn effects_patch_overwrites_only_present_keys() {
        let base = TerminalNotificationPolicyEffects::default();
        let patch: TerminalNotificationPolicyEffectsPatch =
            serde_json::from_str(r#"{"sound": false, "desktop": false}"#).unwrap();
        let merged = patch.merged_into(base);
        assert!(!merged.sound);
        assert!(!merged.desktop);
        assert!(merged.record);
        assert!(merged.mark_unread);
    }

    #[test]
    fn payload_patch_distinguishes_absent_null_and_value_for_surface() {
        let payload = TerminalNotificationPolicyPayload {
            workspace_id: "w".into(),
            surface_id: Some("s".into()),
            title: "t".into(),
            subtitle: "sub".into(),
            body: "b".into(),
        };

        // absent → unchanged
        let absent: TerminalNotificationPolicyPayloadPatch =
            serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(
            absent.merged_into(payload.clone()).surface_id,
            Some("s".into())
        );

        // explicit null → cleared
        let null: TerminalNotificationPolicyPayloadPatch =
            serde_json::from_str(r#"{"surfaceId": null}"#).unwrap();
        assert_eq!(null.merged_into(payload.clone()).surface_id, None);

        // value → overwritten
        let value: TerminalNotificationPolicyPayloadPatch =
            serde_json::from_str(r#"{"surfaceId": "next"}"#).unwrap();
        assert_eq!(
            value.merged_into(payload).surface_id,
            Some("next".into())
        );
    }

    #[test]
    fn envelope_patch_merges_nested() {
        let envelope = TerminalNotificationPolicyEnvelope {
            version: 1,
            notification: TerminalNotificationPolicyPayload {
                workspace_id: "w".into(),
                surface_id: None,
                title: "t".into(),
                subtitle: "sub".into(),
                body: "b".into(),
            },
            context: TerminalNotificationPolicyContext {
                cwd: Some("/tmp".into()),
                config_path: None,
                hook_id: None,
                app_focused: true,
                focused_panel: false,
            },
            effects: TerminalNotificationPolicyEffects::default(),
            stop: None,
        };
        let patch: TerminalNotificationPolicyEnvelopePatch = serde_json::from_str(
            r#"{"notification": {"title": "new"}, "effects": {"desktop": false}, "stop": true}"#,
        )
        .unwrap();
        let merged = patch.merged_into(envelope);
        assert_eq!(merged.notification.title, "new");
        assert_eq!(merged.notification.body, "b");
        assert!(!merged.effects.desktop);
        assert_eq!(merged.stop, Some(true));
    }

    #[test]
    fn suppress_requires_all_three_focus_inputs() {
        assert!(should_suppress_external_delivery(true, true, true));
        assert!(!should_suppress_external_delivery(false, true, true));
        assert!(!should_suppress_external_delivery(true, false, true));
        assert!(!should_suppress_external_delivery(true, true, false));
    }

    #[test]
    fn delivery_decision_branches() {
        let mut effects = TerminalNotificationPolicyEffects::default();
        assert_eq!(delivery_decision(&effects, false), DeliveryDecision::Desktop);
        assert_eq!(
            delivery_decision(&effects, true),
            DeliveryDecision::Suppressed
        );

        effects.desktop = false;
        effects.sound = false;
        effects.command = false;
        assert_eq!(delivery_decision(&effects, false), DeliveryDecision::None);
        assert_eq!(delivery_decision(&effects, true), DeliveryDecision::None);
    }

    #[test]
    fn has_any_effect_tracks_all_lanes() {
        let mut effects = TerminalNotificationPolicyEffects {
            record: false,
            mark_unread: false,
            reorder_workspace: false,
            desktop: false,
            sound: false,
            command: false,
            pane_flash: true,
        };
        // pane_flash alone is not a deliverable effect
        assert!(!has_any_notification_effect(&effects));
        effects.reorder_workspace = true;
        assert!(has_any_notification_effect(&effects));
    }
}
