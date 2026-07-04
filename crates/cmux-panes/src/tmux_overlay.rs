//! Pure geometry for the tmux-style pane overlay.
//!
//! A platform-neutral Rust port of the macOS `CmuxWorkspaces` pane-overlay
//! geometry, whose entire surface is `CGRect`/`CGSize` arithmetic with no
//! AppKit/SwiftUI/I-O dependency. The Swift originals live at
//! `Packages/macOS/CmuxWorkspaces/Sources/CmuxWorkspaces/Window/PaneOverlay/`:
//!
//! * `TmuxPaneOverlayGeometry.swift:1-154` — [`TmuxPaneOverlayGeometry`], the
//!   Bonsplit-snapshot overlay placement math (chrome trim + container offset +
//!   renderable-snapshot selection).
//! * `TmuxPaneLayoutPane.swift:1-58` — [`TmuxPaneLayoutPane`], a single
//!   character-cell pane the experimental tmux-active-pane overlay positions.
//! * `TmuxPaneLayoutReport.swift:1-16` — [`TmuxPaneLayoutReport`], the reported
//!   pane set with active-pane selection.
//!
//! ## Model note (FRAMING CORRECTION)
//!
//! Despite living in the `cmux-panes` crate next to [`crate::tree`], this
//! geometry does **not** walk the recursive `ExternalTreeNode`. The Swift code
//! operates over Bonsplit's *flat* [`LayoutSnapshot`] — a container
//! [`PixelRect`] plus a list of `{pane_id: String, frame: PixelRect}` panes —
//! and shares only [`PixelRect`] with the tree module. The value model below is
//! a new sibling introduced from the test fixture
//! (`TmuxPaneOverlayGeometryTests.swift:9-26`); Bonsplit's inert
//! `focusedPaneId`/`timestamp` (snapshot) and `selectedTabId`/`tabIds`
//! (pane geometry) fields carry no in-crate consumer and are omitted, as is the
//! `PixelRect.cgRect` bridge (here [`PixelRect`] *is* the rectangle type).
//!
//! ## Parity risks pinned
//!
//! * **UUID string casing.** Swift matches `pane.paneId == paneId.id.uuidString`
//!   using `Foundation.UUID.uuidString`, which is **UPPERCASE**; Rust's
//!   [`Uuid::to_string`] is lowercase. The overlay tests build the snapshot
//!   `pane_id` and the [`PaneID`] from the *same* UUID, so a single consistent
//!   formatting (lowercase here) preserves the internal match. A real caller
//!   that feeds an externally-sourced UPPERCASE `pane_id` string would *not*
//!   match — see [`TmuxPaneOverlayGeometry::pane_rect`] — mirroring Swift's own
//!   exact-string comparison; flag a case-insensitive compare there if that ever
//!   becomes a real call site.
//! * **`content_rect` clamp.** The top inset is
//!   `min(chrome, max(0, height - 1))`, capping the trim so a pane always keeps
//!   ≥1pt of content (Swift `TmuxPaneOverlayGeometry.swift:30`); this is *not*
//!   `min(chrome, height)`.
//! * **`effective_snapshot` fallback.** When neither snapshot is renderable the
//!   final `cached ?? live` returns the cached one first (Swift
//!   `TmuxPaneOverlayGeometry.swift:131`).
//! * **`has_renderable_geometry` is strict `> 1`**, not `>= 1` (Swift
//!   `TmuxPaneOverlayGeometry.swift:139-144`).
//! * **`TmuxPaneLayoutPane` cell coords are `i64`** and the overlay guards
//!   `width > 0 && height > 0` (Swift `TmuxPaneLayoutPane.swift:44-49`).

use uuid::Uuid;

use crate::tree::PixelRect;

/// A size in points — Bonsplit/CoreGraphics `CGSize`, used for a character
/// cell's dimensions in [`TmuxPaneLayoutPane::overlay_rect`]. Components are
/// `f64` to mirror Swift's `CGFloat`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

impl Size {
    /// Construct a size (mirrors `CGSize(width:height:)`).
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// A Bonsplit pane identifier — a wrapper over a [`Uuid`], matching the Swift
/// `PaneID` value the app hands to the overlay geometry. Its `.id.uuidString`
/// is what the flat snapshot's [`PaneGeometry::pane_id`] string is compared
/// against (see the module-level UUID-casing parity note).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneID {
    /// The underlying UUID.
    pub id: Uuid,
}

