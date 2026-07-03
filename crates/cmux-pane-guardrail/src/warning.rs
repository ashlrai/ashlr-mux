use uuid::Uuid;

use crate::pane_key::PaneMemoryPaneKey;

/// The content surfaced in the dismissible warning banner.
///
/// Port of Swift `PaneMemoryWarning` (`Equatable, Identifiable, Sendable`).
/// Divergences: `UUID` -> `uuid::Uuid`; `Int64` -> `i64`; `Equatable` ->
/// `#[derive(PartialEq, Eq)]`; the `Identifiable` `id` computed property ->
/// the [`PaneMemoryWarning::id`] method; the `key` computed property ->
/// [`PaneMemoryWarning::key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMemoryWarning {
    pub workspace_id: Uuid,
    pub panel_id: Uuid,
    pub workspace_title: String,
    pub pane_title: String,
    pub memory_bytes: i64,
    pub foreground_command: Option<String>,
}

impl PaneMemoryWarning {
    /// Faithful port of `var id: UUID { panelId }` (`Identifiable`).
    #[must_use]
    pub fn id(&self) -> Uuid {
        self.panel_id
    }

    /// Faithful port of `var key: PaneMemoryPaneKey`.
    #[must_use]
    pub fn key(&self) -> PaneMemoryPaneKey {
        PaneMemoryPaneKey {
            workspace_id: self.workspace_id,
            panel_id: self.panel_id,
        }
    }
}
