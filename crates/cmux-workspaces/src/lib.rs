//! cmux-workspaces — pure workspace / tab / group runtime logic.
//!
//! Headless port of the pure ordering, group-invariant, batch-reorder, sidebar
//! render-projection, selection-sync, new-workspace-placement, and closed-item
//! history logic from the canonical macOS `Packages/macOS/CmuxWorkspaces` package
//! (+ a few app-side helpers). Operates over `WorkspaceRow` value snapshots — no
//! GUI/GPU/agent/Tauri dependency; Swift's in-place `Tab.groupId` mutation becomes
//! new ordered `Vec` returns here.
//!
//! Scaffold — modules are filled in by the port lane.