impl PaneID {
    /// Wrap an existing UUID (mirrors `PaneID(id:)`).
    pub fn from_uuid(id: Uuid) -> Self {
        Self { id }
    }

    /// A fresh random pane id (mirrors Swift `PaneID()` = a new random UUID).
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { id: Uuid::new_v4() }
    }
}

/// A single pane in a Bonsplit [`LayoutSnapshot`] — the pixel-space `frame`
/// keyed by its `pane_id` string.
///
/// Ported from the test fixture `TmuxPaneOverlayGeometryTests.swift:16-21`
/// (`PaneGeometry(paneId:frame:selectedTabId:tabIds:)`). Bonsplit's
/// `selectedTabId`/`tabIds` are shell-only (no in-crate consumer) and omitted.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneGeometry {
    /// The pane's identifier as a string (compared against
    /// [`PaneID::id`]'s string form).
    pub pane_id: String,
    /// The pane's on-screen bounds.
    pub frame: PixelRect,
}

impl PaneGeometry {
    /// Construct a pane geometry entry.
    pub fn new(pane_id: impl Into<String>, frame: PixelRect) -> Self {
        Self {
            pane_id: pane_id.into(),
            frame,
        }
    }
}

/// A flat Bonsplit layout snapshot: a container [`PixelRect`] plus its panes.
///
/// Ported from the test fixture `TmuxPaneOverlayGeometryTests.swift:13-25`
/// (`LayoutSnapshot(containerFrame:panes:focusedPaneId:timestamp:)`). The Swift
/// `focusedPaneId`/`timestamp` fields are inert for the overlay geometry and
/// omitted.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutSnapshot {
    /// The container's on-screen bounds; overlay rects are offset by its origin.
    pub container_frame: PixelRect,
    /// The panes in the snapshot.
    pub panes: Vec<PaneGeometry>,
}

impl LayoutSnapshot {
    /// Construct a layout snapshot.
    pub fn new(container_frame: PixelRect, panes: Vec<PaneGeometry>) -> Self {
        Self {
            container_frame,
            panes,
        }
    }
}

/// Pure geometry for placing the tmux-style pane overlay over a Bonsplit split
/// layout.
///
/// Port of Swift `TmuxPaneOverlayGeometry` (`TmuxPaneOverlayGeometry.swift:13`).
/// Holds only the titlebar-chrome inset to trim from the top of each pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TmuxPaneOverlayGeometry {
    /// Height of the titlebar chrome trimmed off the top of each pane rect so
    /// the overlay covers only the terminal content, not the tab strip.
    pub top_chrome_height: f64,
}

impl TmuxPaneOverlayGeometry {
    /// Creates a geometry resolver (mirrors `init(topChromeHeight:)`,
    /// `TmuxPaneOverlayGeometry.swift:21`).
    pub fn new(top_chrome_height: f64) -> Self {
        Self { top_chrome_height }
    }

    /// Trims the titlebar chrome inset off the top of `rect`, clamped so the
    /// height never drops below zero and the pane keeps at least 1pt.
    ///
    /// Port of `contentRect(_:)` (`TmuxPaneOverlayGeometry.swift:29-37`):
    /// `topInset = min(topChromeHeight, max(0, height - 1))`.
    pub fn content_rect(&self, rect: PixelRect) -> PixelRect {
        let top_inset = self.top_chrome_height.min((rect.height - 1.0).max(0.0));
        PixelRect::new(
            rect.x,
            rect.y + top_inset,
            rect.width,
            (rect.height - top_inset).max(0.0),
        )
    }

    /// Resolves the trimmed content rectangle for a single pane in a snapshot.
    ///
    /// Port of the private `paneRect(layoutSnapshot:paneId:includeContainerOffset:)`
    /// (`TmuxPaneOverlayGeometry.swift:49-76`). Returns `None` when the snapshot
    /// or pane is missing. When `include_container_offset` is `true` only the
    /// container y-offset is removed (window-content space); otherwise both axes
    /// are offset by the container origin (workspace-local space).
    ///
    /// The pane lookup compares [`PaneGeometry::pane_id`] against
    /// `pane_id.id.to_string()` — exact string match, mirroring Swift's
    /// `$0.paneId == paneId.id.uuidString` (see module UUID-casing note).
    fn pane_rect(
        &self,
        layout_snapshot: Option<&LayoutSnapshot>,
        pane_id: Option<&PaneID>,
        include_container_offset: bool,
    ) -> Option<PixelRect> {
        let snapshot = layout_snapshot?;
        let pane_id = pane_id?;
        let target = pane_id.id.to_string();
        let pane_frame = snapshot
            .panes
            .iter()
            .find(|pane| pane.pane_id == target)?
            .frame;

        // `CGRect.offsetBy(dx:dy:)` moves the origin, keeping the size.
        let origin_x = if include_container_offset {
            pane_frame.x
        } else {
            pane_frame.x - snapshot.container_frame.x
        };
        let rect = PixelRect::new(
            origin_x,
            pane_frame.y - snapshot.container_frame.y,
            pane_frame.width,
            pane_frame.height,
        );
        Some(self.content_rect(rect))
    }

