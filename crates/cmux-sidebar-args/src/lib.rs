//! cmux-sidebar-args — sidebar metadata argument parser.
//!
//! Headless port of `SidebarMetadataArgumentParser` and its target value types
//! from the canonical macOS `Packages/macOS/CmuxSidebar` package: a stateless
//! shell-like tokenizer, a `--key[=value]` option parser (stop-at-`--` and
//! no-stop variants), metadata-format / tab-target / optional-panel-id parsing,
//! and the ` -- ` metadata-block splitter. 100% pure (UUID + string trimming).
//!
//! Scaffold — modules are filled in by the port lane.
