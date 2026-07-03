//! cmux-browser-history — omnibar frecency suggestion engine.
//!
//! Headless port of `BrowserHistorySuggestionEngine` from the canonical macOS
//! `Packages/macOS/CmuxBrowser` package: a fuzzy + frecency omnibar scorer with
//! fixed-clock recency decay and URL-normalization dedup (www / default-port /
//! trailing-slash). A distinct domain from `cmux-mentions` (which is an
//! @-mention corpus). Pure logic with an injected clock.
//!
//! Scaffold — modules are filled in by the port lane.