    /// A pane's overlay rect in workspace-local coordinates (the container
    /// origin is subtracted on both axes).
    ///
    /// Port of `overlayRect(layoutSnapshot:paneId:)`
    /// (`TmuxPaneOverlayGeometry.swift:84-93`).
    pub fn overlay_rect(
        &self,
        layout_snapshot: Option<&LayoutSnapshot>,
        pane_id: Option<&PaneID>,
    ) -> Option<PixelRect> {
        self.pane_rect(layout_snapshot, pane_id, false)
    }

    /// A pane's overlay rect in window-content coordinates (only the container
    /// y-offset is removed; the x-offset is preserved).
    ///
    /// Port of `windowOverlayRect(layoutSnapshot:paneId:)`
    /// (`TmuxPaneOverlayGeometry.swift:102-111`).
    pub fn window_overlay_rect(
        &self,
        layout_snapshot: Option<&LayoutSnapshot>,
        pane_id: Option<&PaneID>,
    ) -> Option<PixelRect> {
        self.pane_rect(layout_snapshot, pane_id, true)
    }

    /// Picks the snapshot with renderable geometry, preferring the live one.
    ///
    /// Port of `effectiveSnapshot(cachedSnapshot:liveSnapshot:)`
    /// (`TmuxPaneOverlayGeometry.swift:119-132`): live if renderable, else
    /// cached if renderable, else `cached ?? live` (cached wins when both are
    /// non-renderable).
    pub fn effective_snapshot(
        &self,
        cached_snapshot: Option<LayoutSnapshot>,
        live_snapshot: Option<LayoutSnapshot>,
    ) -> Option<LayoutSnapshot> {
        if let Some(live) = &live_snapshot {
            if Self::has_renderable_geometry(live) {
                return live_snapshot;
            }
        }
        if let Some(cached) = &cached_snapshot {
            if Self::has_renderable_geometry(cached) {
                return cached_snapshot;
            }
        }
        cached_snapshot.or(live_snapshot)
    }

    /// Whether a snapshot has a non-degenerate container and at least one
    /// non-degenerate pane (both axes strictly `> 1`).
    ///
    /// Port of the static `hasRenderableGeometry(_:)`
    /// (`TmuxPaneOverlayGeometry.swift:139-145`).
    pub fn has_renderable_geometry(snapshot: &LayoutSnapshot) -> bool {
        snapshot.container_frame.width > 1.0
            && snapshot.container_frame.height > 1.0
            && snapshot
                .panes
                .iter()
                .any(|pane| pane.frame.width > 1.0 && pane.frame.height > 1.0)
    }
}

/// A single pane reported by tmux in character-cell coordinates, used by the
/// experimental tmux-active-pane overlay to position a highlight over the pane
/// tmux considers active.
///
/// Port of Swift `TmuxPaneLayoutPane` (`TmuxPaneLayoutPane.swift:6-18`). Cell
/// coordinates are `i64` (Swift `Int`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPaneLayoutPane {
    /// tmux's identifier for the pane.
    pub pane_id: String,
    /// Left edge of the pane, in character cells from the surface origin.
    pub left: i64,
    /// Top edge of the pane, in character cells from the surface origin.
    pub top: i64,
    /// Pane width in character cells.
    pub width: i64,
    /// Pane height in character cells.
    pub height: i64,
    /// Whether tmux currently considers this pane the active one.
    pub is_active: bool,
}

impl TmuxPaneLayoutPane {
    /// Creates a tmux pane layout entry (mirrors
    /// `init(paneId:left:top:width:height:isActive:)`,
    /// `TmuxPaneLayoutPane.swift:28`).
    pub fn new(
        pane_id: impl Into<String>,
        left: i64,
        top: i64,
        width: i64,
        height: i64,
        is_active: bool,
    ) -> Self {
        Self {
            pane_id: pane_id.into(),
            left,
            top,
            width,
            height,
            is_active,
        }
    }

