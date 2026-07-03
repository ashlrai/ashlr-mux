use crate::descriptor::PaneMemoryDescriptor;
use crate::pane_key::PaneMemoryPaneKey;
use crate::warning::PaneMemoryWarning;

/// Result of summing a pane's process-tree memory off the main thread.
///
/// Port of Swift `PaneMemorySample` (`Sendable`). Divergences: `Int64` ->
/// `i64`; `[Int]` -> `Vec<i64>` (Swift `Int` is 64-bit on the target
/// platform); the computed `key` / `warning` properties -> the
/// [`PaneMemorySample::key`] / [`PaneMemorySample::warning`] methods. The
/// `residentBytes` / `memoryPressureProcessGroupIDs` fields are carried
/// verbatim as inert informational data (the decision core ignores them,
/// exactly as the Swift engine does).
#[derive(Debug, Clone)]
pub struct PaneMemorySample {
    pub descriptor: PaneMemoryDescriptor,
    /// Physical-footprint bytes summed across every process sharing the pane's
    /// controlling tty. This is what macOS aggregates for "out of application
    /// memory", so it is the signal the threshold is compared against.
    pub memory_bytes: i64,
    /// Resident bytes summed across the same process set (informational).
    pub resident_bytes: i64,
    /// Process-group ids that contribute enough memory to clear this pane's warning.
    pub memory_pressure_process_group_ids: Vec<i64>,
    pub foreground_command: Option<String>,
}

impl PaneMemorySample {
    /// Faithful port of `var key: PaneMemoryPaneKey { descriptor.key }`.
    #[must_use]
    pub fn key(&self) -> PaneMemoryPaneKey {
        self.descriptor.key()
    }

    /// Faithful port of `var warning: PaneMemoryWarning`. Field ordering
    /// matches the Swift initializer exactly.
    #[must_use]
    pub fn warning(&self) -> PaneMemoryWarning {
        PaneMemoryWarning {
            workspace_id: self.descriptor.workspace_id,
            panel_id: self.descriptor.panel_id,
            workspace_title: self.descriptor.workspace_title.clone(),
            pane_title: self.descriptor.pane_title.clone(),
            memory_bytes: self.memory_bytes,
            foreground_command: self.foreground_command.clone(),
        }
    }
}
