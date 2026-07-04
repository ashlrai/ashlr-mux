//! Terminal surface top/title label state machine (the bonsplit tab-strip
//! label shown above each terminal surface).
//!
//! The live label is NOT `TerminalPanel.displayTitle` alone — for terminals
//! that is effectively the constant `"Terminal"` (`Sources/Panels/
//! TerminalPanel.swift:64` defaults `title` to `"Terminal"` and
//! `updateTitle` at `:203-208` has zero callers). The label is a
//! Workspace-owned state machine over three maps keyed by panel UUID
//! (`Sources/Workspace.swift:2369-2372`):
//!
//! - `panelTitles` — OSC/process titles (seeded `"Terminal"` at terminal
//!   panel creation, `Workspace.swift:3106`),
//! - `panelCustomTitles` — user/AI renames,
//! - `panelCustomTitleSources` — provenance ([`CustomTitleSource`]) so AI
//!   auto-titles never beat user titles.
//!
//! Data flow (terminal): Ghostty `SET_TITLE` → `GhosttyTitleChange`
//! notification (`Sources/GhosttyTerminalView.swift:2884-2898`, nil C-string
//! → `""`) → `TabManager` observer (`Sources/TabManager.swift:534-547`) →
//! [`PendingTitleUpdates::enqueue`] (trim, drop-if-empty, last-write-wins,
//! `TabManager.swift:3286-3305`) → coalesced flush → per-entry
//! [`PanelTitleState::update_panel_title`] (`Workspace.swift:5022-5072`).
//!
//! Portability boundary — NOT ported here (UI / process-level machinery):
//! NotificationCenter and the `===` surface-identity gates, the
//! `bonsplitController.updateTab` invocation itself (we emit [`TabSync`]
//! projections instead), `DispatchQueue` timing, the NSAlert rename prompt
//! UI, the remote-tmux `rename-window` side effect (we emit the string
//! only), `AutoNamingStatusStore`, and workspace *window*-title rendering
//! (`TabManager.updateWindowTitle` / `resolvedWorkspaceDisplayTitle` — a
//! separate workspace-level feature).
//!
//! Sanctioned simplifications (documented divergences, no observable
//! behavior change):
//! - Swift flushes `pendingPanelTitleUpdates` in Dictionary iteration order,
//!   which is unspecified; [`PendingTitleUpdates::drain`] likewise yields
//!   `HashMap` order, which is parity-faithful because Swift guarantees none.
//! - Swift skips the `updateTab` push when the panel has no bound bonsplit
//!   tab (`Workspace.swift:4077`, `:5038`). In `setPanelCustomTitle` that
//!   SAME guard (`:4077` `guard let panel …, let tabId …`) also gates the
//!   remote-tmux `rename-window` propagation (`:4084-4089`) — an unbound
//!   panel mutates, returns `true`, and emits NEITHER. This pure model has
//!   no tab binding table, so after a real mutation it always emits both
//!   the [`TabSync`] projection and [`SetOutcome::tmux_rename`] (they are
//!   `Some`/`None` together); the caller that owns the binding applies or
//!   drops the PAIR — dropping `tab_sync` for an unbound panel must also
//!   drop `tmux_rename`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Default title seeded for a new terminal panel.
/// Mirrors `Sources/Panels/TerminalPanel.swift:64` (`title = "Terminal"`)
/// via `displayTitle` (`:121-123`) at panel creation
/// (`Sources/Workspace.swift:3106`).
pub const DEFAULT_TERMINAL_TITLE: &str = "Terminal";

/// Fallback tab label when the resolved fallback trims to empty.
/// Mirrors `Sources/Workspace.swift:3923`.
pub const FALLBACK_TAB_TITLE: &str = "Tab";

/// Swift `CharacterSet.whitespacesAndNewlines` membership, pinned as an
/// explicit table (independent of the Rust `White_Space` property table):
/// Zs (U+0020, U+00A0, U+1680, U+2000–U+200A, U+202F, U+205F, U+3000),
/// Zl U+2028, Zp U+2029, U+0009–U+000D, and U+0085.
///
/// Deliberately NOT members (Swift does not trim them): U+200B ZWSP,
/// U+FEFF BOM, U+180E MONGOLIAN VOWEL SEPARATOR.
fn is_swift_whitespace_or_newline(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{0020}'
            | '\u{0085}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// Both-ends trim mirroring Swift
/// `trimmingCharacters(in: .whitespacesAndNewlines)` — every trim in the
/// title pipeline uses exactly this set (e.g. `Sources/Workspace.swift:5024`,
/// `:3922`, `:4056`; `Sources/TabManager.swift:3287`).
pub fn trim_whitespace_and_newlines(text: &str) -> &str {
    text.trim_matches(is_swift_whitespace_or_newline)
}

/// Provenance of a custom (user/AI) title.
///
/// Mirrors `Workspace.CustomTitleSource` (`Sources/Workspace.swift:4323-4326`):
/// `String`-raw-valued (`"user"` / `"auto"`), `Codable`, round-trips through
/// session persistence. Missing/legacy provenance normalizes to `.user`
/// (`effectiveCustomTitleSource`, `Workspace.swift:4336-4338`; panel-level
/// inline at `:4060`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomTitleSource {
    User,
    Auto,
}

impl CustomTitleSource {
    /// The Swift `rawValue` string.
    pub fn as_str(self) -> &'static str {
        match self {
            CustomTitleSource::User => "user",
            CustomTitleSource::Auto => "auto",
        }
    }

    /// Parse a Swift `rawValue` string; `None` for anything else
    /// (mirrors `CustomTitleSource(rawValue:)` failable init).
    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "user" => Some(CustomTitleSource::User),
            "auto" => Some(CustomTitleSource::Auto),
            _ => None,
        }
    }
}

/// Pure projection of the `bonsplitController.updateTab(title:hasCustomTitle:)`
/// side effect (`Sources/Workspace.swift:4079-4083`, `:5042-5046`,
/// `:1531-1535`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabSync {
    pub title: String,
    pub has_custom_title: bool,
}

/// Workspace-level inputs read by [`PanelTitleState::update_panel_title`]
/// for the single-panel workspace-title propagation
/// (`Sources/Workspace.swift:5049-5059`). The workspace `title`,
/// `customTitle`, and `processTitle` live outside the three panel maps
/// (`Workspace.swift:2164-2165`, `:2618`), so the caller supplies them.
#[derive(Debug, Clone, Copy)]
pub struct WorkspaceTitleContext<'a> {
    /// `workspace.customTitle != nil` — note: nil-check, NOT trimmed-empty
    /// (`Workspace.swift:5050`).
    pub has_custom_title: bool,
    /// Current `workspace.title` (`Workspace.swift:5051`).
    pub title: &'a str,
    /// Current `workspace.processTitle` (non-optional `String`,
    /// `Workspace.swift:2618`, compared at `:5056`).
    pub process_title: &'a str,
}