    /// The overlay rect for this pane within a terminal surface, or `None` when
    /// the cell size or pane dimensions are degenerate.
    ///
    /// Port of `overlayRect(surfaceFrame:cellSize:)`
    /// (`TmuxPaneLayoutPane.swift:43-57`): guards `cell.width > 0`,
    /// `cell.height > 0`, `width > 0`, `height > 0`, then scales the cell coords.
    pub fn overlay_rect(&self, surface_frame: PixelRect, cell_size: Size) -> Option<PixelRect> {
        if cell_size.width > 0.0 && cell_size.height > 0.0 && self.width > 0 && self.height > 0 {
            Some(PixelRect::new(
                surface_frame.x + (self.left as f64 * cell_size.width),
                surface_frame.y + (self.top as f64 * cell_size.height),
                self.width as f64 * cell_size.width,
                self.height as f64 * cell_size.height,
            ))
        } else {
            None
        }
    }
}

/// A tmux pane layout report: the full set of panes tmux reported for a surface.
///
/// Port of Swift `TmuxPaneLayoutReport` (`TmuxPaneLayoutReport.swift:2-16`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPaneLayoutReport {
    /// All panes tmux reported, in tmux's order.
    pub panes: Vec<TmuxPaneLayoutPane>,
}

impl TmuxPaneLayoutReport {
    /// Creates a layout report (mirrors `init(panes:)`,
    /// `TmuxPaneLayoutReport.swift:8`).
    pub fn new(panes: Vec<TmuxPaneLayoutPane>) -> Self {
        Self { panes }
    }

