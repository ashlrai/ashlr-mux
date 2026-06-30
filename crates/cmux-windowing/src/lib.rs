//! Platform-neutral windowing core for the cmux Windows port (M5).
//!
//! Holds the pure value types and geometry the multi-window shell reuses
//! verbatim from the macOS `CmuxWindowing` package. The Win32 window operations
//! (HWND, focus, chrome) live in the desktop shell and call into this crate;
//! everything here is OS-independent and unit-tested on any platform.

pub mod geometry;

pub use geometry::{
    clamp_frame_within, should_preserve_frame_during_constrain, Rect, DEFAULT_CONTENT_HEIGHT,
    DEFAULT_CONTENT_WIDTH, DEFAULT_MINIMUM_VISIBLE_EXTENT,
};
