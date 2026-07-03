//! cmux-git — pure, filesystem-free git metadata parsing.
//!
//! Headless port of the pure subset of `GitMetadataService` from the canonical
//! macOS `Packages/macOS/CmuxGit` package: GitHub repo-slug detection from git
//! remote URLs, `git remote -v` slug extraction with upstream>origin>rest
//! ordering + dedup, and git-config string parsing (inline-comment stripping,
//! `*`/`**` glob match). The filesystem includeIf/gitdir/resolve paths are
//! excluded as host wiring.
//!
//! Scaffold — modules are filled in by the port lane.
