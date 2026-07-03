use uuid::Uuid;

use crate::pane_key::PaneMemoryPaneKey;

/// Main-actor snapshot of one live pane gathered before an off-main memory scan.
///
/// Port of Swift `PaneMemoryDescriptor` (`Sendable`). In the Swift app
/// `ttyName` / `foregroundPID` come from libghostty; here they are carried
/// verbatim as inert data. Divergences: `UUID` -> `uuid::Uuid`; Swift `Int?`
/// -> `Option<i64>` (Swift `Int` is 64-bit on the target platform); the
/// computed `key` property -> the [`PaneMemoryDescriptor::key`] method.
#[derive(Debug, Clone)]
pub struct PaneMemoryDescriptor {
    pub workspace_id: Uuid,
    pub panel_id: Uuid,
    pub workspace_title: String,
    pub pane_title: String,
    pub tty_name: Option<String>,
    pub foreground_pid: Option<i64>,
}

impl PaneMemoryDescriptor {
    /// Faithful port of Swift `var key: PaneMemoryPaneKey`.
    #[must_use]
    pub fn key(&self) -> PaneMemoryPaneKey {
        PaneMemoryPaneKey {
            workspace_id: self.workspace_id,
            panel_id: self.panel_id,
        }
    }
}
