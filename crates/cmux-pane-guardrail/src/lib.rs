//! Faithful Rust port of the macOS Swift pane-memory guardrail decision core.
//!
//! Mirrors these Swift sources 1:1 (value types + the stateless-per-call
//! engine), with the only sanctioned divergence being the platform swap
//! (`Foundation.UUID` -> [`uuid::Uuid`], `mutating func` on a `struct` ->
//! `&mut self`, `Set`/`Dictionary` -> `HashSet`/`HashMap`):
//!
//! - `PaneMemoryPaneKey.swift` -> [`pane_key::PaneMemoryPaneKey`]
//! - `PaneMemoryDescriptor.swift` -> [`descriptor::PaneMemoryDescriptor`]
//! - `PaneMemorySample.swift` -> [`sample::PaneMemorySample`]
//! - `PaneMemoryWarning.swift` -> [`warning::PaneMemoryWarning`]
//! - `PaneMemoryGuardrailEngineOutput.swift` -> [`output::PaneMemoryGuardrailEngineOutput`]
//! - `PaneMemoryGuardrailSampleBatch.swift` -> [`sample_batch::PaneMemoryGuardrailSampleBatch`]
//! - `PaneMemoryGuardrailEngine.swift` -> [`engine::PaneMemoryGuardrailEngine`]
//!
//! The engine implements edge-triggered firing with hysteresis: a warning
//! fires only when a pane crosses `threshold_bytes` having been below before,
//! and a fired warning clears only once the pane drops below
//! `CLEAR_FRACTION * threshold` (0.8). Panes absent from a sample batch are
//! forgotten; `dismiss` suppresses re-fire until a pane clears and re-crosses;
//! `acknowledge_handled` drops a pane's tracked state entirely.

mod descriptor;
mod engine;
mod output;
mod pane_key;
mod sample;
mod sample_batch;
mod warning;

pub use descriptor::PaneMemoryDescriptor;
pub use engine::PaneMemoryGuardrailEngine;
pub use output::PaneMemoryGuardrailEngineOutput;
pub use pane_key::PaneMemoryPaneKey;
pub use sample::PaneMemorySample;
pub use sample_batch::PaneMemoryGuardrailSampleBatch;
pub use warning::PaneMemoryWarning;