/// Result of [`PanelTitleState::update_panel_title`], mirroring
/// `Workspace.updatePanelTitle`'s return value and side effects
/// (`Sources/Workspace.swift:5022-5072`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateOutcome {
    /// The Swift `didMutate` return: panel title changed OR workspace title
    /// propagated. NOTE: a `processTitle`-only propagation does NOT set this
    /// (`Workspace.swift:5056-5058` omits `didMutate = true`).
    pub did_mutate: bool,
    /// Emitted only when the panel title itself changed
    /// (`Workspace.swift:5036-5047`).
    pub tab_sync: Option<TabSync>,
    /// `Some(new_title)` when the single-panel workspace-title propagation
    /// fired (`Workspace.swift:5051-5055`). Fires even when the panel title
    /// itself was unchanged.
    pub workspace_title: Option<String>,
    /// `Some(new_process_title)` when `processTitle` was updated
    /// (`Workspace.swift:5056-5058`); never affects `did_mutate`.
    pub process_title: Option<String>,
}

/// Result of [`PanelTitleState::set_panel_custom_title`], mirroring
/// `Workspace.setPanelCustomTitle`'s return value and side effects
/// (`Sources/Workspace.swift:4049-4091`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetOutcome {
    /// Whether the write landed (the Swift `Bool` return).
    pub applied: bool,
    /// The `updateTab` push after a real mutation (`Workspace.swift:4077-4083`).
    /// `None` on the same-text early return (`:4067-4072`) and on rejects.
    pub tab_sync: Option<TabSync>,
    /// The `rename-window` propagation string for remote tmux mirrors
    /// (`Workspace.swift:4084-4089`): the trimmed title, empty string on
    /// clear. Two caller-owned gates: `isRemoteTmuxMirror` (`:4085`), and
    /// the no-bound-tab guard (`:4077`) which in Swift gates this emission
    /// together with the `updateTab` push — so a caller dropping
    /// [`Self::tab_sync`] for an unbound panel MUST also drop this (Swift
    /// would have emitted neither). We always emit after a real mutation;
    /// `tab_sync` and `tmux_rename` are `Some`/`None` together.
    pub tmux_rename: Option<String>,
}

/// Result of [`PanelTitleState::apply_session_panel_metadata`], mirroring
/// the title-related effects of `Workspace.applySessionPanelMetadata`
/// (`Sources/Workspace.swift:1518-1536`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestoreOutcome {
    /// Whether the inner `setPanelCustomTitle` landed (Swift discards this).
    pub custom_applied: bool,
    /// The inner set's tmux rename emission, if any. Inherits
    /// [`SetOutcome::tmux_rename`]'s caller-owned gates: `isRemoteTmuxMirror`
    /// AND the no-bound-tab guard (`Workspace.swift:4077`) — a caller with
    /// no bound bonsplit tab for the panel drops this (Swift emits nothing).
    pub tmux_rename: Option<String>,
    /// The unconditional restored-title push (`Workspace.swift:1530-1536`) —
    /// `setPanelCustomTitle` skips the sync when there is no custom title,
    /// so the restore pushes explicitly. `None` only when the panel is
    /// unknown (`:1530` guard).
    pub tab_sync: Option<TabSync>,
}

/// The Workspace-owned panel title state machine: the three title maps
/// (`Sources/Workspace.swift:2369-2372`) plus the panel-existence set
/// standing in for `workspace.panels` keys (the guards at
/// `Workspace.swift:4055`, `:5025`, `:4175`, `:10711-10712`).
#[derive(Debug, Clone, Default)]
pub struct PanelTitleState {
    /// `panelTitles` (`Workspace.swift:2369`) — OSC/process titles.
    titles: HashMap<Uuid, String>,
    /// `panelCustomTitles` (`Workspace.swift:2370`) — user/AI renames.
    custom_titles: HashMap<Uuid, String>,
    /// `panelCustomTitleSources` (`Workspace.swift:2371-2372`) — provenance.
    custom_sources: HashMap<Uuid, CustomTitleSource>,
    /// Stand-in for `workspace.panels` keys (existence only).
    panel_ids: HashSet<Uuid>,
}

