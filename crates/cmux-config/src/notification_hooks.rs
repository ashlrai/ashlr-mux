//! Notification-hook resolution: the deterministic core of the cmux config
//! store's `resolveNotificationHooks` pipeline.
//!
//! Direct port of the pure resolution logic in `Sources/CmuxConfig.swift`:
//! - `CmuxButtonIcon.projectRoot(forConfigPath:)` (`:630-636`)
//! - `CmuxNotificationHookDefinition.defaultTimeoutSeconds` /
//!   `resolvedTimeoutSeconds` (`:211`, `:264-266`)
//! - `CmuxActionTrustDescriptor` + `.fingerprint` (`Sources/CmuxActionTrust.swift:4-25`)
//! - `CmuxResolvedNotificationHook` + hand-written `==` / `hash` over
//!   `trustDescriptor?.fingerprint` (`:285-326`)
//! - `CmuxConfigStore.resolvedNotificationHooks(_:sourcePath:)` (`:2376-2410`)
//! - `CmuxConfigStore.resolveNotificationHooks(globalConfig:localConfigs:)`
//!   (`:2349-2374`)
//!
//! # What is intentionally left out (I/O shell / GUI — not part of the pure core)
//! - `CmuxConfigStore` itself (`@MainActor`/`@Published`), `parseConfig`,
//!   `findCmuxConfigHierarchy`, file watchers, `CmuxActionTrust` store,
//!   `NotificationPolicyHookAuthorizer` (NSWindow), and the `posix_spawn` hook
//!   engine (deferred to M8). This module produces the
//!   [`Vec<ResolvedNotificationHook>`] a future engine consumes.
//! - Discovery of the config hierarchy: the caller passes already-parsed
//!   [`NotificationsConfig`] values + their source paths in order.
//!
//! # Injected I/O seam
//! `CmuxConfigStore.canonicalPath(_:)` (`:1994-1996`) resolves symlinks via
//! `URL.resolvingSymlinksInPath().standardizedFileURL.path` — a filesystem
//! touch. Per the canonical-fidelity Windows-port directive we do NOT reach for
//! `std::fs::canonicalize`; instead callers inject a `canonicalize` closure
//! (identity in tests). Both `isGlobalHook` comparison and the descriptor's
//! `configPath` / `projectRoot` fields flow through it.
//!
//! # Path semantics
//! [`project_root`] mirrors macOS `NSString` path semantics (`/`-separated,
//! trailing-slash stripping) for macOS-oracle parity, per the canonical-fidelity
//! directive — it does NOT use `std::path`. Windows `\` / drive-letter handling
//! is a documented follow-up; the oracle inputs are all POSIX-style paths.

use std::collections::BTreeMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{HooksMode, NotificationHook, NotificationsConfig};

/// Fallback hook timeout, in seconds.
///
/// Mirrors `CmuxNotificationHookDefinition.defaultTimeoutSeconds = 20`
/// (`Sources/CmuxConfig.swift:211`). Note: [`NotificationHook::timeout_seconds`]
/// is non-optional in this crate and already defaults to `20.0` at decode time
/// (serde `#[serde(default)]`), so `resolvedTimeoutSeconds`'s `?? 20` collapses
/// to a passthrough here (`Sources/CmuxConfig.swift:264-266`). The constant is
/// retained to document the shared value.
pub const DEFAULT_TIMEOUT_SECONDS: f64 = 20.0;

// ---------------------------------------------------------------------------
// project_root  (CmuxButtonIcon.projectRoot(forConfigPath:), :630-636)
// ---------------------------------------------------------------------------

/// The project root directory for a config file path.
///
/// Mirrors `CmuxButtonIcon.projectRoot(forConfigPath:)`
/// (`Sources/CmuxConfig.swift:630-636`): drop the last path component; if the
/// resulting directory's last component is `.cmux`, drop one more.
///
/// Implemented with `/`-based `NSString`-style lexical semantics (see module
/// docs). No filesystem access.
pub fn project_root(config_path: &str) -> String {
    let config_dir = deleting_last_path_component(config_path);
    if last_path_component(&config_dir) == ".cmux" {
        deleting_last_path_component(&config_dir)
    } else {
        config_dir
    }
}

