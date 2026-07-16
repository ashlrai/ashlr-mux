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

use alacritty_terminal::event::{EventListener, VoidListener};
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
///
/// Generic over the `EventListener` so a byte-fed grid can use the no-op
/// [`VoidListener`] (the default) while a live surface plugs in a listener that
/// forwards terminal query responses back to the PTY.
pub struct TerminalGrid<L: EventListener = VoidListener> {
    term: Term<L>,
    parser: Processor,
    size: GridSize,
}

impl TerminalGrid<VoidListener> {
    /// Create an empty grid of `size` with no event sink.
    pub fn new(size: GridSize) -> Self {
        Self::with_listener(size, VoidListener)
    }
}

impl<L: EventListener> TerminalGrid<L> {
    /// Create an empty grid of `size` whose `Term` reports events (query
    /// responses, bell, title, child exit) to `listener`.
    pub fn with_listener(size: GridSize, listener: L) -> Self {
        let term = Term::new(Config::default(), &size, listener);
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

    /// Discard retained scrollback while preserving the visible viewport.
    pub fn clear_history(&mut self) {
        self.term.grid_mut().clear_history();
    }

    /// The visible viewport as one string per row, top to bottom, with trailing
    /// blank cells trimmed.
    pub fn visible_lines(&self) -> Vec<String> {
        self.rows_from(Line(0))
    }

    /// Plain-text rows from the current viewport or the full retained grid.
    /// `line_limit` tails the result, matching `surface.read_text --lines`.
    pub fn text_lines(&self, include_scrollback: bool, line_limit: Option<usize>) -> Vec<String> {
        let grid = self.term.grid();
        let history_lines = if include_scrollback {
            grid.history_size()
        } else {
            0
        };
        let first_line = -(history_lines as i32);
        let mut lines = self.rows_from(Line(first_line));
        let cursor_line = history_lines.saturating_add(grid.cursor.point.line.0.max(0) as usize);
        let last_content_line = lines.iter().rposition(|line| !line.is_empty()).unwrap_or(0);
        let meaningful_lines = (cursor_line.max(last_content_line) + 1).min(lines.len());
        lines.truncate(meaningful_lines);
        if let Some(limit) = line_limit {
            let keep_from = lines.len().saturating_sub(limit);
            lines.drain(..keep_from);
        }
        lines
    }

    fn rows_from(&self, first_line: Line) -> Vec<String> {
        let grid = self.term.grid();
        (first_line.0..self.size.screen_lines as i32)
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

    #[test]
    fn text_lines_can_include_scrollback_and_apply_a_tail_limit() {
        let mut term = TerminalGrid::new(GridSize::new(20, 2));
        term.advance(b"one\r\ntwo\r\nthree");

        assert_eq!(term.text_lines(false, None), vec!["two", "three"]);
        assert_eq!(term.text_lines(true, None), vec!["one", "two", "three"]);
        assert_eq!(term.text_lines(true, Some(2)), vec!["two", "three"]);
    }

    #[test]
    fn text_lines_omit_unused_rows_but_preserve_an_empty_cursor_line() {
        let mut term = TerminalGrid::new(GridSize::new(20, 4));
        term.advance(b"one\r\n");

        assert_eq!(term.visible_lines(), vec!["one", "", "", ""]);
        assert_eq!(term.text_lines(false, None), vec!["one", ""]);
        assert_eq!(term.text_lines(false, Some(1)), vec![""]);
    }

    #[test]
    fn clear_history_discards_scrollback_without_erasing_the_viewport() {
        let mut term = TerminalGrid::new(GridSize::new(20, 2));
        term.advance(b"one\r\ntwo\r\nthree");
        assert_eq!(term.text_lines(true, None), vec!["one", "two", "three"]);

        term.clear_history();

        assert_eq!(term.text_lines(true, None), vec!["two", "three"]);
        assert_eq!(term.text_lines(false, None), vec!["two", "three"]);
    }

    #[test]
    fn terminal_modes_track_the_vt_state_machine() {
        let mut term = grid();
        assert!(!term.bracketed_paste_enabled());
        assert!(!term.application_cursor_keys_enabled());
        assert!(!term.alternate_screen_enabled());
        assert!(!term.mouse_reporting_enabled());
        assert!(!term.sgr_mouse_enabled());

        term.advance(b"\x1b[?1h\x1b[?1002h\x1b[?1006h\x1b[?2004h\x1b[?1049h");
        assert!(term.bracketed_paste_enabled());
        assert!(term.application_cursor_keys_enabled());
        assert!(term.alternate_screen_enabled());
        assert!(term.mouse_reporting_enabled());
        assert!(term.sgr_mouse_enabled());

        term.advance(b"\x1b[?1l\x1b[?1002l\x1b[?1006l\x1b[?2004l\x1b[?1049l");
        assert!(!term.bracketed_paste_enabled());
        assert!(!term.application_cursor_keys_enabled());
        assert!(!term.alternate_screen_enabled());
        assert!(!term.mouse_reporting_enabled());
        assert!(!term.sgr_mouse_enabled());
    }

    #[test]
    fn render_grid_snapshot_matches_the_canonical_wire_contract() {
        let mut term = TerminalGrid::new(GridSize::new(8, 2));
        term.advance(b"one\r\ntwo\r\nthree\x1b[?2004h");

        let snapshot = term.render_grid_snapshot_with_scrollback("surface-1", 19, 240);
        assert_eq!(snapshot.format, "cmux.render-grid.v1");
        assert_eq!(snapshot.surface_id, "surface-1");
        assert_eq!(snapshot.state_seq, 19);
        assert_eq!((snapshot.columns, snapshot.rows), (8, 2));
        assert!(snapshot.full);
        assert!(snapshot.cleared_rows.is_empty());
        assert_eq!(snapshot.active_screen, RenderGridScreen::Primary);
        assert_eq!(
            snapshot
                .row_spans
                .iter()
                .map(|span| (span.row, span.column, span.text.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, 0, "two"), (1, 0, "three")]
        );
        assert_eq!(snapshot.scrollback_rows, 1);
        assert_eq!(snapshot.scrollback_spans[0].text, "one");
        assert!(snapshot
            .modes
            .iter()
            .any(|mode| mode.code == 2004 && !mode.ansi && mode.on));
        assert_eq!(
            snapshot
                .cursor
                .as_ref()
                .map(|cursor| (cursor.row, cursor.column)),
            Some((1, 5))
        );

        let json = serde_json::to_value(&snapshot).expect("serializable render-grid frame");
        assert_eq!(json["format"], "cmux.render-grid.v1");
        assert_eq!(json["surface_id"], "surface-1");
        assert_eq!(json["state_seq"], 19);
        assert!(json.get("row_spans").is_some());
        assert!(json.get("scrollback_spans").is_some());
    }

    #[test]
    fn render_grid_snapshot_tracks_scrolled_viewports_and_budgets_history() {
        let mut term = TerminalGrid::new(GridSize::new(8, 2));
        term.advance(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");

        let bounded = term.render_grid_snapshot_with_scrollback("surface", 27, 2);
        assert_eq!(bounded.scrollback_rows, 2);
        assert_eq!(
            bounded
                .scrollback_spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three"]
        );

        term.scroll_display_lines(1);
        let scrolled = term.render_grid_snapshot_with_scrollback("surface", 28, 240);
        assert_eq!(
            scrolled
                .row_spans
                .iter()
                .map(|span| (span.row, span.text.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "three"), (1, "four")]
        );
        assert_eq!(scrolled.cursor, None, "live cursor is below the viewport");
        assert_eq!(
            scrolled
                .scrollback_spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"]
        );
    }

    #[test]
    fn render_grid_preserves_widths_styles_and_resolved_colors() {
        let mut term = TerminalGrid::new(GridSize::new(12, 2));
        term.advance("界e\u{301}".as_bytes());
        term.advance(b"\x1b[2;1H\x1b[1;2;3;4;5;7;8;9;53;38;2;1;2;3;48;5;4mA");

        let snapshot = term.render_grid_snapshot("surface", 1);
        let wide = snapshot
            .row_spans
            .iter()
            .find(|span| span.row == 0)
            .expect("wide and combining text span");
        assert_eq!(wide.text, "界e\u{301}");
        assert_eq!(wide.cell_width, Some(3));

        let styled = snapshot
            .row_spans
            .iter()
            .find(|span| span.row == 1)
            .expect("styled span");
        let style = &snapshot.styles[styled.style_id];
        assert_eq!(style.foreground.as_deref(), Some("#010203"));
        assert_eq!(style.background.as_deref(), Some("#0000ee"));
        assert!(style.bold && style.faint && style.italic && style.underline);
        assert!(style.blink && style.inverse && style.invisible);
        assert!(style.strikethrough && style.overline);
    }

    #[test]
    fn render_grid_preserves_dynamic_colors_and_active_screen() {
        let mut term = TerminalGrid::new(GridSize::new(12, 2));
        term.advance(b"primary\x1b]10;#112233\x07\x1b]11;#445566\x07\x1b]12;#778899\x07");
        let primary = term.render_grid_snapshot("surface", 1);
        assert_eq!(primary.terminal_foreground.as_deref(), Some("#112233"));
        assert_eq!(primary.terminal_background.as_deref(), Some("#445566"));
        assert_eq!(primary.terminal_cursor_color.as_deref(), Some("#778899"));

        term.advance(b"\x1b[?1049h\x1b[Halternate");
        let alternate = term.render_grid_snapshot_with_scrollback("surface", 2, 240);
        assert_eq!(alternate.active_screen, RenderGridScreen::Alternate);
        assert_eq!(alternate.scrollback_rows, 0);
        assert!(alternate.scrollback_spans.is_empty());
        assert_eq!(alternate.row_spans[0].text, "alternate");
    }

    #[test]
    fn persistence_snapshot_is_reconstructible_bounded_and_theme_portable() {
        let mut term = TerminalGrid::new(GridSize::new(12, 3));
        term.advance(b"one\r\n\x1b]10;#112233\x07\x1b[1;31mtwo\x1b[0m\r\nthree\r\nfour");

        let vt = term
            .vt_snapshot_for_persistence(4_000, 400_000)
            .expect("non-empty persisted state");
        assert!(vt.chars().count() <= 400_000);
        assert!(!vt.contains("\x1b]10;"));

        let mut restored = TerminalGrid::new(GridSize::new(12, 3));
        restored.advance(vt.as_bytes());
        assert_eq!(restored.text_lines(true, None), term.text_lines(true, None));
        let frame = restored.render_grid_snapshot_with_scrollback("surface", 1, 4_000);
        assert!(frame
            .styles
            .iter()
            .any(|style| style.bold && style.foreground.as_deref() == Some("#cd0000")));
    }

    #[test]
    fn persistence_snapshot_caps_total_rows_and_can_tail_inside_the_viewport() {
        let mut term = TerminalGrid::new(GridSize::new(16, 5));
        term.advance(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        assert_eq!(
            term.vt_snapshot_for_persistence(2, usize::MAX),
            Some("four\n\x1b[1Gfive".into())
        );

        let mut many = TerminalGrid::new(GridSize::new(16, 1));
        for row in 0..4_005 {
            if row != 0 {
                many.advance(b"\r\n");
            }
            many.advance(format!("row{row:04}").as_bytes());
        }
        let vt = many
            .vt_snapshot_for_persistence(4_000, usize::MAX)
            .expect("recent rows");
        assert_eq!(vt.lines().count(), 4_000);
        assert!(vt.starts_with("row0005"));
        assert!(vt.ends_with("row4004"));
    }

    #[test]
    fn persistence_snapshot_preserves_hyperlinks_and_underline_styles() {
        let mut term = TerminalGrid::new(GridSize::new(40, 2));
        term.advance(
            b"\x1b]8;id=docs;https://example.test/docs\x1b\\link\x1b]8;;\x1b\\ \
              \x1b[1;4:2mdouble\x1b[0m \x1b[4:3mcurly\x1b[0m \
              \x1b[4:4mdotted\x1b[0m \x1b[4:5mdashed\x1b[0m",
        );

        let vt = term
            .vt_snapshot_for_persistence(4_000, 400_000)
            .expect("styled hyperlink row");
        assert!(vt.contains("\x1b]8;id=docs;https://example.test/docs\x1b\\"));
        assert!(vt.contains("\x1b]8;;\x1b\\"));
        for underline in ["4:2", "4:3", "4:4", "4:5"] {
            assert!(vt.contains(underline), "missing SGR {underline}: {vt:?}");
        }
        assert!(vt.contains(";1;4:2;"), "double underline must retain bold");

        let mut restored = TerminalGrid::new(GridSize::new(40, 2));
        restored.advance(vt.as_bytes());
        let hyperlink = restored.term.grid()[Line(0)][Column(0)]
            .hyperlink()
            .expect("hyperlink metadata reconstructed");
        assert_eq!(hyperlink.id(), "docs");
        assert_eq!(hyperlink.uri(), "https://example.test/docs");
        let double = &restored.term.grid()[Line(0)][Column(5)];
        assert!(double.flags.contains(Flags::DOUBLE_UNDERLINE));
        assert!(double.flags.contains(Flags::BOLD));
    }

    #[test]
    fn persistence_snapshot_preserves_blink_overline_and_alternate_screen() {
        let mut term = TerminalGrid::new(GridSize::new(32, 2));
        term.advance(b"primary\r\nold\x1b[?1049h\x1b[H\x1b[5;53mstyled\x1b[25;55m plain");

        let frame = term.render_grid_snapshot("surface", 1);
        assert!(frame
            .styles
            .iter()
            .any(|style| style.blink && style.overline));
        let vt = term
            .vt_snapshot_for_persistence(4_000, 400_000)
            .expect("active alternate screen");
        assert!(vt.contains(";5;") && vt.contains(";53;"), "{vt:?}");
        assert!(vt.contains("styled"));
        assert!(!vt.contains("primary"));
        assert!(!vt.contains("old"));
    }

    #[test]
    fn persistence_snapshot_rejects_blank_uses_lf_and_tails_at_escape_boundaries() {
        let mut blank = TerminalGrid::new(GridSize::new(8, 2));
        blank.advance(b" \t\r\n  ");
        assert_eq!(blank.vt_snapshot_for_persistence(4_000, 400_000), None);

        let mut content = TerminalGrid::new(GridSize::new(32, 1));
        content.advance(b"\x1b[1;31m0123456789abcdef\x1b[0m");
        let vt = content
            .vt_snapshot_for_persistence(4_000, 10)
            .expect("bounded newest text");
        assert!(vt.chars().count() <= 10, "{vt:?}");
        assert!(vt.contains("abcdef"), "{vt:?}");
        assert!(!vt.starts_with("[1;"), "partial CSI: {vt:?}");
        assert!(!vt.contains('\r'));

        let mut restored = TerminalGrid::new(GridSize::new(32, 1));
        restored.advance(vt.as_bytes());
        assert!(restored.text_lines(false, None)[0].ends_with("abcdef"));
    }
}
