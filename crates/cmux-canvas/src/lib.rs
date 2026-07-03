//! cmux-canvas — pure free-canvas geometry / layout / snap / placement engine.
//!
//! Headless port of the Foundation-only (zero AppKit) macOS `CmuxCanvas` package:
//! value types (points/sizes/rects, direction, resize-edges, guides), the
//! z-ordered `CanvasLayout`, and the four deterministic algorithms — snap engine,
//! placer, aligner, spatial navigator — plus viewport math. f64 throughout with
//! Swift-parity arithmetic order; serde round-trip. The SwiftUI/AppKit CanvasUI
//! (minimap, focus) is excluded.
//!
//! Scaffold — modules are filled in by the port lane.