    /// The active pane, or the first pane when none is marked active.
    ///
    /// Port of the `activePane` computed property
    /// (`TmuxPaneLayoutReport.swift:13-15`):
    /// `panes.first(where: \.isActive) ?? panes.first`.
    pub fn active_pane(&self) -> Option<&TmuxPaneLayoutPane> {
        self.panes
            .iter()
            .find(|pane| pane.is_active)
            .or_else(|| self.panes.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the Swift test's private `snapshot(container:panes:)` helper
    /// (`TmuxPaneOverlayGeometryTests.swift:9-26`): a flat snapshot whose pane
    /// ids are the UUIDs' string form (lowercase here vs. Swift's uppercase
    /// `uuidString` — see the module UUID-casing parity note; internal
    /// consistency with [`PaneID`] is what the lookup relies on).
    fn snapshot(container: PixelRect, panes: &[(Uuid, PixelRect)]) -> LayoutSnapshot {
        LayoutSnapshot::new(
            container,
            panes
                .iter()
                .map(|(id, frame)| PaneGeometry::new(id.to_string(), *frame))
                .collect(),
        )
    }

    // --- TmuxPaneOverlayGeometry suite ---------------------------------------

    /// Ported from `TmuxPaneOverlayGeometryTests.contentRectTrims`
    /// (`TmuxPaneOverlayGeometryTests.swift:28-37`).
    #[test]
    fn content_rect_trims() {
        let geometry = TmuxPaneOverlayGeometry::new(28.0);
        let rect = geometry.content_rect(PixelRect::new(10.0, 20.0, 100.0, 200.0));
        assert_eq!(rect, PixelRect::new(10.0, 48.0, 100.0, 172.0));

        // A pane shorter than the chrome height keeps at least 1pt of content.
        let tiny = geometry.content_rect(PixelRect::new(0.0, 0.0, 50.0, 10.0));
        assert_eq!(tiny, PixelRect::new(0.0, 9.0, 50.0, 1.0));
    }

    /// Ported from `TmuxPaneOverlayGeometryTests.overlayRectWorkspaceLocal`
    /// (`TmuxPaneOverlayGeometryTests.swift:39-50`).
    #[test]
    fn overlay_rect_workspace_local() {
        let pane_id = Uuid::new_v4();
        let snap = snapshot(
            PixelRect::new(5.0, 7.0, 300.0, 400.0),
            &[(pane_id, PixelRect::new(50.0, 100.0, 120.0, 240.0))],
        );
        let geometry = TmuxPaneOverlayGeometry::new(28.0);
        let rect = geometry.overlay_rect(Some(&snap), Some(&PaneID::from_uuid(pane_id)));
        // offset: x -5, y -7; then top inset 28.
        assert_eq!(
            rect,
            Some(PixelRect::new(45.0, 93.0 + 28.0, 120.0, 240.0 - 28.0))
        );
    }

    /// Ported from `TmuxPaneOverlayGeometryTests.windowOverlayRectKeepsX`
    /// (`TmuxPaneOverlayGeometryTests.swift:52-63`).
    #[test]
    fn window_overlay_rect_keeps_x() {
        let pane_id = Uuid::new_v4();
        let snap = snapshot(
            PixelRect::new(5.0, 7.0, 300.0, 400.0),
            &[(pane_id, PixelRect::new(50.0, 100.0, 120.0, 240.0))],
        );
        let geometry = TmuxPaneOverlayGeometry::new(28.0);
        let rect = geometry.window_overlay_rect(Some(&snap), Some(&PaneID::from_uuid(pane_id)));
        // x is NOT offset; y offset by -7; top inset 28.
        assert_eq!(
            rect,
            Some(PixelRect::new(50.0, 93.0 + 28.0, 120.0, 240.0 - 28.0))
        );
    }

    /// Ported from `TmuxPaneOverlayGeometryTests.missingYieldsNil`
    /// (`TmuxPaneOverlayGeometryTests.swift:65-75`).
    #[test]
    fn missing_yields_nil() {
        let geometry = TmuxPaneOverlayGeometry::new(28.0);
        assert_eq!(geometry.overlay_rect(None, Some(&PaneID::new())), None);
        let snap = snapshot(PixelRect::new(0.0, 0.0, 100.0, 100.0), &[]);
        assert_eq!(
            geometry.overlay_rect(Some(&snap), Some(&PaneID::new())),
            None
        );
        assert_eq!(geometry.overlay_rect(Some(&snap), None), None);
    }

    /// Ported from `TmuxPaneOverlayGeometryTests.effectiveSnapshotPrefersLive`
    /// (`TmuxPaneOverlayGeometryTests.swift:77-96`).
    #[test]
    fn effective_snapshot_prefers_live() {
        let geometry = TmuxPaneOverlayGeometry::new(0.0);
        let renderable = snapshot(
            PixelRect::new(0.0, 0.0, 100.0, 100.0),
            &[(Uuid::new_v4(), PixelRect::new(0.0, 0.0, 50.0, 50.0))],
        );
        let degenerate = snapshot(PixelRect::new(0.0, 0.0, 0.0, 0.0), &[]);

        // Live renderable wins.
        assert_eq!(
            geometry.effective_snapshot(Some(degenerate.clone()), Some(renderable.clone())),
            Some(renderable.clone())
        );
        // Live degenerate falls back to renderable cache.
        assert_eq!(
            geometry.effective_snapshot(Some(renderable.clone()), Some(degenerate.clone())),
            Some(renderable.clone())
        );
        // Both degenerate: returns cached (non-nil) per the fallback order.
        assert_eq!(
            geometry.effective_snapshot(Some(degenerate.clone()), Some(degenerate.clone())),
            Some(degenerate.clone())
        );
        // Nil cache with degenerate live returns the degenerate live.
        assert_eq!(
            geometry.effective_snapshot(None, Some(degenerate.clone())),
            Some(degenerate.clone())
        );
    }

    /// Ported from `TmuxPaneOverlayGeometryTests.hasRenderableGeometry`
    /// (`TmuxPaneOverlayGeometryTests.swift:98-117`).
    #[test]
    fn has_renderable_geometry() {
        let renderable = snapshot(
            PixelRect::new(0.0, 0.0, 100.0, 100.0),
            &[(Uuid::new_v4(), PixelRect::new(0.0, 0.0, 50.0, 50.0))],
        );
        assert!(TmuxPaneOverlayGeometry::has_renderable_geometry(&renderable));

        let tiny_container = snapshot(
            PixelRect::new(0.0, 0.0, 1.0, 1.0),
            &[(Uuid::new_v4(), PixelRect::new(0.0, 0.0, 50.0, 50.0))],
        );
        assert!(!TmuxPaneOverlayGeometry::has_renderable_geometry(
            &tiny_container
        ));

        let tiny_panes = snapshot(
            PixelRect::new(0.0, 0.0, 100.0, 100.0),
            &[(Uuid::new_v4(), PixelRect::new(0.0, 0.0, 1.0, 1.0))],
        );
        assert!(!TmuxPaneOverlayGeometry::has_renderable_geometry(
            &tiny_panes
        ));
    }

    // --- Parity-risk edge cases (flagged in the port notes) ------------------

    /// `effective_snapshot`'s final `cached ?? live` fallback: a non-renderable
    /// cache with a `None` live returns the cache (Swift line 131), and
    /// `None`/`None` returns `None`.
    #[test]
    fn effective_snapshot_final_fallback() {
        let geometry = TmuxPaneOverlayGeometry::new(0.0);
        let degenerate = snapshot(PixelRect::new(0.0, 0.0, 0.0, 0.0), &[]);
        assert_eq!(
            geometry.effective_snapshot(Some(degenerate.clone()), None),
            Some(degenerate)
        );
        assert_eq!(geometry.effective_snapshot(None, None), None);
    }

    /// `content_rect` clamp is `min(chrome, max(0, height - 1))`, not
    /// `min(chrome, height)`: a pane exactly 1pt tall keeps its full 1pt (top
    /// inset clamps to 0).
    #[test]
    fn content_rect_one_point_pane_keeps_full_height() {
        let geometry = TmuxPaneOverlayGeometry::new(28.0);
        let rect = geometry.content_rect(PixelRect::new(4.0, 8.0, 50.0, 1.0));
        assert_eq!(rect, PixelRect::new(4.0, 8.0, 50.0, 1.0));
    }

    /// A pane id that does not match any snapshot pane yields `None` (exact
    /// string comparison; the lookup mirrors Swift's `==` on the id string).
    #[test]
    fn overlay_rect_unknown_pane_id_yields_nil() {
        let geometry = TmuxPaneOverlayGeometry::new(28.0);
        let snap = snapshot(
            PixelRect::new(0.0, 0.0, 100.0, 100.0),
            &[(Uuid::new_v4(), PixelRect::new(0.0, 0.0, 50.0, 50.0))],
        );
        assert_eq!(
            geometry.overlay_rect(Some(&snap), Some(&PaneID::new())),
            None
        );
    }

    /// [`PaneID::new`] yields random, distinct ids (Swift `PaneID()`).
    #[test]
    fn pane_id_new_is_random() {
        assert_ne!(PaneID::new(), PaneID::new());
    }

    // --- TmuxPaneLayoutReport suite ------------------------------------------

    /// Ported from `TmuxPaneLayoutReportTests.activePane`
    /// (`TmuxPaneOverlayGeometryTests.swift:122-129`).
    #[test]
    fn active_pane() {
        let a = TmuxPaneLayoutPane::new("a", 0, 0, 10, 10, false);
        let b = TmuxPaneLayoutPane::new("b", 10, 0, 10, 10, true);
        assert_eq!(
            TmuxPaneLayoutReport::new(vec![a.clone(), b.clone()]).active_pane(),
            Some(&b)
        );
        assert_eq!(
            TmuxPaneLayoutReport::new(vec![a.clone()]).active_pane(),
            Some(&a)
        );
        assert_eq!(TmuxPaneLayoutReport::new(vec![]).active_pane(), None);
    }

    /// Ported from `TmuxPaneLayoutReportTests.overlayRect`
    /// (`TmuxPaneOverlayGeometryTests.swift:131-139`).
    #[test]
    fn layout_pane_overlay_rect() {
        let pane = TmuxPaneLayoutPane::new("a", 2, 3, 4, 5, true);
        let rect = pane.overlay_rect(
            PixelRect::new(100.0, 200.0, 999.0, 999.0),
            Size::new(8.0, 16.0),
        );
        assert_eq!(
            rect,
            Some(PixelRect::new(100.0 + 16.0, 200.0 + 48.0, 32.0, 80.0))
        );
    }

    /// Ported from `TmuxPaneLayoutReportTests.overlayRectNil`
    /// (`TmuxPaneOverlayGeometryTests.swift:141-147`).
    #[test]
    fn layout_pane_overlay_rect_nil() {
        let pane = TmuxPaneLayoutPane::new("a", 0, 0, 4, 5, true);
        assert_eq!(
            pane.overlay_rect(PixelRect::new(0.0, 0.0, 0.0, 0.0), Size::new(0.0, 16.0)),
            None
        );
        let zero_pane = TmuxPaneLayoutPane::new("a", 0, 0, 0, 5, true);
        assert_eq!(
            zero_pane.overlay_rect(PixelRect::new(0.0, 0.0, 0.0, 0.0), Size::new(8.0, 16.0)),
            None
        );
    }
}
