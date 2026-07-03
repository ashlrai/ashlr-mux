//! Port of `Orchestration/CommandPaletteListScope.swift`.

/// Which list the palette is showing: the `>`-prefixed command list or the
/// workspace/surface switcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandPaletteListScope {
    /// The command list (query prefixed with `>`).
    Commands,
    /// The workspace/surface switcher list.
    Switcher,
}
