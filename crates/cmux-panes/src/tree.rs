//! The `ExternalTreeNode` split-tree value model.
//!
//! A platform-neutral Rust port of Bonsplit's *external snapshot* value types —
//! the immutable tree the macOS app hands to `CmuxPanes` for pure geometry and
//! ordering. The Swift originals live in the `Bonsplit` package
//! (`vendor/bonsplit`, `ExternalTreeNode` / `ExternalSplitNode` /
//! `ExternalPaneNode` / `PixelRect`); their shape is pinned here from the two
//! `CmuxPanes` test fixtures that construct them verbatim:
//! `Packages/macOS/CmuxPanes/Tests/CmuxPanesTests/SpatialOrderTests.swift:7-9`
//! and `.../SplitGeometryTests.swift:9-32`.
//!
//! This is the shared anchor model for the whole `cmux-panes` crate: the
//! spatial-ordering port ([`crate::spatial_order`]) reads only pane `id`s and
//! the split `first`/`second` children, while the sibling split-geometry port
//! (equalize / keyboard-resize, ported later) additionally reads `frame`,
//! `orientation`, and `divider_position`.

use uuid::Uuid;

/// A pixel-space rectangle — Bonsplit's `PixelRect`, the `ExternalPaneNode.frame`
/// on-screen bounds. Components are `f64` to mirror Swift's `Double` (the
/// `CmuxPanes` fixtures declare `x/y/width/height: Double`, and
/// `ExternalTreeNode+SplitGeometry.swift:132-137` feeds them straight into
/// `CGRect`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelRect {
    /// Minimum-x (origin x).
    pub x: f64,
    /// Minimum-y (origin y).
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

impl PixelRect {
    /// Construct a rect from origin + size.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// A leaf pane — Bonsplit's `ExternalPaneNode`.
///
/// `id` is the stable pane identifier used by the ordering walk. `frame` is its
/// on-screen bounds. `tabs` / `selected_tab_id` carry the pane's panel set for
/// the geometry/chrome consumers; the spatial-order walk does not read them.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalPaneNode {
    /// Stable pane id (a `String`, matching Swift — *not* a parsed `UUID`).
    pub id: String,
    /// On-screen bounds of the pane.
    pub frame: PixelRect,
    /// The pane's panel ids in tab order.
    ///
    /// PARITY NOTE: the `Bonsplit` submodule is not checked out in this tree, so
    /// the element type is inferred (not source-verified) from the surrounding
    /// contract — `selected_tab_id` is a `UUID?` and `orderedPanelIds`'
    /// `paneTabs` is `[String: [UUID]]` — to be `Uuid`. The `CmuxPanes` fixtures
    /// only ever build this as `[]`, and no ported code path reads it, so the
    /// choice is behaviour-inert for the spatial-order port.
    pub tabs: Vec<Uuid>,
    /// The pane's selected panel id, if any.
    pub selected_tab_id: Option<Uuid>,
}

impl ExternalPaneNode {
    /// Construct a pane node (mirrors Swift
    /// `ExternalPaneNode(id:frame:tabs:selectedTabId:)`).
    pub fn new(
        id: impl Into<String>,
        frame: PixelRect,
        tabs: Vec<Uuid>,
        selected_tab_id: Option<Uuid>,
    ) -> Self {
        Self {
            id: id.into(),
            frame,
            tabs,
            selected_tab_id,
        }
    }
}

/// An internal split — Bonsplit's `ExternalSplitNode`.
///
/// `orientation` is a free-form Swift `String` (`"horizontal"` / `"vertical"`),
/// kept as a `String` here for verbatim parity (the split-geometry port compares
/// it by value, and an unparseable `id` is a meaningful state there). The
/// `first` / `second` children are heap-boxed because the tree is recursive.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSplitNode {
    /// Split id as a raw `String` (Swift stores the id un-parsed; the
    /// split-geometry port is the one that tries `UUID(uuidString:)` and flags
    /// failures — spatial order never inspects it).
    pub id: String,
    /// Split orientation, e.g. `"horizontal"` or `"vertical"`.
    pub orientation: String,
    /// Divider position along the split axis (Swift `Double`).
    pub divider_position: f64,
    /// First / top / left child.
    pub first: Box<ExternalTreeNode>,
    /// Second / bottom / right child.
    pub second: Box<ExternalTreeNode>,
}

impl ExternalSplitNode {
    /// Construct a split node, boxing the children (mirrors Swift
    /// `ExternalSplitNode(id:orientation:dividerPosition:first:second:)`).
    pub fn new(
        id: impl Into<String>,
        orientation: impl Into<String>,
        divider_position: f64,
        first: ExternalTreeNode,
        second: ExternalTreeNode,
    ) -> Self {
        Self {
            id: id.into(),
            orientation: orientation.into(),
            divider_position,
            first: Box::new(first),
            second: Box::new(second),
        }
    }
}

/// A node in the split tree — Bonsplit's `ExternalTreeNode` enum
/// (`.pane` / `.split`).
#[derive(Debug, Clone, PartialEq)]
pub enum ExternalTreeNode {
    /// A leaf pane.
    Pane(ExternalPaneNode),
    /// An internal split with two children.
    Split(ExternalSplitNode),
}
