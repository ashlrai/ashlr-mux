//! cmux-command-palette — pure command-palette model + search orchestrator.
//!
//! Headless port of the remaining pure pieces of the canonical macOS
//! `Packages/macOS/CmuxCommandPalette` package that are NOT already ported into
//! `cmux-mentions::palette` (the fuzzy matcher, search corpus, and search engine
//! live there and are reused verbatim via a path dependency). Covers request
//! kinds, overlay-promotion policy, list scope, usage entries, resolved match
//! model, the switcher search indexer, and the search orchestrator.
//!
//! Scaffold — modules are filled in by the port lane.
