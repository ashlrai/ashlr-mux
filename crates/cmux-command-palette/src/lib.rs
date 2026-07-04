//! cmux-command-palette — pure command-palette model + search orchestrator.
//!
//! Headless port of the remaining pure pieces of the canonical macOS
//! `Packages/macOS/CmuxCommandPalette` package that are NOT already ported into
//! [`cmux_mentions::palette`] (the fuzzy matcher, search corpus, and search
//! engine live there and are reused verbatim via the `cmux-mentions` path
//! dependency).
//!
//! Module map (one module per canonical Swift file):
//!
//! | Rust module          | Swift source                                    |
//! |----------------------|-------------------------------------------------|
//! | [`request_kind`]     | `Request/CommandPaletteRequestKind.swift`       |
//! | [`overlay_promotion`]| `Policy/CommandPaletteOverlayPromotionPolicy.swift` |
//! | [`list_scope`]       | `Orchestration/CommandPaletteListScope.swift`   |
//! | [`usage`]            | `Orchestration/CommandPaletteUsageEntry.swift`  |
//! | [`resolved_match`]   | `Orchestration/CommandPaletteResolvedSearchMatch.swift` |
//! | [`switcher_indexer`] | `Search/CommandPaletteSwitcherSearchIndexer.swift` + `Search/CommandPaletteSwitcherSearchMetadata.swift` |
//! | [`orchestrator`]     | `Orchestration/CommandPaletteSearchOrchestrator.swift` |
//! | [`command`]          | `Values/CommandPaletteCommand.swift`            |
//! | [`context`]          | `Context/CommandPaletteContextSnapshot.swift` + `Context/CommandPaletteContextKeys.swift` |
//! | [`window_store`]     | `State/CommandPaletteWindowStore.swift` + `Snapshot/CommandPaletteDebugSnapshot.swift` + `Snapshot/CommandPaletteDebugResultRow.swift` |
//!
//! # The nucleo accelerator path is intentionally absent
//!
//! Swift's `CommandPaletteSearchOrchestrator` prefers an optional native nucleo
//! FFI index (`CommandPaletteNucleoSearchIndex`) and falls back to the pure
//! Swift engine when that dylib is unavailable. Exactly as [`cmux_mentions`]
//! does for its candidate index, this port **always takes the documented
//! nucleo-unavailable fallback path**: [`orchestrator`] carries the
//! `search_index` parameter for signature parity but it is an uninhabited
//! placeholder that callers always pass as `None`, so every flow reduces to the
//! Swift-engine path plus the scope branch. See the `// DIVERGENCE:` comments in
//! [`orchestrator`].
//!
//! # Intentionally NOT ported (host / AppKit-bound)
//!
//! The command catalog/registry and all UI glue are excluded: the handler
//! registry, command contributions/action handling, the SwiftUI overlay and
//! `ContentView` command-palette wiring. [`command`] also omits the
//! `CommandPaletteCommand.action` closure and `CommandPaletteSearchResult`
//! (which embeds that action) — both are host-bound.

pub mod command;
pub mod context;
pub mod list_scope;
pub mod orchestrator;
pub mod overlay_promotion;
pub mod request_kind;
pub mod resolved_match;
pub mod switcher_indexer;
pub mod usage;
pub mod window_store;

pub use command::CommandPaletteCommand;
pub use context::{CommandPaletteContextKeys, CommandPaletteContextSnapshot};
pub use list_scope::CommandPaletteListScope;
pub use orchestrator::{CommandPaletteSearchOrchestrator, NucleoSearchIndex};
pub use overlay_promotion::CommandPaletteOverlayPromotionPolicy;
pub use request_kind::CommandPaletteRequestKind;
pub use resolved_match::CommandPaletteResolvedSearchMatch;
pub use switcher_indexer::{
    CommandPaletteSwitcherSearchIndexer, CommandPaletteSwitcherSearchMetadata, MetadataDetail,
};
pub use usage::CommandPaletteUsageEntry;
pub use window_store::{
    CommandPaletteDebugResultRow, CommandPaletteDebugSnapshot, CommandPaletteWindowStore,
    PrunedPendingOpen, VisibilityUpdate,
};
