//! Platform-neutral pane split-tree core for the cmux Windows port.
//!
//! Pure ports from the macOS `CmuxPanes` package: the `ExternalTreeNode` value
//! model (Bonsplit's immutable split-tree snapshot) plus the geometry/ordering
//! computed over it. Everything here is OS-independent and unit-tested on any
//! platform; the platform layer supplies the snapshot and applies the results.
//!
//! Currently ported:
//!
//! * [`tree`] — the `ExternalTreeNode` / `ExternalSplitNode` /
//!   `ExternalPaneNode` / `PixelRect` value model.
//! * [`spatial_order`] — on-screen spatial ordering (`orderedPaneIds` /
//!   `orderedPanelIds`), a depth-first walk of the tree.

pub mod spatial_order;
pub mod tree;

pub use tree::{ExternalPaneNode, ExternalSplitNode, ExternalTreeNode, PixelRect};
