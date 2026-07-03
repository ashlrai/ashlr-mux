//! cmux-canvas — pure free-canvas geometry / layout / snap / placement engine.
//!
//! Headless port of the Foundation-only (zero AppKit) macOS `CmuxCanvas`
//! package (`Packages/macOS/CmuxCanvas/Sources/CmuxCanvas`): value types
//! (points/sizes/rects, direction, resize-edges, guides), the z-ordered
//! `CanvasLayout`, and the four deterministic algorithms — snap engine, placer,
//! aligner, spatial navigator — plus viewport math. `f64` throughout with
//! Swift-parity arithmetic order; `serde` round-trip on `CanvasLayout`. The
//! SwiftUI/AppKit `CmuxCanvasUI` (minimap, focus) is excluded.
//!
//! Module → Swift source map:
//! - [`geometry`] ← `CanvasPoint.swift`, `CanvasSize.swift`, `CanvasRect.swift`,
//!   `CanvasMetrics.swift`, `CanvasDirection.swift`, `CanvasResizeEdges.swift`,
//!   `CanvasGuide.swift`, `CanvasSnapResult.swift`, `CanvasAlignmentCommand.swift`
//! - [`pane`] ← `CanvasPaneID.swift`, `CanvasPanelID.swift`, `CanvasPane.swift`
//! - [`layout`] ← `CanvasLayout.swift`
//! - [`snap`] ← `CanvasSnapEngine.swift`
//! - [`placer`] ← `CanvasPlacer.swift`
//! - [`aligner`] ← `CanvasAligner.swift`
//! - [`spatial_nav`] ← `CanvasSpatialNavigator.swift`
//! - [`viewport`] ← `CanvasViewportMath.swift`

pub mod aligner;
pub mod geometry;
pub mod layout;
pub mod pane;
pub mod placer;
pub mod snap;
pub mod spatial_nav;
pub mod viewport;

pub use aligner::CanvasAligner;
pub use geometry::{
    CanvasAlignmentCommand, CanvasDirection, CanvasGuide, CanvasGuideAxis, CanvasMetrics,
    CanvasPoint, CanvasRect, CanvasResizeEdges, CanvasSize, CanvasSnapResult,
};
pub use layout::CanvasLayout;
pub use pane::{CanvasPane, CanvasPaneID, CanvasPanelID, Uuid};
pub use placer::CanvasPlacer;
pub use snap::CanvasSnapEngine;
pub use spatial_nav::CanvasSpatialNavigator;
pub use viewport::CanvasViewportMath;
