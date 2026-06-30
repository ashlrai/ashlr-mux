//! Web↔Rust pane geometry contract (M2 WS3, cross-cutting rule 4).
//!
//! The web chrome owns *geometry intent* and Rust owns *pixels*. The web sends
//! a per-surface [`PaneGeometry`] in device-independent pixels (DIP); Rust
//! converts DIP→physical px via the window scale factor, clamps the rect to the
//! host bounds, applies the optional clip, and orders surfaces by `z_index`.
//! There is exactly one ordering authority and it lives here — the web only
//! declares intent.
//!
//! This is the data-model analogue of cmux's macOS portal placement, where
//! `WindowTerminalHostView` sets `hostView.frame = frameInContainer` and stacks
//! surfaces with `addSubview(positioned:relativeTo:)`, clamping the target rect
//! with `frameInHost.intersection(hostBounds)`
//! (`Sources/TerminalWindowPortal.swift:835`, `:883`–`:909`, `:1064`).
//!
//! The JSON shape is versioned ([`GEOMETRY_CONTRACT_VERSION`]) and golden /
//! parity tested (cross-cutting rule 1). It is consumed by M6 (splits) and M7
//! (browser pane), so its shape is frozen early.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Wire-format version for the pane geometry contract. Bump on any
/// breaking change to the JSON shapes in this module.
pub const GEOMETRY_CONTRACT_VERSION: u32 = 1;

/// A rectangle in device-independent pixels (DIP), as sent by the web chrome.
/// The origin is the top-left of the host content area; +x is right, +y is down.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RectDip {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl RectDip {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Convert DIP→physical px at the given window `scale` factor.
    ///
    /// Each edge is rounded independently (`round(right) - round(left)` for the
    /// width) rather than rounding the origin and size separately. This keeps
    /// adjacent panes that share a DIP edge pixel-aligned with no seam gap or
    /// 1px overlap — the tiling invariant a split terminal layout depends on.
    pub fn to_px(&self, scale: f64) -> RectPx {
        let left = (self.x * scale).round() as i32;
        let top = (self.y * scale).round() as i32;
        let right = ((self.x + self.width) * scale).round() as i32;
        let bottom = ((self.y + self.height) * scale).round() as i32;
        RectPx {
            x: left,
            y: top,
            width: (right - left).max(0),
            height: (bottom - top).max(0),
        }
    }
}

/// A rectangle in physical pixels — the placement/clip space the renderer and
/// the native child surface live in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl RectPx {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[inline]
    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    #[inline]
    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    /// A rect with no area (zero or negative extent) covers nothing.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// Intersect two rects. Returns `None` when they do not overlap (the analogue
    /// of `frameInHost.intersection(hostBounds)` yielding an empty rect).
    pub fn intersection(&self, other: &RectPx) -> Option<RectPx> {
        if self.is_empty() || other.is_empty() {
            return None;
        }
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            return None;
        }
        Some(RectPx {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

/// Per-surface geometry intent sent from the web chrome.
///
/// `rect_dip` is the surface frame; `clip_rect` (when present) further clips the
/// visible region (e.g. a surface partially scrolled under a sibling). `z_index`
/// is the stacking order — higher is closer to the user (drawn on top).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaneGeometry {
    pub surface_id: Uuid,
    pub rect_dip: RectDip,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_rect: Option<RectDip>,
}

const fn default_true() -> bool {
    true
}

/// The resolved native placement for a surface: physical-pixel frame and clip,
/// both already clamped to the host bounds. Produced by [`PaneGeometry::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedPlacement {
    pub surface_id: Uuid,
    pub z_index: i32,
    /// Surface frame in physical px, clamped to the host bounds.
    pub rect_px: RectPx,
    /// Final clip region in physical px (a subset of `rect_px`; equals `rect_px`
    /// when no `clip_rect` was supplied).
    pub clip_px: RectPx,
}

impl PaneGeometry {
    /// Resolve this surface's geometry against the host content bounds (physical
    /// px) at the window `scale`. Returns `None` when the surface is hidden or
    /// lies entirely outside the host (nothing to composite).
    pub fn resolve(&self, host_bounds_px: &RectPx, scale: f64) -> Option<ResolvedPlacement> {
        if !self.visible {
            return None;
        }
        let rect_px = self.rect_dip.to_px(scale);
        let placed = rect_px.intersection(host_bounds_px)?;
        let clip_px = match &self.clip_rect {
            Some(clip) => placed.intersection(&clip.to_px(scale))?,
            None => placed,
        };
        Some(ResolvedPlacement {
            surface_id: self.surface_id,
            z_index: self.z_index,
            rect_px: placed,
            clip_px,
        })
    }
}

