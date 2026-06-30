//! VT/grid engine (M2 WS1).
//!
//! [`TerminalGrid`] owns an `alacritty_terminal::Term` plus the `vte` ANSI
//! [`Processor`], turning a raw (UTF-8) PTY byte stream into a cell grid. This
//! is the v1 analogue of cmux's libghostty surface state — the same role, but
//! decoupled from the Metal/Zig stack so it builds on Windows. The renderer
//! (WS2) and compositor (WS3) consume the grid; the ConPTY pump (the
//! [`crate::conpty`] read thread) feeds [`TerminalGrid::advance`].
//!
//! Bytes are decoded as UTF-8 by the VT parser (cross-cutting rule 5) — never
//! as CP-437 — so box-drawing and emoji survive intact.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

/// Terminal grid dimensions in character cells. Implements alacritty's
/// [`Dimensions`] so it can drive `Term` construction and resize directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub columns: usize,
    pub screen_lines: usize,
}

impl GridSize {
    pub fn new(columns: usize, screen_lines: usize) -> Self {
        Self {
            columns,
            screen_lines,
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// The VT state machine + cell grid for one terminal surface.
pub struct TerminalGrid {
    term: Term<VoidListener>,
    parser: Processor,
    size: GridSize,
}

impl TerminalGrid {
    /// Create an empty grid of `size`.
    pub fn new(size: GridSize) -> Self {
        let term = Term::new(Config::default(), &size, VoidListener);
        Self {
            term,
            parser: Processor::new(),
            size,
        }
    }

    /// The current grid dimensions.
    pub fn size(&self) -> GridSize {
        self.size
    }

    /// Feed raw terminal output (UTF-8 PTY bytes) into the VT state machine.
    /// Safe to call with arbitrary chunk boundaries — the parser carries state
    /// across calls, so a multi-byte sequence split across two `advance` calls
    /// still parses correctly.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Resize the grid (reflows the underlying `Term`).
    pub fn resize(&mut self, size: GridSize) {
        self.term.resize(size);
        self.size = size;
    }

    /// The visible viewport as one string per row, top to bottom, with trailing
    /// blank cells trimmed.
    pub fn visible_lines(&self) -> Vec<String> {
        let grid = self.term.grid();
        (0..self.size.screen_lines as i32)
            .map(|line| {
                let mut row = String::with_capacity(self.size.columns);
                for col in 0..self.size.columns {
                    row.push(grid[Line(line)][Column(col)].c);
                }
                row.trim_end().to_owned()
            })
            .collect()
    }

    /// Cursor position as `(line, column)`, zero-based into the viewport.
    pub fn cursor(&self) -> (usize, usize) {
        let point = self.term.grid().cursor.point;
        (point.line.0.max(0) as usize, point.column.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> TerminalGrid {
        TerminalGrid::new(GridSize::new(20, 5))
    }

    #[test]
    fn renders_plain_text_into_the_grid() {
        let mut term = grid();
        term.advance(b"hello");
        assert_eq!(term.visible_lines()[0], "hello");
        assert_eq!(term.cursor(), (0, 5));
    }

    #[test]
    fn newline_and_carriage_return_move_the_cursor() {
        let mut term = grid();
        term.advance(b"ab\r\ncd");
        let lines = term.visible_lines();
        assert_eq!(lines[0], "ab");
        assert_eq!(lines[1], "cd");
        assert_eq!(term.cursor(), (1, 2));
    }

    #[test]
    fn carriage_return_overwrites_from_column_zero() {
        let mut term = grid();
        term.advance(b"abc\rX");
        assert_eq!(term.visible_lines()[0], "Xbc");
    }

    #[test]
    fn utf8_box_drawing_renders_without_codepage_corruption() {
        // Cross-cutting rule 5: bytes are UTF-8, never CP-437. Box-drawing
        // characters must survive as themselves.
        let mut term = grid();
        term.advance("a├─┤b".as_bytes());
        assert_eq!(term.visible_lines()[0], "a├─┤b");
    }

    #[test]
    fn split_multibyte_sequence_across_advances_parses() {
        let mut term = grid();
        let bytes = "├".as_bytes(); // 3 bytes
        term.advance(&bytes[..1]);
        term.advance(&bytes[1..]);
        assert_eq!(term.visible_lines()[0], "├");
    }

    #[test]
    fn csi_cursor_position_places_text() {
        let mut term = grid();
        // CSI 2;3 H -> move to row 2, col 3 (1-based), then write.
        term.advance(b"\x1b[2;3HX");
        assert_eq!(term.visible_lines()[1], "  X");
    }

    #[test]
    fn resize_updates_dimensions() {
        let mut term = grid();
        assert_eq!(term.size(), GridSize::new(20, 5));
        assert_eq!(term.visible_lines().len(), 5);
        term.resize(GridSize::new(40, 10));
        assert_eq!(term.size(), GridSize::new(40, 10));
        assert_eq!(term.visible_lines().len(), 10);
    }
}
