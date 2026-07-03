//! cmux-appearance — appearance / theme resolution layer.
//!
//! Headless port of the resolution logic that sits between the already-ported
//! `cmux-config::Appearance` (3-case config-file enum) and `cmux-terminal`'s
//! terminal theme: `AppearanceMode` normalization (a DISTINCT 4-case enum
//! including `Auto`), the system prefers-dark test, color-scheme resolution, and
//! the `light:X,dark:Y` terminal-theme-name selection codec. Pure string/enum
//! logic; UserDefaults reads are injected as `Option<&str>`.
//!
//! Scaffold — modules are filled in by the port lane.