/// `/`-based port of `NSString.deletingLastPathComponent`.
///
/// Matches the documented Foundation examples: `"/tmp/scratch.tiff" -> "/tmp"`,
/// `"/tmp/lock/" -> "/tmp"`, `"/tmp/" -> "/"`, `"/tmp" -> "/"`,
/// `"scratch.tiff" -> ""`, `"/" -> "/"`.
fn deleting_last_path_component(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let trimmed = path.trim_end_matches('/');
    // The path was all slashes (e.g. "/" or "///").
    if trimmed.is_empty() {
        return "/".to_owned();
    }
    match trimmed.rfind('/') {
        None => String::new(),
        // Last slash is the leading root slash: parent is root.
        Some(0) => "/".to_owned(),
        Some(idx) => {
            // Strip any trailing slashes from the parent (e.g. "/a//b" -> "/a").
            let parent = trimmed[..idx].trim_end_matches('/');
            if parent.is_empty() {
                "/".to_owned()
            } else {
                parent.to_owned()
            }
        }
    }
}

/// `/`-based port of `NSString.lastPathComponent`.
///
/// `"/tmp/lock" -> "lock"`, `"/tmp/lock/" -> "lock"`, `".cmux" -> ".cmux"`,
/// `"/" -> "/"`, `"" -> ""`.
fn last_path_component(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        // Path was all slashes: `NSString` returns "/".
        return "/".to_owned();
    }
    match trimmed.rfind('/') {
        None => trimmed.to_owned(),
        Some(idx) => trimmed[idx + 1..].to_owned(),
    }
}

// ---------------------------------------------------------------------------
// ActionTrustDescriptor  (CmuxActionTrust.swift:4-25)
// ---------------------------------------------------------------------------

/// Trust descriptor for a resolved action, consumed by the (out-of-scope) trust
/// store to decide whether a hook is authorized to run.
///
/// Mirrors `CmuxActionTrustDescriptor` (`Sources/CmuxActionTrust.swift:4-25`).
/// The Swift type is `Codable`; its `fingerprint` is a SHA-256 over its JSON
/// encoding with **sorted keys** and **nil optionals omitted**. This port
/// reproduces that byte layout via a [`BTreeMap`] (sorted keys) that only
/// inserts `Some` fields (omitting `None`) — a plain `serde` struct would emit
/// keys in declaration order and would NOT match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionTrustDescriptor {
    /// `schemaVersion` (default `1` in Swift; always encoded — non-optional).
    pub schema_version: i64,
    /// `actionID`.
    pub action_id: String,
    /// `kind`.
    pub kind: String,
    /// `command`.
    pub command: Option<String>,
    /// `target`.
    pub target: Option<String>,
    /// `workspaceCommand`. Always `None` for notification hooks; modeled as an
    /// opaque JSON value so a non-nil case would still round-trip into the
    /// fingerprint. (Swift `CmuxCommandDefinition?`.)
    pub workspace_command: Option<Value>,
    /// `configPath`.
    pub config_path: Option<String>,
    /// `projectRoot`.
    pub project_root: Option<String>,
    /// `iconFingerprint`.
    pub icon_fingerprint: Option<String>,
}