impl PanelTitleState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a panel without seeding a title (non-terminal panel kinds —
    /// browser/project/etc. — never seed `panelTitles`).
    pub fn insert_panel(&mut self, panel_id: Uuid) {
        self.panel_ids.insert(panel_id);
    }

    /// Register a terminal panel and seed `panelTitles[id] = "Terminal"`,
    /// mirroring terminal-panel creation
    /// (`Sources/Workspace.swift:3105-3106`: `panelTitles[terminalPanel.id]
    /// = terminalPanel.displayTitle`, which is `"Terminal"` at creation).
    pub fn seed_terminal_panel(&mut self, panel_id: Uuid) {
        self.panel_ids.insert(panel_id);
        self.titles
            .insert(panel_id, DEFAULT_TERMINAL_TITLE.to_string());
    }

    /// Raw `panelTitles[id]` accessor (e.g. `TabManager.swift:3349`).
    pub fn raw_title(&self, panel_id: Uuid) -> Option<&str> {
        self.titles.get(&panel_id).map(String::as_str)
    }

    /// Raw `panelCustomTitles[id]` accessor.
    pub fn custom_title(&self, panel_id: Uuid) -> Option<&str> {
        self.custom_titles.get(&panel_id).map(String::as_str)
    }

    /// Raw `panelCustomTitleSources[id]` accessor.
    pub fn custom_title_source(&self, panel_id: Uuid) -> Option<CustomTitleSource> {
        self.custom_sources.get(&panel_id).copied()
    }

    /// Mirrors `Workspace.resolvedPanelTitle(panelId:fallback:)`
    /// (`Sources/Workspace.swift:3921-3929`): a non-empty trimmed custom
    /// title wins; otherwise the trimmed fallback, or `"Tab"` when the
    /// fallback trims to empty. A whitespace-only custom title falls
    /// through (defensive re-trim at `:3924-3925`).
    pub fn resolved_panel_title(&self, panel_id: Uuid, fallback: &str) -> String {
        let trimmed_fallback = trim_whitespace_and_newlines(fallback);
        let fallback_title = if trimmed_fallback.is_empty() {
            FALLBACK_TAB_TITLE
        } else {
            trimmed_fallback
        };
        if let Some(custom) = self.custom_titles.get(&panel_id) {
            let custom = trim_whitespace_and_newlines(custom);
            if !custom.is_empty() {
                return custom.to_string();
            }
        }
        fallback_title.to_string()
    }

    /// Mirrors `Workspace.panelTitle(panelId:)`
    /// (`Sources/Workspace.swift:4174-4178`): `None` when the panel is
    /// unknown; otherwise resolve with `panelTitles[id] ?? displayTitle`
    /// as the fallback.
    pub fn panel_title(&self, panel_id: Uuid, display_title: &str) -> Option<String> {
        if !self.panel_ids.contains(&panel_id) {
            return None;
        }
        let fallback = self
            .titles
            .get(&panel_id)
            .map_or(display_title, String::as_str);
        Some(self.resolved_panel_title(panel_id, fallback))
    }

    /// Mirrors the rename-prompt initial value
    /// (`Sources/Workspace.swift:10710-10717`): `None` when the panel is
    /// unknown (`:10711-10712` guard), else the RAW un-resolved chain
    /// `panelCustomTitles[id] ?? panelTitles[id] ?? displayTitle`.
    pub fn rename_prompt_seed(&self, panel_id: Uuid, display_title: &str) -> Option<String> {
        if !self.panel_ids.contains(&panel_id) {
            return None;
        }
        Some(
            self.custom_titles
                .get(&panel_id)
                .or_else(|| self.titles.get(&panel_id))
                .map_or(display_title, String::as_str)
                .to_string(),
        )
    }

    /// Mirrors `Workspace.updatePanelTitle(panelId:title:)`
    /// (`Sources/Workspace.swift:5022-5072`) — the OSC/process title write.
    ///
    /// Guard: trimmed-empty title or unknown panel → no-op returning the
    /// default (all-`false`/`None`) outcome (`:5025`). An OSC clearing the
    /// title never clears the label; the last non-empty title sticks.
    ///
    /// `display_title` is `panel.displayTitle`, the `panelTitles` fallback
    /// at `:5040` (kept for 1:1 structure even though `panelTitles[id]` was
    /// just written on the only path that reads it).
    pub fn update_panel_title(
        &mut self,
        panel_id: Uuid,
        title: &str,
        workspace: WorkspaceTitleContext<'_>,
        display_title: &str,
    ) -> UpdateOutcome {
        let trimmed = trim_whitespace_and_newlines(title);
        if trimmed.is_empty() || !self.panel_ids.contains(&panel_id) {
            return UpdateOutcome::default();
        }
        let mut outcome = UpdateOutcome::default();

        // Workspace.swift:5030-5034
        if self.titles.get(&panel_id).map(String::as_str) != Some(trimmed) {
            self.titles.insert(panel_id, trimmed.to_string());
            outcome.did_mutate = true;
        }

        // Workspace.swift:5036-5047 — tab push only when this panel's title
        // changed (didMutate == didMutatePanelTitle at this point).
        if outcome.did_mutate {
            let base_title = self
                .titles
                .get(&panel_id)
                .cloned()
                .unwrap_or_else(|| display_title.to_string());
            outcome.tab_sync = Some(TabSync {
                title: self.resolved_panel_title(panel_id, &base_title),
                has_custom_title: self.custom_titles.contains_key(&panel_id),
            });
        }

        // Workspace.swift:5049-5059 — single-panel workspace-title
        // propagation; independent of whether the panel title changed.
        if self.panel_ids.len() == 1 && !workspace.has_custom_title {
            if workspace.title != trimmed {
                outcome.workspace_title = Some(trimmed.to_string());
                outcome.did_mutate = true;
            }
            if workspace.process_title != trimmed {
                // Does NOT set didMutate (Workspace.swift:5056-5058).
                outcome.process_title = Some(trimmed.to_string());
            }
        }

        outcome
    }

    /// Mirrors `Workspace.setPanelCustomTitle(panelId:title:source:)`
    /// (`Sources/Workspace.swift:4049-4091`) — the provenance state machine.
    ///
    /// `.auto` writes are rejected when a user-owned title exists (missing
    /// provenance normalizes to user, `:4060`), and `.auto` never clears
    /// (`:4059`). A same-text user write re-claims ownership and returns
    /// applied with NO tab push (`:4067-4072`). `display_title` is
    /// `panel.displayTitle`, the `panelTitles` fallback at `:4078`.
    pub fn set_panel_custom_title(
        &mut self,
        panel_id: Uuid,
        title: Option<&str>,
        source: CustomTitleSource,
        display_title: &str,
    ) -> SetOutcome {
        // Workspace.swift:4055
        if !self.panel_ids.contains(&panel_id) {
            return SetOutcome::default();
        }
        let trimmed = trim_whitespace_and_newlines(title.unwrap_or(""));
        let previous = self.custom_titles.get(&panel_id).cloned();

        // Workspace.swift:4058-4061
        if source == CustomTitleSource::Auto {
            if trimmed.is_empty() {
                return SetOutcome::default();
            }
            if previous.is_some()
                && self
                    .custom_sources
                    .get(&panel_id)
                    .copied()
                    .unwrap_or(CustomTitleSource::User)
                    == CustomTitleSource::User
            {
                return SetOutcome::default();
            }
        }

        if trimmed.is_empty() {
            // Workspace.swift:4062-4065 — user clear.
            if previous.is_none() {
                return SetOutcome::default();
            }
            self.custom_titles.remove(&panel_id);
            self.custom_sources.remove(&panel_id);
        } else {
            if previous.as_deref() == Some(trimmed) {
                // Workspace.swift:4067-4072 — same text: a user write still
                // claims ownership; early return with no tab push.
                if source == CustomTitleSource::User {
                    self.custom_sources
                        .insert(panel_id, CustomTitleSource::User);
                }
                return SetOutcome {
                    applied: true,
                    tab_sync: None,
                    tmux_rename: None,
                };
            }
            self.custom_titles.insert(panel_id, trimmed.to_string());
            self.custom_sources.insert(panel_id, source);
        }

        // Workspace.swift:4077-4089 — push after a real mutation.
        let base_title = self
            .titles
            .get(&panel_id)
            .cloned()
            .unwrap_or_else(|| display_title.to_string());
        SetOutcome {
            applied: true,
            tab_sync: Some(TabSync {
                title: self.resolved_panel_title(panel_id, &base_title),
                has_custom_title: self.custom_titles.contains_key(&panel_id),
            }),
            tmux_rename: Some(trimmed.to_string()),
        }
    }

    /// Mirrors the title-related restore path of
    /// `Workspace.applySessionPanelMetadata`
    /// (`Sources/Workspace.swift:1518-1536`): a trimmed non-empty snapshot
    /// title lands in `panelTitles` (the TRIMMED value, unguarded on panel
    /// existence, `:1519-1521`); then `setPanelCustomTitle` with
    /// `snapshot.customTitleSource ?? .user` (`:1523`); then an
    /// unconditional restored-title push (`:1530-1536`).
    pub fn apply_session_panel_metadata(
        &mut self,
        panel_id: Uuid,
        snapshot_title: Option<&str>,
        snapshot_custom_title: Option<&str>,
        snapshot_custom_title_source: Option<CustomTitleSource>,
        display_title: &str,
    ) -> RestoreOutcome {
        if let Some(raw) = snapshot_title {
            let trimmed = trim_whitespace_and_newlines(raw);
            if !trimmed.is_empty() {
                self.titles.insert(panel_id, trimmed.to_string());
            }
        }

        let set = self.set_panel_custom_title(
            panel_id,
            snapshot_custom_title,
            snapshot_custom_title_source.unwrap_or(CustomTitleSource::User),
            display_title,
        );

        let tab_sync = if self.panel_ids.contains(&panel_id) {
            let fallback = self
                .titles
                .get(&panel_id)
                .cloned()
                .unwrap_or_else(|| display_title.to_string());
            Some(TabSync {
                title: self.resolved_panel_title(panel_id, &fallback),
                has_custom_title: self.custom_titles.contains_key(&panel_id),
            })
        } else {
            None
        };

        RestoreOutcome {
            custom_applied: set.applied,
            tmux_rename: set.tmux_rename,
            tab_sync,
        }
    }

    /// Mirrors the title-map pruning in `Workspace.pruneSurfaceMetadata`
    /// (`Sources/Workspace.swift:5079-5081`): filter all three maps to valid
    /// surface ids. NOTE: Swift does not prune the `panels` map there, so
    /// `panel_ids` is intentionally untouched; use [`Self::remove_panel`]
    /// when the panel itself is removed.
    pub fn prune(&mut self, valid_surface_ids: &HashSet<Uuid>) {
        self.titles.retain(|id, _| valid_surface_ids.contains(id));
        self.custom_titles
            .retain(|id, _| valid_surface_ids.contains(id));
        self.custom_sources
            .retain(|id, _| valid_surface_ids.contains(id));
    }

    /// Drop a panel from the existence set (the `panels` map removal that
    /// happens outside `pruneSurfaceMetadata`).
    pub fn remove_panel(&mut self, panel_id: Uuid) {
        self.panel_ids.remove(&panel_id);
    }

    /// TEST/CARRY seam: write `panelCustomTitles[id]` directly without
    /// provenance, as move/respawn carry flows do (see
    /// `cmuxTests/WorkspaceTitleProvenanceTests.swift:119-123`). Legacy
    /// entries without provenance are user-owned per `Workspace.swift:4060`.
    pub fn insert_carried_custom_title(&mut self, panel_id: Uuid, title: &str) {
        self.custom_titles.insert(panel_id, title.to_string());
    }
}

