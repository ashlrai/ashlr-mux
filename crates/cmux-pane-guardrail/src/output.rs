use std::collections::HashSet;

use uuid::Uuid;

use crate::pane_key::PaneMemoryPaneKey;
use crate::warning::PaneMemoryWarning;

/// Port of Swift `PaneMemoryGuardrailEngineOutput` (`Equatable`).
///
/// Divergences: `Equatable` -> `#[derive(PartialEq, Eq)]`; Swift
/// `[PaneMemoryWarning]` -> `Vec` (order-dependent equality preserved,
/// matching Swift `Array` `Equatable`); Swift `Set<...>` -> `HashSet<...>`
/// (order-independent equality preserved, matching Swift `Set` `Equatable`);
/// `UUID` -> `uuid::Uuid`; the `bannerToPresent` computed property ->
/// [`PaneMemoryGuardrailEngineOutput::banner_to_present`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMemoryGuardrailEngineOutput {
    /// Panes that crossed the threshold this tick and whose banners have not
    /// been dismissed — present each once (edge-trigger).
    pub banners_to_present: Vec<PaneMemoryWarning>,
    /// Workspaces that currently own at least one warned pane (badge set).
    pub warned_workspace_ids: HashSet<Uuid>,
    /// Panes currently in warned state.
    pub warned_pane_keys: HashSet<PaneMemoryPaneKey>,
    /// Panes that dropped below the clear level this tick.
    pub cleared_panes: HashSet<PaneMemoryPaneKey>,
}

impl PaneMemoryGuardrailEngineOutput {
    /// Faithful port of `var bannerToPresent: PaneMemoryWarning? { bannersToPresent.first }`.
    ///
    /// Divergence: returns a borrow (`Option<&PaneMemoryWarning>`) rather than
    /// an owned optional copy; observable semantics for this read accessor are
    /// identical.
    #[must_use]
    pub fn banner_to_present(&self) -> Option<&PaneMemoryWarning> {
        self.banners_to_present.first()
    }
}