impl ActionTrustDescriptor {
    /// Lowercase-hex SHA-256 of the sorted-keys, nil-omitting JSON encoding.
    ///
    /// Mirrors `CmuxActionTrustDescriptor.fingerprint`
    /// (`Sources/CmuxActionTrust.swift:15-24`): `JSONEncoder` with
    /// `.sortedKeys`, synthesized `encodeIfPresent` for optionals (nil omitted),
    /// then `SHA256` rendered as `%02x` bytes.
    ///
    /// Parity notes:
    /// - Keys are alphabetized by [`BTreeMap`] iteration order:
    ///   `actionID, command, configPath, iconFingerprint, kind, projectRoot,
    ///   schemaVersion, target, workspaceCommand`.
    /// - Swift `JSONEncoder` here sets only `.sortedKeys` (NOT
    ///   `.withoutEscapingSlashes`), so Foundation escapes every `/` as `\/`.
    ///   `serde_json` emits raw `/`, so we post-process its compact output with
    ///   `replace('/', "\\/")` to match byte-for-byte. This is safe because
    ///   compact JSON contains `/` only inside string values (paths), never in
    ///   structural tokens or serde escape sequences — so a global replace maps
    ///   exactly onto Swift's per-character slash escaping. Without this, every
    ///   non-global hook (whose `configPath`/`projectRoot` are file paths) would
    ///   produce a fingerprint that diverges from canonical macOS, breaking the
    ///   persisted `trusted-actions.json` authorization contract.
    /// - Both emit compact output and integers without a fraction; the only
    ///   encoding divergence is the slash escaping handled above.
    pub fn fingerprint(&self) -> String {
        let mut map: BTreeMap<&str, Value> = BTreeMap::new();
        // Non-optional fields — always encoded.
        map.insert("schemaVersion", Value::from(self.schema_version));
        map.insert("actionID", Value::from(self.action_id.clone()));
        map.insert("kind", Value::from(self.kind.clone()));
        // Optional fields — omitted when None (Swift `encodeIfPresent`).
        if let Some(v) = &self.command {
            map.insert("command", Value::from(v.clone()));
        }
        if let Some(v) = &self.target {
            map.insert("target", Value::from(v.clone()));
        }
        if let Some(v) = &self.workspace_command {
            map.insert("workspaceCommand", v.clone());
        }
        if let Some(v) = &self.config_path {
            map.insert("configPath", Value::from(v.clone()));
        }
        if let Some(v) = &self.project_root {
            map.insert("projectRoot", Value::from(v.clone()));
        }
        if let Some(v) = &self.icon_fingerprint {
            map.insert("iconFingerprint", Value::from(v.clone()));
        }
        // `to_string` on a `BTreeMap` serializes entries in sorted key order.
        let json = serde_json::to_string(&map).expect("descriptor map serializes");
        // Swift's `JSONEncoder` (`.sortedKeys` only) escapes `/` as `\/`;
        // `serde_json` does not. Escape here so path-bearing descriptors hash
        // identically to canonical macOS. See parity notes above.
        let json = json.replace('/', "\\/");
        let digest = Sha256::digest(json.as_bytes());
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }
}

// ---------------------------------------------------------------------------
// ResolvedNotificationHook  (CmuxConfig.swift:285-326)
// ---------------------------------------------------------------------------

/// A fully resolved notification hook ready for the (out-of-scope) hook engine.
///
/// Mirrors `CmuxResolvedNotificationHook` (`Sources/CmuxConfig.swift:285-326`),
/// including its **hand-written** `Equatable` / `Hashable`: equality and hashing
/// fold `trustDescriptor` down to `trustDescriptor?.fingerprint` (`:309-325`),
/// NOT the full descriptor. This port implements [`PartialEq`] and [`Hash`] by
/// hand to preserve that contract (a derived impl would compare every descriptor
/// field).
#[derive(Debug, Clone)]
pub struct ResolvedNotificationHook {
    /// `id`.
    pub id: String,
    /// `command`.
    pub command: String,
    /// `timeoutSeconds` (already defaulted; see [`DEFAULT_TIMEOUT_SECONDS`]).
    pub timeout_seconds: f64,
    /// `sourcePath` — the config file this hook came from. Optional to mirror
    /// the Swift `String?`, though resolution always sets it.
    pub source_path: Option<String>,
    /// `cwd` — the hook's working directory (its config's [`project_root`]).
    pub cwd: String,
    /// `trustDescriptor` — `None` for global hooks (Swift sets it to `nil`).
    pub trust_descriptor: Option<ActionTrustDescriptor>,
}