/// Pure core of the `TabManager` pending-title coalescer
/// (`Sources/TabManager.swift:3286-3319`): last-write-wins per
/// `(workspace/tab id, panel id)` key, trimmed, empty-after-trim dropped.
/// The surface-identity (`===`) gates and the debounce timer live with the
/// caller.
#[derive(Debug, Clone, Default)]
pub struct PendingTitleUpdates {
    pending: HashMap<(Uuid, Uuid), String>,
}

impl PendingTitleUpdates {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mirrors `enqueuePanelTitleUpdate` (`Sources/TabManager.swift:
    /// 3286-3305`): trim; DROP if empty (an OSC clearing the title never
    /// clears the label — the last non-empty title sticks, `:3287-3288`);
    /// last-write-wins per `(tabId, panelId)` key (`:3298-3299`). Returns
    /// whether the update was stored.
    pub fn enqueue(&mut self, workspace_id: Uuid, panel_id: Uuid, raw_title: &str) -> bool {
        let trimmed = trim_whitespace_and_newlines(raw_title);
        if trimmed.is_empty() {
            return false;
        }
        self.pending
            .insert((workspace_id, panel_id), trimmed.to_string());
        true
    }

    /// Mirrors `flushPendingPanelTitleUpdates` (`Sources/TabManager.swift:
    /// 3306-3319`): take-all-and-clear. Iteration order of the returned map
    /// is unspecified, exactly as Swift Dictionary iteration is.
    pub fn drain(&mut self) -> HashMap<(Uuid, Uuid), String> {
        std::mem::take(&mut self.pending)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }
}

/// Coalescing delay in seconds, mirroring
/// `PanelTitleUpdateCoalescingSettings.delay`
/// (`Sources/PanelTitleUpdateCoalescingSettings.swift:6-8`, `:18-23`):
/// disabled → `1.0/30.0`; enabled → `clamp(configured_ms, 33, 5000) / 1000`.
pub fn coalescing_delay_seconds(enabled: bool, configured_ms: i64) -> f64 {
    if !enabled {
        return 1.0 / 30.0;
    }
    let clamped = configured_ms.clamp(33, 5000);
    clamped as f64 / 1000.0
}

/// Mirrors `PanelTitleUpdateCoalescingSettings.configuredDelayMilliseconds`
/// (`Sources/PanelTitleUpdateCoalescingSettings.swift:25-27`):
/// `(delay * 1000).rounded()` — Swift `.rounded()` is round-half-away-from-
/// zero, same as `f64::round`.
pub fn configured_delay_milliseconds(enabled: bool, configured_ms: i64) -> i64 {
    (coalescing_delay_seconds(enabled, configured_ms) * 1000.0).round() as i64
}

/// The v2 auto-naming per-panel gate
/// (`Sources/TerminalController.swift:3311-3313`): the panel-level `.auto`
/// write is skipped when `panel_only_if_multiple` is requested and the
/// workspace has fewer than two panels.
pub fn auto_naming_panel_gate(panel_only_if_multiple: bool, panel_count: usize) -> bool {
    !(panel_only_if_multiple && panel_count < 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    const NO_WORKSPACE_PROPAGATION: WorkspaceTitleContext<'_> = WorkspaceTitleContext {
        has_custom_title: true, // blocks the single-panel branch
        title: "",
        process_title: "",
    };

    // ---- (1) trim-set edges -------------------------------------------

    #[test]
    fn trim_oracle_table() {
        // (input, expected) pinned to Swift .whitespacesAndNewlines.
        let cases: &[(&str, &str)] = &[
            (" x ", "x"),
            ("\tx\n", "x"),
            ("\u{0085}x\u{0085}", "x"), // NEL trimmed
            ("\u{00A0}x", "x"),         // NBSP trimmed (Zs)
            ("\u{1680}x", "x"),         // OGHAM SPACE MARK
            ("\u{2000}x\u{200A}", "x"), // Zs range ends
            ("\u{2028}x", "x"),         // LINE SEPARATOR (Zl)
            ("\u{2029}x", "x"),         // PARAGRAPH SEPARATOR (Zp)
            ("\u{202F}x", "x"),         // NARROW NO-BREAK SPACE
            ("\u{205F}x", "x"),         // MEDIUM MATHEMATICAL SPACE
            ("\u{3000}x", "x"),         // IDEOGRAPHIC SPACE
            ("\u{200B}x", "\u{200B}x"), // ZWSP NOT trimmed
            ("\u{FEFF}x", "\u{FEFF}x"), // BOM NOT trimmed
            ("\u{180E}x", "\u{180E}x"), // MVS NOT trimmed
            ("a b", "a b"),             // interior whitespace preserved
            (" a\u{00A0}b ", "a\u{00A0}b"),
            ("", ""),
            (" \n\t\u{3000}", ""),
        ];
        for (input, expected) in cases {
            assert_eq!(
                trim_whitespace_and_newlines(input),
                *expected,
                "input {input:?}"
            );
        }
    }

    // ---- (2) resolvedPanelTitle ----------------------------------------

    #[test]
    fn resolved_title_empty_fallback_is_tab() {
        let mut state = PanelTitleState::new();
        state.insert_panel(id(1));
        assert_eq!(state.resolved_panel_title(id(1), ""), "Tab");
        assert_eq!(state.resolved_panel_title(id(1), "  \n "), "Tab");
    }

    #[test]
    fn resolved_title_trims_fallback() {
        let state = PanelTitleState::new();
        assert_eq!(state.resolved_panel_title(id(1), "  vim  "), "vim");
    }

    #[test]
    fn resolved_title_custom_wins_over_fallback() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.set_panel_custom_title(
            id(1),
            Some("Build Pane"),
            CustomTitleSource::User,
            "Terminal",
        );
        assert_eq!(state.resolved_panel_title(id(1), "zsh"), "Build Pane");
    }