/// Resolve and stack a set of surfaces. The result is ordered bottom→top by
/// `z_index`; surfaces with equal `z_index` keep their input order (stable),
/// and hidden / off-host surfaces are dropped. This is the single ordering
/// authority referenced by WS3 and reused by M6/M7.
pub fn resolve_stack(
    geometries: &[PaneGeometry],
    host_bounds_px: &RectPx,
    scale: f64,
) -> Vec<ResolvedPlacement> {
    let mut placements: Vec<ResolvedPlacement> = geometries
        .iter()
        .filter_map(|g| g.resolve(host_bounds_px, scale))
        .collect();
    placements.sort_by_key(|p| p.z_index);
    placements
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> RectPx {
        RectPx::new(0, 0, 1000, 800)
    }

    #[test]
    fn dip_to_px_identity_at_scale_one() {
        let r = RectDip::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.to_px(1.0), RectPx::new(10, 20, 100, 50));
    }

    #[test]
    fn dip_to_px_scales_at_150_percent() {
        let r = RectDip::new(10.0, 20.0, 100.0, 50.0);
        // left=15, top=30, right=round(165)=165, bottom=round(105)=105
        assert_eq!(r.to_px(1.5), RectPx::new(15, 30, 150, 75));
    }

    #[test]
    fn adjacent_panes_tile_without_seam_at_fractional_scale() {
        // Two panes sharing the DIP edge at x=33.5 must not gap or overlap once
        // rasterized at 1.25x: the right edge of A equals the left edge of B.
        let a = RectDip::new(0.0, 0.0, 33.5, 100.0).to_px(1.25);
        let b = RectDip::new(33.5, 0.0, 33.5, 100.0).to_px(1.25);
        assert_eq!(a.right(), b.x, "shared edge must be pixel-identical");
    }

    #[test]
    fn intersection_overlap() {
        let a = RectPx::new(0, 0, 100, 100);
        let b = RectPx::new(50, 50, 100, 100);
        assert_eq!(a.intersection(&b), Some(RectPx::new(50, 50, 50, 50)));
    }

    #[test]
    fn intersection_disjoint_is_none() {
        let a = RectPx::new(0, 0, 10, 10);
        let b = RectPx::new(100, 100, 10, 10);
        assert_eq!(a.intersection(&b), None);
    }

    #[test]
    fn intersection_touching_edge_is_none() {
        // Edges touching but no overlapping area.
        let a = RectPx::new(0, 0, 10, 10);
        let b = RectPx::new(10, 0, 10, 10);
        assert_eq!(a.intersection(&b), None);
    }

    #[test]
    fn resolve_clamps_to_host() {
        let geo = PaneGeometry {
            surface_id: Uuid::nil(),
            rect_dip: RectDip::new(900.0, 0.0, 400.0, 200.0),
            z_index: 0,
            visible: true,
            clip_rect: None,
        };
        let placed = geo.resolve(&host(), 1.0).unwrap();
        // 900+400=1300 clamped to host width 1000 → width 100.
        assert_eq!(placed.rect_px, RectPx::new(900, 0, 100, 200));
        assert_eq!(placed.clip_px, placed.rect_px);
    }

    #[test]
    fn resolve_hidden_is_none() {
        let geo = PaneGeometry {
            surface_id: Uuid::nil(),
            rect_dip: RectDip::new(0.0, 0.0, 100.0, 100.0),
            z_index: 0,
            visible: false,
            clip_rect: None,
        };
        assert!(geo.resolve(&host(), 1.0).is_none());
    }

    #[test]
    fn resolve_fully_offscreen_is_none() {
        let geo = PaneGeometry {
            surface_id: Uuid::nil(),
            rect_dip: RectDip::new(2000.0, 0.0, 100.0, 100.0),
            z_index: 0,
            visible: true,
            clip_rect: None,
        };
        assert!(geo.resolve(&host(), 1.0).is_none());
    }

    #[test]
    fn resolve_applies_clip_subset() {
        let geo = PaneGeometry {
            surface_id: Uuid::nil(),
            rect_dip: RectDip::new(0.0, 0.0, 200.0, 200.0),
            z_index: 0,
            visible: true,
            clip_rect: Some(RectDip::new(50.0, 50.0, 80.0, 80.0)),
        };
        let placed = geo.resolve(&host(), 1.0).unwrap();
        assert_eq!(placed.rect_px, RectPx::new(0, 0, 200, 200));
        assert_eq!(placed.clip_px, RectPx::new(50, 50, 80, 80));
    }

    #[test]
    fn resolve_stack_orders_by_z_index_stably() {
        let mk = |z: i32| PaneGeometry {
            surface_id: Uuid::nil(),
            rect_dip: RectDip::new(0.0, 0.0, 10.0, 10.0),
            z_index: z,
            visible: true,
            clip_rect: None,
        };
        let stack = resolve_stack(&[mk(5), mk(1), mk(5), mk(0)], &host(), 1.0);
        let zs: Vec<i32> = stack.iter().map(|p| p.z_index).collect();
        assert_eq!(zs, vec![0, 1, 5, 5]);
    }

    #[test]
    fn pane_geometry_json_roundtrip() {
        let json = r#"{
            "surface_id": "00000000-0000-0000-0000-000000000001",
            "rect_dip": { "x": 0.0, "y": 0.0, "width": 640.0, "height": 480.0 },
            "z_index": 3,
            "visible": true,
            "clip_rect": { "x": 0.0, "y": 0.0, "width": 320.0, "height": 480.0 }
        }"#;
        let geo: PaneGeometry = serde_json::from_str(json).unwrap();
        assert_eq!(geo.z_index, 3);
        assert_eq!(geo.rect_dip.width, 640.0);
        assert_eq!(geo.clip_rect.unwrap().width, 320.0);
        let back = serde_json::to_string(&geo).unwrap();
        let again: PaneGeometry = serde_json::from_str(&back).unwrap();
        assert_eq!(geo, again);
    }

    #[test]
    fn pane_geometry_defaults_when_optional_fields_absent() {
        // z_index, visible, and clip_rect are optional on the wire.
        let json = r#"{
            "surface_id": "00000000-0000-0000-0000-000000000002",
            "rect_dip": { "x": 1.0, "y": 2.0, "width": 3.0, "height": 4.0 }
        }"#;
        let geo: PaneGeometry = serde_json::from_str(json).unwrap();
        assert_eq!(geo.z_index, 0);
        assert!(geo.visible);
        assert!(geo.clip_rect.is_none());
    }
}