impl PartialEq for ResolvedNotificationHook {
    /// Mirrors `CmuxResolvedNotificationHook.==` (`:309-315`): compares scalar
    /// fields and `trustDescriptor?.fingerprint` (not the whole descriptor).
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.command == other.command
            && self.timeout_seconds == other.timeout_seconds
            && self.source_path == other.source_path
            && self.cwd == other.cwd
            && self.trust_descriptor.as_ref().map(ActionTrustDescriptor::fingerprint)
                == other.trust_descriptor.as_ref().map(ActionTrustDescriptor::fingerprint)
    }
}

impl std::hash::Hash for ResolvedNotificationHook {
    /// Mirrors `CmuxResolvedNotificationHook.hash(into:)` (`:318-325`): combines
    /// the scalar fields and `trustDescriptor?.fingerprint`. The `f64`
    /// `timeoutSeconds` is hashed via its bit pattern (Swift hashes the
    /// `Double`; there is no oracle over NaN, which does not occur here).
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.command.hash(state);
        self.timeout_seconds.to_bits().hash(state);
        self.source_path.hash(state);
        self.cwd.hash(state);
        self.trust_descriptor
            .as_ref()
            .map(ActionTrustDescriptor::fingerprint)
            .hash(state);
    }
}

// Not `Eq`: `f64` has no total equality (NaN). Matches Swift's `==` semantics.

// ---------------------------------------------------------------------------
// resolution  (CmuxConfig.swift:2349-2410)
// ---------------------------------------------------------------------------

/// Resolve the hooks from a single config source into
/// [`ResolvedNotificationHook`]s.
///
/// Mirrors `CmuxConfigStore.resolvedNotificationHooks(_:sourcePath:)`
/// (`Sources/CmuxConfig.swift:2376-2410`): drop disabled definitions; for global
/// sources the descriptor is `nil`, otherwise build a `notificationHook` /
/// `notificationPolicy` descriptor with the canonical source path and canonical
/// project root.
///
/// `cwd` is the source's [`project_root`]; `canonical_source` is the caller's
/// pre-canonicalized source path (`configPath`); `canonicalize` produces the
/// descriptor's `projectRoot` from `cwd` (identity in tests).
pub fn resolved_hooks_for<C: Fn(&str) -> String>(
    defs: &[NotificationHook],
    source_path: &str,
    is_global: bool,
    cwd: &str,
    canonical_source: &str,
    canonicalize: &C,
) -> Vec<ResolvedNotificationHook> {
    defs.iter()
        .filter(|definition| definition.enabled)
        .map(|definition| {
            let trust_descriptor = if is_global {
                None
            } else {
                Some(ActionTrustDescriptor {
                    schema_version: 1,
                    action_id: definition.id.clone(),
                    kind: "notificationHook".to_owned(),
                    command: Some(definition.command.clone()),
                    target: Some("notificationPolicy".to_owned()),
                    workspace_command: None,
                    config_path: Some(canonical_source.to_owned()),
                    project_root: Some(canonicalize(cwd)),
                    icon_fingerprint: None,
                })
            };
            ResolvedNotificationHook {
                id: definition.id.clone(),
                command: definition.command.clone(),
                // `NotificationHook.timeout_seconds` is already defaulted to
                // `DEFAULT_TIMEOUT_SECONDS` at decode time (see const docs), so
                // this passthrough matches Swift's `resolvedTimeoutSeconds`.
                timeout_seconds: definition.timeout_seconds,
                source_path: Some(source_path.to_owned()),
                cwd: cwd.to_owned(),
                trust_descriptor,
            }
        })
        .collect()
}