    #[test]
    fn resolved_title_whitespace_only_custom_falls_through() {
        // Defensive re-trim at Workspace.swift:3924-3925: a carried custom
        // title of pure whitespace is ignored in resolution.
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.insert_carried_custom_title(id(1), " \u{3000} ");
        assert_eq!(state.resolved_panel_title(id(1), "zsh"), "zsh");
        assert_eq!(state.resolved_panel_title(id(1), "  "), "Tab");
    }

    // ---- (3) provenance machine ----------------------------------------
    // Replicates cmuxTests/WorkspaceTitleProvenanceTests.swift:89-124.

    #[test]
    fn provenance_auto_sets_when_unset() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        let out = state.set_panel_custom_title(
            id(1),
            Some("Fix auth bug"),
            CustomTitleSource::Auto,
            "Terminal",
        );
        assert!(out.applied);
        assert_eq!(state.custom_title(id(1)), Some("Fix auth bug"));
        assert_eq!(
            state.custom_title_source(id(1)),
            Some(CustomTitleSource::Auto)
        );
    }

    #[test]
    fn provenance_user_overwrites_auto_and_claims_ownership() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.set_panel_custom_title(
            id(1),
            Some("Fix auth bug"),
            CustomTitleSource::Auto,
            "Terminal",
        );
        let out = state.set_panel_custom_title(
            id(1),
            Some("Build Pane"),
            CustomTitleSource::User,
            "Terminal",
        );
        assert!(out.applied);
        assert_eq!(
            state.custom_title_source(id(1)),
            Some(CustomTitleSource::User)
        );
        // Auto over user is rejected.
        let out =
            state.set_panel_custom_title(id(1), Some("Other"), CustomTitleSource::Auto, "Terminal");
        assert!(!out.applied);
        assert_eq!(out.tab_sync, None);
        assert_eq!(state.custom_title(id(1)), Some("Build Pane"));
    }

    #[test]
    fn provenance_auto_refreshes_auto() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.set_panel_custom_title(
            id(1),
            Some("Fix auth bug"),
            CustomTitleSource::Auto,
            "Terminal",
        );
        let out = state.set_panel_custom_title(
            id(1),
            Some("Debug login flow"),
            CustomTitleSource::Auto,
            "Terminal",
        );
        assert!(out.applied);
        assert_eq!(state.custom_title(id(1)), Some("Debug login flow"));
        assert_eq!(
            state.custom_title_source(id(1)),
            Some(CustomTitleSource::Auto)
        );
    }

    #[test]
    fn provenance_user_clear_resets_and_reopens_auto() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.set_panel_custom_title(
            id(1),
            Some("Build Pane"),
            CustomTitleSource::User,
            "Terminal",
        );
        let out = state.set_panel_custom_title(id(1), None, CustomTitleSource::User, "Terminal");
        assert!(out.applied);
        assert_eq!(state.custom_title(id(1)), None);
        assert_eq!(state.custom_title_source(id(1)), None);
        // Clear propagates an empty tmux rename string (Workspace.swift:4087).
        assert_eq!(out.tmux_rename.as_deref(), Some(""));
        // Auto re-set after clear lands.
        let out = state.set_panel_custom_title(
            id(1),
            Some("Refreshed"),
            CustomTitleSource::Auto,
            "Terminal",
        );
        assert!(out.applied);
        assert_eq!(
            state.custom_title_source(id(1)),
            Some(CustomTitleSource::Auto)
        );
    }

    #[test]
    fn provenance_auto_never_clears() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.set_panel_custom_title(
            id(1),
            Some("Fix auth bug"),
            CustomTitleSource::Auto,
            "Terminal",
        );
        assert!(
            !state
                .set_panel_custom_title(id(1), None, CustomTitleSource::Auto, "Terminal")
                .applied
        );
        assert!(
            !state
                .set_panel_custom_title(id(1), Some("   "), CustomTitleSource::Auto, "Terminal")
                .applied
        );
        assert_eq!(state.custom_title(id(1)), Some("Fix auth bug"));
    }

    #[test]
    fn provenance_user_clear_without_custom_is_noop() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        let out = state.set_panel_custom_title(id(1), None, CustomTitleSource::User, "Terminal");
        assert!(!out.applied);
        assert_eq!(out.tab_sync, None);
        assert_eq!(out.tmux_rename, None);
    }

    #[test]
    fn provenance_same_text_user_reconfirm_claims_ownership_no_sync() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.set_panel_custom_title(id(1), Some("Name"), CustomTitleSource::Auto, "Terminal");
        // Same text, user source: applied, source flips to user, NO pushes.
        let out =
            state.set_panel_custom_title(id(1), Some("Name"), CustomTitleSource::User, "Terminal");
        assert!(out.applied);
        assert_eq!(out.tab_sync, None);
        assert_eq!(out.tmux_rename, None);
        assert_eq!(
            state.custom_title_source(id(1)),
            Some(CustomTitleSource::User)
        );
        // Ownership claimed: a later auto write is rejected.
        assert!(
            !state
                .set_panel_custom_title(id(1), Some("Other"), CustomTitleSource::Auto, "Terminal")
                .applied
        );
    }

    #[test]
    fn provenance_same_text_auto_reconfirm_keeps_auto_source() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.set_panel_custom_title(id(1), Some("Name"), CustomTitleSource::Auto, "Terminal");
        let out =
            state.set_panel_custom_title(id(1), Some("Name"), CustomTitleSource::Auto, "Terminal");
        assert!(out.applied);
        assert_eq!(out.tab_sync, None);
        assert_eq!(
            state.custom_title_source(id(1)),
            Some(CustomTitleSource::Auto)
        );
    }

    #[test]
    fn provenance_carried_title_without_source_is_user_owned() {
        // WorkspaceTitleProvenanceTests.swift:113-124.
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.insert_carried_custom_title(id(1), "Carried Tab");
        assert!(
            !state
                .set_panel_custom_title(id(1), Some("Other"), CustomTitleSource::Auto, "Terminal")
                .applied
        );
        assert_eq!(state.custom_title(id(1)), Some("Carried Tab"));
    }

    #[test]
    fn provenance_unknown_panel_rejected() {
        let mut state = PanelTitleState::new();
        let out =
            state.set_panel_custom_title(id(9), Some("X"), CustomTitleSource::User, "Terminal");
        assert!(!out.applied);
    }

    #[test]
    fn set_custom_title_trims_and_syncs_tab() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        let out = state.set_panel_custom_title(
            id(1),
            Some("  Build Pane \n"),
            CustomTitleSource::User,
            "Terminal",
        );
        assert!(out.applied);
        assert_eq!(
            out.tab_sync,
            Some(TabSync {
                title: "Build Pane".to_string(),
                has_custom_title: true
            })
        );
        assert_eq!(out.tmux_rename.as_deref(), Some("Build Pane"));
    }

    #[test]
    fn set_custom_title_tab_sync_and_tmux_rename_gate_together() {
        // Workspace.swift:4077: ONE guard (`guard let panel …, let tabId …`)
        // gates BOTH the updateTab push (:4079-4083) AND the remote-tmux
        // rename-window propagation (:4084-4089) — an unbound panel emits
        // NEITHER. The pure model has no tab-binding table (sanctioned
        // divergence, module doc), so the contract it must uphold is that
        // tab_sync and tmux_rename are Some/None TOGETHER on every path,
        // letting the binding-owning caller drop the pair as one unit.
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));

        // Unknown panel: neither (Workspace.swift:4055).
        let out =
            state.set_panel_custom_title(id(9), Some("X"), CustomTitleSource::User, "Terminal");
        assert_eq!(out.tab_sync, None);
        assert_eq!(out.tmux_rename, None);

        // Real set: both (:4077-4089).
        let out =
            state.set_panel_custom_title(id(1), Some("Name"), CustomTitleSource::User, "Terminal");
        assert!(out.tab_sync.is_some());
        assert_eq!(out.tmux_rename.as_deref(), Some("Name"));

        // Same-text early return: neither (:4067-4072).
        let out =
            state.set_panel_custom_title(id(1), Some("Name"), CustomTitleSource::User, "Terminal");
        assert_eq!(out.tab_sync, None);
        assert_eq!(out.tmux_rename, None);

        // Rejected auto over user: neither (:4058-4061).
        let out =
            state.set_panel_custom_title(id(1), Some("Other"), CustomTitleSource::Auto, "Terminal");
        assert_eq!(out.tab_sync, None);
        assert_eq!(out.tmux_rename, None);

        // Real clear: both, rename is "" (:4062-4065, :4084-4089).
        let out = state.set_panel_custom_title(id(1), None, CustomTitleSource::User, "Terminal");
        assert!(out.tab_sync.is_some());
        assert_eq!(out.tmux_rename.as_deref(), Some(""));

        // No-op clear: neither (:4063).
        let out = state.set_panel_custom_title(id(1), None, CustomTitleSource::User, "Terminal");
        assert_eq!(out.tab_sync, None);
        assert_eq!(out.tmux_rename, None);
    }

    #[test]
    fn clear_custom_title_syncs_tab_back_to_process_title() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.update_panel_title(id(1), "vim", NO_WORKSPACE_PROPAGATION, "Terminal");
        state.set_panel_custom_title(
            id(1),
            Some("Build Pane"),
            CustomTitleSource::User,
            "Terminal",
        );
        let out = state.set_panel_custom_title(id(1), None, CustomTitleSource::User, "Terminal");
        assert!(out.applied);
        assert_eq!(
            out.tab_sync,
            Some(TabSync {
                title: "vim".to_string(),
                has_custom_title: false
            })
        );
    }

    // ---- (4) update_panel_title ----------------------------------------

    #[test]
    fn update_empty_or_whitespace_is_noop() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        for raw in ["", "   ", "\n\t", "\u{3000}\u{00A0}"] {
            let out = state.update_panel_title(id(1), raw, NO_WORKSPACE_PROPAGATION, "Terminal");
            assert_eq!(out, UpdateOutcome::default(), "raw {raw:?}");
        }
        assert_eq!(state.raw_title(id(1)), Some("Terminal"));
    }

    #[test]
    fn update_unknown_panel_is_noop() {
        let mut state = PanelTitleState::new();
        let out = state.update_panel_title(id(9), "vim", NO_WORKSPACE_PROPAGATION, "Terminal");
        assert_eq!(out, UpdateOutcome::default());
        assert_eq!(state.raw_title(id(9)), None);
    }

    #[test]
    fn update_unchanged_value_no_mutation_no_sync() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        // Seeded "Terminal"; writing "Terminal" again changes nothing.
        let out =
            state.update_panel_title(id(1), " Terminal ", NO_WORKSPACE_PROPAGATION, "Terminal");
        assert!(!out.did_mutate);
        assert_eq!(out.tab_sync, None);
    }

    #[test]
    fn update_changed_value_syncs_resolved_title() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        let out = state.update_panel_title(id(1), " vim \n", NO_WORKSPACE_PROPAGATION, "Terminal");
        assert!(out.did_mutate);
        assert_eq!(
            out.tab_sync,
            Some(TabSync {
                title: "vim".to_string(),
                has_custom_title: false
            })
        );
        assert_eq!(state.raw_title(id(1)), Some("vim"));
    }

    #[test]
    fn update_with_custom_title_masks_label_but_updates_underneath() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.set_panel_custom_title(
            id(1),
            Some("Build Pane"),
            CustomTitleSource::User,
            "Terminal",
        );
        let out = state.update_panel_title(id(1), "vim", NO_WORKSPACE_PROPAGATION, "Terminal");
        assert!(out.did_mutate);
        // The label stays the custom title...
        assert_eq!(
            out.tab_sync,
            Some(TabSync {
                title: "Build Pane".to_string(),
                has_custom_title: true
            })
        );
        // ...but panelTitles updated underneath, visible via the raw
        // rename-prompt chain once the custom title is cleared.
        assert_eq!(state.raw_title(id(1)), Some("vim"));
        state.set_panel_custom_title(id(1), None, CustomTitleSource::User, "Terminal");
        assert_eq!(
            state.rename_prompt_seed(id(1), "Terminal").as_deref(),
            Some("vim")
        );
    }

    #[test]
    fn update_single_panel_propagates_workspace_title() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        let ctx = WorkspaceTitleContext {
            has_custom_title: false,
            title: "Terminal",
            process_title: "",
        };
        let out = state.update_panel_title(id(1), "vim", ctx, "Terminal");
        assert!(out.did_mutate);
        assert_eq!(out.workspace_title.as_deref(), Some("vim"));
        assert_eq!(out.process_title.as_deref(), Some("vim"));
    }

    #[test]
    fn update_panel_title_unchanged_but_workspace_differs_returns_true() {
        // Workspace.swift:5049-5059 runs even when the panel title itself
        // did not change: "panel title unchanged but workspace title
        // differs -> returns true".
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.update_panel_title(
            id(1),
            "vim",
            WorkspaceTitleContext {
                has_custom_title: false,
                title: "vim",
                process_title: "vim",
            },
            "Terminal",
        );
        let ctx = WorkspaceTitleContext {
            has_custom_title: false,
            title: "Terminal", // workspace title diverged
            process_title: "vim",
        };
        let out = state.update_panel_title(id(1), "vim", ctx, "Terminal");
        assert!(out.did_mutate, "workspace-title propagation sets didMutate");
        assert_eq!(out.tab_sync, None, "panel title unchanged -> no tab push");
        assert_eq!(out.workspace_title.as_deref(), Some("vim"));
        assert_eq!(out.process_title, None, "processTitle already matches");
    }

    #[test]
    fn update_process_title_only_does_not_set_did_mutate() {
        // Workspace.swift:5056-5058: processTitle write omits didMutate.
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.update_panel_title(
            id(1),
            "vim",
            WorkspaceTitleContext {
                has_custom_title: false,
                title: "vim",
                process_title: "vim",
            },
            "Terminal",
        );
        let ctx = WorkspaceTitleContext {
            has_custom_title: false,
            title: "vim",         // workspace title matches
            process_title: "zsh", // processTitle diverged
        };
        let out = state.update_panel_title(id(1), "vim", ctx, "Terminal");
        assert!(!out.did_mutate);
        assert_eq!(out.process_title.as_deref(), Some("vim"));
    }

    #[test]
    fn update_multi_panel_never_propagates_workspace_title() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.seed_terminal_panel(id(2));
        let ctx = WorkspaceTitleContext {
            has_custom_title: false,
            title: "Terminal",
            process_title: "",
        };
        let out = state.update_panel_title(id(1), "vim", ctx, "Terminal");
        assert!(out.did_mutate);
        assert_eq!(out.workspace_title, None);
        assert_eq!(out.process_title, None);
    }

    #[test]
    fn update_workspace_custom_title_blocks_propagation() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        let ctx = WorkspaceTitleContext {
            has_custom_title: true,
            title: "Terminal",
            process_title: "",
        };
        let out = state.update_panel_title(id(1), "vim", ctx, "Terminal");
        assert!(out.did_mutate);
        assert_eq!(out.workspace_title, None);
        assert_eq!(out.process_title, None);
    }

    // ---- panelTitle / rename seed ---------------------------------------

    #[test]
    fn panel_title_none_for_unknown_panel() {
        let state = PanelTitleState::new();
        assert_eq!(state.panel_title(id(9), "Terminal"), None);
    }

    #[test]
    fn panel_title_resolution_chain() {
        let mut state = PanelTitleState::new();
        state.insert_panel(id(1)); // no seeded title
        assert_eq!(
            state.panel_title(id(1), "Terminal").as_deref(),
            Some("Terminal")
        );
        state.seed_terminal_panel(id(2));
        state.update_panel_title(id(2), "vim", NO_WORKSPACE_PROPAGATION, "Terminal");
        assert_eq!(state.panel_title(id(2), "Terminal").as_deref(), Some("vim"));
        state.set_panel_custom_title(id(2), Some("Build"), CustomTitleSource::User, "Terminal");
        assert_eq!(
            state.panel_title(id(2), "Terminal").as_deref(),
            Some("Build")
        );
    }

    #[test]
    fn rename_prompt_seed_raw_chain() {
        let mut state = PanelTitleState::new();
        assert_eq!(state.rename_prompt_seed(id(1), "Terminal"), None);
        state.insert_panel(id(1));
        assert_eq!(
            state.rename_prompt_seed(id(1), "Terminal").as_deref(),
            Some("Terminal")
        );
        state.seed_terminal_panel(id(1));
        state.update_panel_title(id(1), "vim", NO_WORKSPACE_PROPAGATION, "Terminal");
        assert_eq!(
            state.rename_prompt_seed(id(1), "Terminal").as_deref(),
            Some("vim")
        );
        state.set_panel_custom_title(id(1), Some("Build"), CustomTitleSource::User, "Terminal");
        assert_eq!(
            state.rename_prompt_seed(id(1), "Terminal").as_deref(),
            Some("Build")
        );
    }

    // ---- (5) seed + prune + restore -------------------------------------

    #[test]
    fn seed_terminal_panel_seeds_terminal() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        assert_eq!(state.raw_title(id(1)), Some("Terminal"));
        assert_eq!(
            state.panel_title(id(1), "Terminal").as_deref(),
            Some("Terminal")
        );
    }

    #[test]
    fn prune_drops_stale_keys_in_all_three_maps() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.seed_terminal_panel(id(2));
        state.set_panel_custom_title(id(1), Some("A"), CustomTitleSource::User, "Terminal");
        state.set_panel_custom_title(id(2), Some("B"), CustomTitleSource::Auto, "Terminal");
        let valid: HashSet<Uuid> = [id(2)].into_iter().collect();
        state.prune(&valid);
        assert_eq!(state.raw_title(id(1)), None);
        assert_eq!(state.custom_title(id(1)), None);
        assert_eq!(state.custom_title_source(id(1)), None);
        assert_eq!(state.raw_title(id(2)), Some("Terminal"));
        assert_eq!(state.custom_title(id(2)), Some("B"));
        assert_eq!(
            state.custom_title_source(id(2)),
            Some(CustomTitleSource::Auto)
        );
    }

    #[test]
    fn restore_stores_trimmed_title_and_defaults_source_to_user() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        let out = state.apply_session_panel_metadata(
            id(1),
            Some("  vim  "),
            Some("Build"),
            None, // legacy snapshot: no provenance -> .user
            "Terminal",
        );
        // Trimmed title stored (Workspace.swift:1519-1521 stores the
        // TRIMMED value).
        assert_eq!(state.raw_title(id(1)), Some("vim"));
        assert!(out.custom_applied);
        assert_eq!(
            state.custom_title_source(id(1)),
            Some(CustomTitleSource::User)
        );
        assert_eq!(
            out.tab_sync,
            Some(TabSync {
                title: "Build".to_string(),
                has_custom_title: true
            })
        );
        // Legacy user-owned restore blocks later auto writes.
        assert!(
            !state
                .set_panel_custom_title(id(1), Some("Other"), CustomTitleSource::Auto, "Terminal")
                .applied
        );
    }

    #[test]
    fn restore_without_custom_title_still_pushes_tab_sync() {
        // Workspace.swift:1526-1536: setPanelCustomTitle skips the sync when
        // there is no custom title, so the restore pushes unconditionally.
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        let out = state.apply_session_panel_metadata(id(1), Some("vim"), None, None, "Terminal");
        assert!(!out.custom_applied);
        assert_eq!(out.tmux_rename, None);
        assert_eq!(
            out.tab_sync,
            Some(TabSync {
                title: "vim".to_string(),
                has_custom_title: false
            })
        );
    }

    #[test]
    fn restore_whitespace_snapshot_title_is_ignored() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        let out = state.apply_session_panel_metadata(id(1), Some("  \n "), None, None, "Terminal");
        assert_eq!(state.raw_title(id(1)), Some("Terminal"));
        assert_eq!(
            out.tab_sync,
            Some(TabSync {
                title: "Terminal".to_string(),
                has_custom_title: false
            })
        );
    }

    #[test]
    fn restore_preserves_auto_provenance() {
        let mut state = PanelTitleState::new();
        state.seed_terminal_panel(id(1));
        state.apply_session_panel_metadata(
            id(1),
            None,
            Some("Fix auth bug"),
            Some(CustomTitleSource::Auto),
            "Terminal",
        );
        assert_eq!(
            state.custom_title_source(id(1)),
            Some(CustomTitleSource::Auto)
        );
        // Auto provenance restored: a later auto refresh lands.
        assert!(
            state
                .set_panel_custom_title(id(1), Some("Other"), CustomTitleSource::Auto, "Terminal")
                .applied
        );
    }

    // ---- (6) coalescer ---------------------------------------------------

    #[test]
    fn coalescer_last_write_wins_per_key() {
        let mut pending = PendingTitleUpdates::new();
        assert!(pending.enqueue(id(10), id(1), " first "));
        assert!(pending.enqueue(id(10), id(1), "second"));
        assert!(pending.enqueue(id(10), id(2), "other"));
        assert_eq!(pending.len(), 2);
        let drained = pending.drain();
        assert_eq!(
            drained.get(&(id(10), id(1))).map(String::as_str),
            Some("second")
        );
        assert_eq!(
            drained.get(&(id(10), id(2))).map(String::as_str),
            Some("other")
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn coalescer_rejects_empty_after_trim() {
        let mut pending = PendingTitleUpdates::new();
        assert!(!pending.enqueue(id(10), id(1), ""));
        assert!(!pending.enqueue(id(10), id(1), " \n\u{00A0} "));
        assert!(pending.is_empty());
        // An empty OSC after a real one does NOT clear the pending entry.
        assert!(pending.enqueue(id(10), id(1), "vim"));
        assert!(!pending.enqueue(id(10), id(1), "   "));
        assert_eq!(
            pending.drain().get(&(id(10), id(1))).map(String::as_str),
            Some("vim")
        );
    }

    #[test]
    fn coalescing_delay_clamp_table() {
        // (enabled, configured_ms, expected_seconds)
        let cases: &[(bool, i64, f64)] = &[
            (false, 0, 1.0 / 30.0),
            (false, 9999, 1.0 / 30.0),
            (true, 32, 0.033),
            (true, 33, 0.033),
            (true, 100, 0.1),
            (true, 5000, 5.0),
            (true, 5001, 5.0),
            (true, -1, 0.033),
        ];
        for (enabled, ms, expected) in cases {
            let got = coalescing_delay_seconds(*enabled, *ms);
            assert!(
                (got - expected).abs() < 1e-12,
                "({enabled}, {ms}) -> {got}, expected {expected}"
            );
        }
        assert_eq!(configured_delay_milliseconds(false, 0), 33); // (1/30*1000).rounded()
        assert_eq!(configured_delay_milliseconds(true, 32), 33);
        assert_eq!(configured_delay_milliseconds(true, 5001), 5000);
        assert_eq!(configured_delay_milliseconds(true, 100), 100);
    }

    // ---- (7) CustomTitleSource strings ----------------------------------

    #[test]
    fn custom_title_source_raw_values_round_trip() {
        assert_eq!(CustomTitleSource::User.as_str(), "user");
        assert_eq!(CustomTitleSource::Auto.as_str(), "auto");
        assert_eq!(
            CustomTitleSource::from_raw("user"),
            Some(CustomTitleSource::User)
        );
        assert_eq!(
            CustomTitleSource::from_raw("auto"),
            Some(CustomTitleSource::Auto)
        );
        assert_eq!(CustomTitleSource::from_raw("USER"), None);
        assert_eq!(CustomTitleSource::from_raw(""), None);
        // Codable parity: encodes as the raw string.
        assert_eq!(
            serde_json::to_string(&CustomTitleSource::User).unwrap(),
            "\"user\""
        );
        assert_eq!(
            serde_json::from_str::<CustomTitleSource>("\"auto\"").unwrap(),
            CustomTitleSource::Auto
        );
        assert!(serde_json::from_str::<CustomTitleSource>("\"other\"").is_err());
    }

    // ---- secondary-writer gate -------------------------------------------

    #[test]
    fn auto_naming_panel_gate_table() {
        // TerminalController.swift:3312: skipped when panel_only_if_multiple
        // && panels.count < 2.
        assert!(auto_naming_panel_gate(false, 0));
        assert!(auto_naming_panel_gate(false, 1));
        assert!(!auto_naming_panel_gate(true, 0));
        assert!(!auto_naming_panel_gate(true, 1));
        assert!(auto_naming_panel_gate(true, 2));
    }
}
