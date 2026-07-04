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
//! * [`tmux_overlay`] — pure geometry for the tmux-style pane overlay over a
//!   *flat* Bonsplit `LayoutSnapshot` (shares only [`PixelRect`] with the tree).
//! * [`surface_map`] — the bidirectional surface-id <-> panel-id map with the
//!   exclusive-by-panel invariant (from `PaneTreeModel`).

pub mod spatial_order;
pub mod surface_map;
pub mod tmux_overlay;
pub mod tree;

pub use surface_map::{PaneSurfaceMap, SurfaceId};
pub use tmux_overlay::{
    LayoutSnapshot, PaneGeometry, PaneID, Size, TmuxPaneLayoutPane, TmuxPaneLayoutReport,
    TmuxPaneOverlayGeometry,
};
pub use tree::{ExternalPaneNode, ExternalSplitNode, ExternalTreeNode, PixelRect};