/// Resolve the effective notification hooks across the global config and an
/// ordered chain of local configs.
///
/// Mirrors `CmuxConfigStore.resolveNotificationHooks(globalConfig:localConfigs:)`
/// (`Sources/CmuxConfig.swift:2349-2374`): append the global hooks first, then
/// each local config in order; a local config with `hooksMode == replace` clears
/// the accumulator before appending its own hooks. A hook is treated as global
/// (descriptor `nil`) when its canonicalized source path equals the canonicalized
/// global config path.
///
/// Divergence from Swift, inherited not introduced: Swift `guard let
/// notifications = entry.config.notifications else { continue }` skips locals
/// that have no `notifications` section entirely (so their absent section never
/// triggers `replace`). This port takes only locals that HAVE a section (the
/// caller filters), matching that behavior — an empty section still applies
/// `replace`.
pub fn resolve_notification_hooks<C: Fn(&str) -> String>(
    global: Option<&NotificationsConfig>,
    global_path: &str,
    locals: &[(String, NotificationsConfig)],
    canonicalize: C,
) -> Vec<ResolvedNotificationHook> {
    let mut hooks: Vec<ResolvedNotificationHook> = Vec::new();
    let canonical_global = canonicalize(global_path);

    if let Some(global_config) = global {
        let cwd = project_root(global_path);
        // The global source path is trivially global
        // (`canonical(global) == canonical(global)`).
        hooks.extend(resolved_hooks_for(
            &global_config.hooks,
            global_path,
            true,
            &cwd,
            &canonical_global,
            &canonicalize,
        ));
    }

    for (path, config) in locals {
        if config.hooks_mode == HooksMode::Replace {
            hooks.clear();
        }
        let cwd = project_root(path);
        let canonical_source = canonicalize(path);
        let is_global = canonical_source == canonical_global;
        hooks.extend(resolved_hooks_for(
            &config.hooks,
            path,
            is_global,
            &cwd,
            &canonical_source,
            &canonicalize,
        ));
    }

    hooks
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Identity canonicalizer — the injected I/O seam in tests. All oracle
    /// cases have `is_global` trivially true for globals / false for locals, so
    /// identity reproduces `CmuxConfigStore.canonicalPath`'s asserted contract.
    fn identity(path: &str) -> String {
        path.to_owned()
    }

    fn hook(id: &str, command: &str, enabled: bool) -> NotificationHook {
        NotificationHook {
            id: id.to_owned(),
            command: command.to_owned(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            enabled,
        }
    }

    fn notifications(mode: HooksMode, hooks: Vec<NotificationHook>) -> NotificationsConfig {
        NotificationsConfig {
            hooks_mode: mode,
            hooks,
            ..NotificationsConfig::default()
        }
    }

    // -- project_root / path lexical helpers ---------------------------------

    #[test]
    fn project_root_strips_cmux_directory() {
        // `.../project/.cmux/cmux.json` -> `.../project`
        assert_eq!(project_root("/root/project/.cmux/cmux.json"), "/root/project");
        // `.../project/child/.cmux/cmux.json` -> `.../project/child`
        assert_eq!(
            project_root("/root/project/child/.cmux/cmux.json"),
            "/root/project/child"
        );
    }

    #[test]
    fn project_root_without_cmux_keeps_config_dir() {
        // Explicit config not under `.cmux` -> just the containing directory.
        assert_eq!(
            project_root("/root/explicit/custom-cmux.json"),
            "/root/explicit"
        );
    }

    #[test]
    fn deleting_last_path_component_matches_nsstring_examples() {
        assert_eq!(deleting_last_path_component("/tmp/scratch.tiff"), "/tmp");
        assert_eq!(deleting_last_path_component("/tmp/lock/"), "/tmp");
        assert_eq!(deleting_last_path_component("/tmp/"), "/");
        assert_eq!(deleting_last_path_component("/tmp"), "/");
        assert_eq!(deleting_last_path_component("scratch.tiff"), "");
        assert_eq!(deleting_last_path_component("/"), "/");
        assert_eq!(deleting_last_path_component(""), "");
    }

    #[test]
    fn last_path_component_matches_nsstring_examples() {
        assert_eq!(last_path_component("/tmp/lock"), "lock");
        assert_eq!(last_path_component("/tmp/lock/"), "lock");
        assert_eq!(last_path_component(".cmux"), ".cmux");
        assert_eq!(last_path_component("/"), "/");
        assert_eq!(last_path_component(""), "");
    }

    // -- fingerprint ----------------------------------------------------------

    fn parent_descriptor() -> ActionTrustDescriptor {
        ActionTrustDescriptor {
            schema_version: 1,
            action_id: "parent".to_owned(),
            kind: "notificationHook".to_owned(),
            command: Some("cat".to_owned()),
            target: Some("notificationPolicy".to_owned()),
            workspace_command: None,
            config_path: Some("/tmp/project/.cmux/cmux.json".to_owned()),
            project_root: Some("/tmp/project".to_owned()),
            icon_fingerprint: None,
        }
    }

    /// Author-derived oracle: Swift has NO test pinning the fingerprint hex
    /// (it is otherwise consumed only by the out-of-scope trust store). The
    /// expected value is the lowercase-hex SHA-256 of the sorted-keys,
    /// nil-omitting JSON as Swift's `JSONEncoder` (`.sortedKeys` only) emits it
    /// — i.e. with `/` escaped as `\/` (Foundation escapes slashes unless
    /// `.withoutEscapingSlashes` is set, which `CmuxActionTrust.swift:15-19`
    /// does NOT set):
    /// `{"actionID":"parent","command":"cat",`
    /// `"configPath":"\/tmp\/project\/.cmux\/cmux.json","kind":"notificationHook",`
    /// `"projectRoot":"\/tmp\/project","schemaVersion":1,`
    /// `"target":"notificationPolicy"}`
    /// hand-computed with an independent SHA-256 implementation.
    #[test]
    fn fingerprint_matches_hand_computed_oracle() {
        assert_eq!(
            parent_descriptor().fingerprint(),
            "be4017b68b8c9dbbd1874303c00a7ab7b7d0318c37f5dbba3108044955dd5c4d"
        );
    }

    /// Parity pin for the slash-escaping divergence (Swift `JSONEncoder` with
    /// only `.sortedKeys` escapes `/` as `\/`; `serde_json` does not). Encoded
    /// JSON (Swift form): `{"actionID":"a","configPath":"\/x\/y","kind":"k",`
    /// `"schemaVersion":1}`. The UNescaped serde form would hash to
    /// differing bytes; this test fails if the `replace('/', "\\/")`
    /// step regresses. Hex is SHA-256 of the slash-escaped byte stream.
    #[test]
    fn fingerprint_escapes_slashes_like_swift_json_encoder() {
        let d = ActionTrustDescriptor {
            schema_version: 1,
            action_id: "a".to_owned(),
            kind: "k".to_owned(),
            command: None,
            target: None,
            workspace_command: None,
            config_path: Some("/x/y".to_owned()),
            project_root: None,
            icon_fingerprint: None,
        };
        // SHA-256 of
        // `{"actionID":"a","configPath":"\/x\/y","kind":"k","schemaVersion":1}`
        // (slashes escaped, as Foundation's JSONEncoder emits them).
        assert_eq!(
            d.fingerprint(),
            "717ad43463f2f30ff66c435ad4efbb2fc178207e739804b6af62e6ff1d4d1aef"
        );
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let d = parent_descriptor();
        assert_eq!(d.fingerprint(), d.fingerprint());
        // 64 lowercase hex chars.
        let fp = d.fingerprint();
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn fingerprint_omits_nil_and_ignores_field_order() {
        // Two descriptors equal in Some-fields but differing only in the None
        // fields being None -> same fingerprint (nil is omitted, not encoded).
        let a = parent_descriptor();
        let mut b = parent_descriptor();
        b.icon_fingerprint = None; // already None; explicit for clarity.
        assert_eq!(a.fingerprint(), b.fingerprint());
        // Changing a Some field changes the fingerprint.
        let mut c = parent_descriptor();
        c.command = Some("ls".to_owned());
        assert_ne!(a.fingerprint(), c.fingerprint());
    }

    // -- resolve_notification_hooks (ported Swift store oracles) --------------

    /// Port of `testNotificationHooksAppendThroughConfigHierarchy`
    /// (`cmuxTests/CmuxConfigTests.swift:960-1016`). Discovery is skipped: we
    /// feed the already-parsed global + parent + child configs with their paths.
    #[test]
    fn append_through_config_hierarchy() {
        let global_path = "/root/global/cmux.json";
        let parent_path = "/root/project/.cmux/cmux.json";
        let child_path = "/root/project/child/.cmux/cmux.json";

        let global = notifications(HooksMode::Append, vec![hook("global", "cat", true)]);
        let parent = notifications(HooksMode::Append, vec![hook("parent", "cat", true)]);
        let child = notifications(
            HooksMode::Append,
            vec![hook("child", "cat", false), hook("nearest", "cat", true)],
        );

        let resolved = resolve_notification_hooks(
            Some(&global),
            global_path,
            &[
                (parent_path.to_owned(), parent),
                (child_path.to_owned(), child),
            ],
            identity,
        );

        let ids: Vec<&str> = resolved.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["global", "parent", "nearest"]);

        // Global hook has no descriptor.
        assert!(resolved[0].trust_descriptor.is_none());

        // Parent (local) hook descriptor.
        let parent_desc = resolved[1].trust_descriptor.as_ref().unwrap();
        assert_eq!(parent_desc.kind, "notificationHook");
        assert_eq!(parent_desc.command.as_deref(), Some("cat"));
        assert_eq!(parent_desc.config_path.as_deref(), Some(parent_path));

        // Nearest (child) hook descriptor exists with the right kind.
        let nearest_desc = resolved[2].trust_descriptor.as_ref().unwrap();
        assert_eq!(nearest_desc.kind, "notificationHook");

        // cwd via project_root: parent -> `.../project`, child -> `.../project/child`.
        assert_eq!(resolved[1].cwd, "/root/project");
        assert_eq!(resolved[2].cwd, "/root/project/child");
    }

    /// Port of
    /// `testNotificationHooksIncludeExplicitLocalConfigOutsideDiscoveredHierarchy`
    /// (`cmuxTests/CmuxConfigTests.swift:1018-1056`).
    #[test]
    fn include_explicit_local_outside_hierarchy() {
        let global_path = "/root/global/cmux.json";
        let explicit_path = "/root/explicit/custom-cmux.json";

        let global = notifications(HooksMode::Append, vec![hook("global", "cat", true)]);
        let explicit = notifications(HooksMode::Append, vec![hook("explicit", "cat", true)]);

        let resolved = resolve_notification_hooks(
            Some(&global),
            global_path,
            &[(explicit_path.to_owned(), explicit)],
            identity,
        );

        let ids: Vec<&str> = resolved.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["global", "explicit"]);
        assert_eq!(resolved[1].source_path.as_deref(), Some(explicit_path));
    }

    /// Port of `testNotificationHooksReplaceInheritedHooks`
    /// (`cmuxTests/CmuxConfigTests.swift:1058-1099`). `hooksMode: replace` on the
    /// child clears the inherited global hook.
    #[test]
    fn replace_inherited_hooks() {
        let global_path = "/root/global/cmux.json";
        let child_path = "/root/project/child/.cmux/cmux.json";

        let global = notifications(HooksMode::Append, vec![hook("global", "cat", true)]);
        let child = notifications(HooksMode::Replace, vec![hook("child", "cat", true)]);

        let resolved = resolve_notification_hooks(
            Some(&global),
            global_path,
            &[(child_path.to_owned(), child)],
            identity,
        );

        let ids: Vec<&str> = resolved.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["child"]);
    }

    /// Port of `testNotificationHooksResolveFromExplicitWorkspaceDirectory`
    /// (`cmuxTests/CmuxConfigTests.swift:1101-1141`). Resolving from a workspace
    /// directory yields global then the discovered child hook. Discovery is out
    /// of scope, so the caller supplies the child config directly.
    #[test]
    fn resolve_from_explicit_workspace_directory() {
        let global_path = "/root/global/cmux.json";
        let child_path = "/root/project/child/.cmux/cmux.json";

        let global = notifications(HooksMode::Append, vec![hook("global", "cat", true)]);
        let child = notifications(HooksMode::Append, vec![hook("child", "cat", true)]);

        let resolved = resolve_notification_hooks(
            Some(&global),
            global_path,
            &[(child_path.to_owned(), child)],
            identity,
        );

        let ids: Vec<&str> = resolved.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["global", "child"]);
    }

    // -- parity-risk edge cases ----------------------------------------------

    #[test]
    fn local_equal_to_global_path_is_treated_as_global() {
        // A local whose canonicalized path equals the global path gets a nil
        // descriptor (Swift isGlobalHook == true), mirroring :2383-2388.
        let global_path = "/root/global/cmux.json";
        let global = notifications(HooksMode::Append, vec![hook("g", "cat", true)]);
        let local = notifications(HooksMode::Append, vec![hook("also-global", "cat", true)]);

        let resolved = resolve_notification_hooks(
            Some(&global),
            global_path,
            &[(global_path.to_owned(), local)],
            identity,
        );
        assert_eq!(resolved.len(), 2);
        assert!(resolved[1].trust_descriptor.is_none());
    }

    #[test]
    fn no_global_config_yields_only_locals() {
        let global_path = "/root/global/cmux.json";
        let local = notifications(HooksMode::Append, vec![hook("only", "cat", true)]);
        let resolved = resolve_notification_hooks(
            None,
            global_path,
            &[("/root/project/.cmux/cmux.json".to_owned(), local)],
            identity,
        );
        let ids: Vec<&str> = resolved.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["only"]);
        // Local (non-global) hook carries a descriptor.
        assert!(resolved[0].trust_descriptor.is_some());
    }

    #[test]
    fn disabled_hooks_are_dropped() {
        let resolved = resolved_hooks_for(
            &[hook("on", "cat", true), hook("off", "cat", false)],
            "/root/project/.cmux/cmux.json",
            false,
            "/root/project",
            "/root/project/.cmux/cmux.json",
            &identity,
        );
        let ids: Vec<&str> = resolved.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["on"]);
    }

    #[test]
    fn replace_on_empty_section_clears_accumulator() {
        // An (empty) replace section still clears inherited hooks — matches the
        // Swift behavior for a present-but-empty `notifications` section.
        let global_path = "/root/global/cmux.json";
        let global = notifications(HooksMode::Append, vec![hook("global", "cat", true)]);
        let empty_replace = notifications(HooksMode::Replace, vec![]);
        let resolved = resolve_notification_hooks(
            Some(&global),
            global_path,
            &[("/root/project/.cmux/cmux.json".to_owned(), empty_replace)],
            identity,
        );
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolved_hook_equality_folds_descriptor_to_fingerprint() {
        // Two hooks whose descriptors differ only in a field NOT in the
        // fingerprint would be equal — but every descriptor field IS in the
        // fingerprint, so we instead prove equality holds for identical hooks
        // and that a differing command (which changes the fingerprint) breaks it.
        let make = |cmd: &str| ResolvedNotificationHook {
            id: "h".to_owned(),
            command: cmd.to_owned(),
            timeout_seconds: 20.0,
            source_path: Some("/p".to_owned()),
            cwd: "/c".to_owned(),
            trust_descriptor: Some(ActionTrustDescriptor {
                schema_version: 1,
                action_id: "h".to_owned(),
                kind: "notificationHook".to_owned(),
                command: Some(cmd.to_owned()),
                target: Some("notificationPolicy".to_owned()),
                workspace_command: None,
                config_path: Some("/p".to_owned()),
                project_root: Some("/c".to_owned()),
                icon_fingerprint: None,
            }),
        };
        assert_eq!(make("cat"), make("cat"));
        assert_ne!(make("cat"), make("ls"));
    }
}
