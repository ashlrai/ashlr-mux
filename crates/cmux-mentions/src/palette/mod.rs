//! Internal support port of the `CmuxCommandPalette` search pieces that
//! `TextBoxMentionCandidateIndex.swift` imports:
//!
//! - [`fuzzy`] — `CommandPaletteFuzzyMatcher.swift` (preparation, scoring,
//!   `tokenCanMatchWithoutSingleEdit` prefilter, match indices).
//! - [`corpus`] — `CommandPaletteSearchCorpusEntry.swift`,
//!   `CommandPaletteSearchCorpusResult.swift`,
//!   `CommandPaletteSearchWordText.swift`.
//! - [`engine`] — `CommandPaletteSearchEngine.swift`.
//!
//! NOT ported from that package: `CommandPaletteNucleoSearchIndex` /
//! `CommandPaletteNucleoSearchLibrary` (bindings to the optional native
//! nucleo dylib — Swift treats it as an accelerator whose absence falls back
//! to the pure engine, and this port always takes that fallback path), the
//! `CommandPaletteSearchOrchestrator` (palette-app orchestration), and all
//! UI/state files. These belong to a future command-palette crate.

pub mod corpus;
pub mod engine;
pub mod fuzzy;
