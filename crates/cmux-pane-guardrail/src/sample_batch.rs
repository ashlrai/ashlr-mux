use std::collections::HashMap;

use crate::pane_key::PaneMemoryPaneKey;
use crate::sample::PaneMemorySample;

/// Port of Swift `PaneMemoryGuardrailSampleBatch` (`Sendable`).
///
/// Divergences: Swift `[PaneMemorySample]` -> `Vec`; Swift
/// `[PaneMemoryPaneKey: PaneMemorySample]` dictionary -> `HashMap`; `Bool` ->
/// `bool`. Carried verbatim; the decision core consumes `samples` directly.
#[derive(Debug, Clone)]
pub struct PaneMemoryGuardrailSampleBatch {
    pub samples: Vec<PaneMemorySample>,
    pub scoped_only_samples_by_key: HashMap<PaneMemoryPaneKey, PaneMemorySample>,
    pub includes_cmux_scope: bool,
}
