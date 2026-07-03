use uuid::Uuid;

/// Stable identity of a single pane (workspace + panel) for guardrail tracking.
///
/// Port of Swift `PaneMemoryPaneKey` (`Hashable, Sendable`). Rust
/// divergence: Foundation `UUID` -> `uuid::Uuid`; `Hashable` ->
/// `#[derive(PartialEq, Eq, Hash)]`; `Sendable` is automatic (`Send`/`Sync`).
/// It is a plain value type, so `Copy` mirrors Swift's value semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneMemoryPaneKey {
    pub workspace_id: Uuid,
    pub panel_id: Uuid,
}

impl PaneMemoryPaneKey {
    #[must_use]
    pub fn new(workspace_id: Uuid, panel_id: Uuid) -> Self {
        Self {
            workspace_id,
            panel_id,
        }
    }
}
