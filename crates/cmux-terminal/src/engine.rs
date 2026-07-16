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

use std::collections::HashMap;

use alacritty_terminal::event::{EventListener, VoidListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Processor};
use alacritty_terminal::vte::{Params, Parser as VteParser, Perform};
use serde::Serialize;

use crate::theme::TerminalTheme;

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
    mode_parser: VteParser,
    mode_state: CanonicalModeState,
    size: GridSize,
}

const CANONICAL_RENDER_GRID_MODES: [RenderGridMode; 31] = [
    // ANSI modes.
    RenderGridMode::new(2, true, false),
    RenderGridMode::new(4, true, false),
    RenderGridMode::new(12, true, true),
    RenderGridMode::new(20, true, false),
    // DEC modes. Screen/cursor/geometry/negotiation modes excluded by the
    // canonical producer are intentionally absent.
    RenderGridMode::new(1, false, false),
    RenderGridMode::new(4, false, false),
    RenderGridMode::new(5, false, false),
    RenderGridMode::new(6, false, false),
    RenderGridMode::new(7, false, true),
    RenderGridMode::new(8, false, false),
    RenderGridMode::new(9, false, false),
    RenderGridMode::new(40, false, false),
    RenderGridMode::new(45, false, false),
    RenderGridMode::new(66, false, false),
    RenderGridMode::new(67, false, false),
    RenderGridMode::new(69, false, false),
    RenderGridMode::new(1000, false, false),
    RenderGridMode::new(1002, false, false),
    RenderGridMode::new(1003, false, false),
    RenderGridMode::new(1004, false, false),
    RenderGridMode::new(1005, false, false),
    RenderGridMode::new(1006, false, false),
    RenderGridMode::new(1007, false, true),
    RenderGridMode::new(1015, false, false),
    RenderGridMode::new(1016, false, false),
    RenderGridMode::new(1035, false, true),
    RenderGridMode::new(1036, false, true),
    RenderGridMode::new(1039, false, false),
    RenderGridMode::new(1045, false, false),
    RenderGridMode::new(2004, false, false),
    RenderGridMode::new(2027, false, false),
];

struct CanonicalModeState {
    values: [RenderGridMode; CANONICAL_RENDER_GRID_MODES.len()],
    saved: [bool; CANONICAL_RENDER_GRID_MODES.len()],
}

impl Default for CanonicalModeState {
    fn default() -> Self {
        Self {
            values: CANONICAL_RENDER_GRID_MODES,
            saved: [false; CANONICAL_RENDER_GRID_MODES.len()],
        }
    }
}

impl CanonicalModeState {
    fn index(&self, code: u16, ansi: bool) -> Option<usize> {
        self.values
            .iter()
            .position(|mode| mode.code == code && mode.ansi == ansi)
    }

    fn set(&mut self, code: u16, ansi: bool, on: bool) {
        if let Some(index) = self.index(code, ansi) {
            self.values[index].on = on;
        }
    }

    fn save(&mut self, code: u16, ansi: bool) {
        if let Some(index) = self.index(code, ansi) {
            self.saved[index] = self.values[index].on;
        }
    }

    fn restore(&mut self, code: u16, ansi: bool) {
        if let Some(index) = self.index(code, ansi) {
            self.values[index].on = self.saved[index];
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn is_set(&self, code: u16, ansi: bool) -> bool {
        self.index(code, ansi)
            .is_some_and(|index| self.values[index].on)
    }
}

impl Perform for CanonicalModeState {
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }
        let ansi = match intermediates {
            b"" => true,
            b"?" => false,
            _ => return,
        };
        for param in params {
            let Some(code) = param.first().copied() else {
                continue;
            };
            match action {
                'h' => self.set(code, ansi, true),
                'l' => self.set(code, ansi, false),
                's' => self.save(code, ansi),
                'r' => self.restore(code, ansi),
                _ => {}
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore || !intermediates.is_empty() {
            return;
        }
        match byte {
            b'=' => self.set(66, false, true),
            b'>' => self.set(66, false, false),
            b'c' => self.reset(),
            _ => {}
        }
    }
}

/// A full terminal-state snapshot using canonical cmux's
/// `cmux.render-grid.v1` JSON field names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderGridFrame {
    pub format: String,
    pub surface_id: String,
    pub state_seq: u64,
    pub columns: usize,
    pub rows: usize,
    pub cursor: Option<RenderGridCursor>,
    pub full: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cleared_rows: Vec<usize>,
    pub styles: Vec<RenderGridStyle>,
    pub row_spans: Vec<RenderGridRowSpan>,
    pub active_screen: RenderGridScreen,
    pub modes: Vec<RenderGridMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_foreground: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_cursor_color: Option<String>,
    pub scrollback_rows: usize,
    pub scrollback_spans: Vec<RenderGridRowSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderGridCursor {
    pub row: usize,
    pub column: usize,
    pub visible: bool,
    pub style: RenderGridCursorStyle,
    pub blinking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderGridCursorStyle {
    Block,
    Bar,
    Underline,
    BlockHollow,
}

/// A resolved visual style. IDs are assigned per frame and style zero is the
/// canonical default pen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
pub struct RenderGridStyle {
    pub id: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderGridRowSpan {
    pub row: usize,
    pub column: usize,
    pub style_id: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_width: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderGridScreen {
    Primary,
    Alternate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RenderGridMode {
    pub code: u16,
    pub ansi: bool,
    pub on: bool,
}

impl RenderGridMode {
    const fn new(code: u16, ansi: bool, on: bool) -> Self {
        Self { code, ansi, on }
    }
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
            mode_parser: VteParser::new(),
            mode_state: CanonicalModeState::default(),
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
        self.mode_parser.advance(&mut self.mode_state, bytes);
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

    /// Whether the child enabled DECSET 2004 bracketed-paste mode.
    pub fn bracketed_paste_enabled(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Whether cursor/navigation keys must use SS3 instead of CSI encoding.
    pub fn application_cursor_keys_enabled(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn alternate_screen_enabled(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    pub fn mouse_reporting_enabled(&self) -> bool {
        self.term.mode().intersects(TermMode::MOUSE_MODE)
    }

    pub fn sgr_mouse_enabled(&self) -> bool {
        self.term.mode().contains(TermMode::SGR_MOUSE)
    }

    /// Scroll the displayed viewport without changing the live cursor.
    pub fn scroll_display_lines(&mut self, lines: i32) {
        if lines != 0 {
            self.term.scroll_display(Scroll::Delta(lines));
        }
    }

    /// Export the complete retained primary-screen history and displayed
    /// viewport. Callers that cross a process boundary should prefer
    /// [`Self::render_grid_snapshot_with_scrollback`] with an explicit budget.
    pub fn render_grid_snapshot(&self, surface_id: &str, state_seq: u64) -> RenderGridFrame {
        self.render_grid_snapshot_with_scrollback(surface_id, state_seq, usize::MAX)
    }

    /// Export a canonical full render-grid frame with at most
    /// `max_scrollback_rows` rows above the displayed viewport.
    pub fn render_grid_snapshot_with_scrollback(
        &self,
        surface_id: &str,
        state_seq: u64,
        max_scrollback_rows: usize,
    ) -> RenderGridFrame {
        let grid = self.term.grid();
        let display_offset = grid.display_offset();
        let scrollback_rows = if self.alternate_screen_enabled() {
            0
        } else {
            grid.history_size()
                .saturating_sub(display_offset)
                .min(max_scrollback_rows)
        };

        let cursor_point = grid.cursor.point;
        let displayed_cursor_row = cursor_point.line.0 + display_offset as i32;
        let cursor_in_viewport = (0..self.size.screen_lines as i32).contains(&displayed_cursor_row);
        let cursor_style = self.term.cursor_style();
        let cursor = Some(RenderGridCursor {
            row: if cursor_in_viewport {
                displayed_cursor_row as usize
            } else {
                0
            },
            column: cursor_point.column.0,
            visible: cursor_in_viewport && self.term.mode().contains(TermMode::SHOW_CURSOR),
            style: match cursor_style.shape {
                alacritty_terminal::vte::ansi::CursorShape::Underline => {
                    RenderGridCursorStyle::Underline
                }
                alacritty_terminal::vte::ansi::CursorShape::Beam => RenderGridCursorStyle::Bar,
                alacritty_terminal::vte::ansi::CursorShape::HollowBlock => {
                    RenderGridCursorStyle::BlockHollow
                }
                alacritty_terminal::vte::ansi::CursorShape::Block
                | alacritty_terminal::vte::ansi::CursorShape::Hidden => {
                    RenderGridCursorStyle::Block
                }
            },
            blinking: cursor_style.blinking,
        });

        let renderable = self.term.renderable_content();
        let colors = renderable.colors;
        let theme = TerminalTheme::default();
        let mut default_foreground = dynamic_color(colors, NamedColor::Foreground)
            .unwrap_or_else(|| theme_color_hex(theme.foreground));
        let mut default_background = dynamic_color(colors, NamedColor::Background)
            .unwrap_or_else(|| theme_color_hex(theme.background));
        if self.mode_state.is_set(5, false) {
            std::mem::swap(&mut default_foreground, &mut default_background);
        }
        let default_style = RenderGridStyle {
            foreground: Some(default_foreground),
            background: Some(default_background),
            ..RenderGridStyle::default()
        };
        let mut styles = vec![default_style.clone()];
        let mut style_ids = HashMap::from([(default_style.clone(), 0)]);
        let style_context = RenderGridStyleContext {
            theme: &theme,
            colors,
            default_style: &default_style,
        };
        let row_spans = self.render_grid_spans(
            Line(-(display_offset as i32)),
            self.size.screen_lines,
            &style_context,
            &mut styles,
            &mut style_ids,
        );
        let scrollback_spans = self.render_grid_spans(
            Line(-((display_offset + scrollback_rows) as i32)),
            scrollback_rows,
            &style_context,
            &mut styles,
            &mut style_ids,
        );

        RenderGridFrame {
            format: "cmux.render-grid.v1".into(),
            surface_id: surface_id.into(),
            state_seq,
            columns: self.size.columns,
            rows: self.size.screen_lines,
            cursor,
            full: true,
            cleared_rows: Vec::new(),
            styles,
            row_spans,
            active_screen: if self.alternate_screen_enabled() {
                RenderGridScreen::Alternate
            } else {
                RenderGridScreen::Primary
            },
            modes: self.render_grid_modes(),
            terminal_foreground: dynamic_color(colors, NamedColor::Foreground),
            terminal_background: dynamic_color(colors, NamedColor::Background),
            terminal_cursor_color: dynamic_color(colors, NamedColor::Cursor),
            scrollback_rows,
            scrollback_spans,
        }
    }

    /// Export rendered cells as a bounded VT stream suitable for session
    /// restoration. Dynamic default-color OSC state is deliberately excluded
    /// so the active theme remains authoritative after replay.
    pub fn vt_snapshot_for_persistence(
        &self,
        max_active_rows: usize,
        max_characters: usize,
    ) -> Option<String> {
        if max_active_rows == 0 || max_characters == 0 {
            return None;
        }

        let grid = self.term.grid();
        let history_rows = if self.alternate_screen_enabled() {
            0
        } else {
            grid.history_size()
        };
        let candidate_rows = history_rows.saturating_add(self.size.screen_lines);
        let row_count = candidate_rows.min(max_active_rows);
        let skipped_rows = candidate_rows.saturating_sub(row_count);
        let first_line = Line(-(history_rows as i32) + skipped_rows as i32);
        let cursor_row = history_rows
            .saturating_add(grid.cursor.point.line.0.max(0) as usize)
            .checked_sub(skipped_rows)
            .filter(|row| *row < row_count);

        let mut rows = Vec::with_capacity(row_count);
        let mut last_content_row = None;
        for row_index in 0..row_count {
            let line = Line(first_line.0 + row_index as i32);
            let last_column = (0..self.size.columns)
                .rfind(|column| persistence_cell_has_content(&grid[line][Column(*column)]));
            if last_column.is_some() {
                last_content_row = Some(row_index);
            }
            rows.push(persistence_vt_row(
                grid,
                line,
                last_column.map_or(0, |column| column + 1),
            ));
        }

        let meaningful_rows = last_content_row
            .into_iter()
            .chain(cursor_row)
            .max()
            .map_or(0, |row| row + 1);
        rows.truncate(meaningful_rows);

        // Explicit CHA after LF makes replay independent of linefeed/newline
        // mode in the destination terminal.
        let snapshot = rows.join("\n\x1b[1G");
        if !vt_has_visible_text(&snapshot) {
            return None;
        }
        let tailed = tail_vt_characters(&snapshot, max_characters);
        vt_has_visible_text(tailed).then(|| tailed.to_owned())
    }

    fn render_grid_spans(
        &self,
        first_line: Line,
        row_count: usize,
        style_context: &RenderGridStyleContext<'_>,
        styles: &mut Vec<RenderGridStyle>,
        style_ids: &mut HashMap<RenderGridStyle, usize>,
    ) -> Vec<RenderGridRowSpan> {
        let grid = self.term.grid();
        let mut spans = Vec::new();
        for row in 0..row_count {
            let line = Line(first_line.0 + row as i32);
            let mut pending: Option<RenderGridRowSpan> = None;
            for column in 0..self.size.columns {
                let cell = &grid[line][Column(column)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                let style = render_grid_style(cell, style_context);
                let style_id = render_grid_style_id(style, styles, style_ids);
                let has_grapheme = cell
                    .zerowidth()
                    .is_some_and(|characters| !characters.is_empty());
                let has_text = cell.c != ' ' || has_grapheme;
                if !has_text && style_id == 0 {
                    flush_render_grid_span(&mut pending, &mut spans);
                    continue;
                }

                let cell_width = if cell.flags.contains(Flags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
                let owns_span = has_text && (cell_width != 1 || has_grapheme);
                if owns_span {
                    flush_render_grid_span(&mut pending, &mut spans);
                }
                let can_append = pending.as_ref().is_some_and(|span| {
                    span.style_id == style_id
                        && span.column + span.cell_width.unwrap_or_default() == column
                });
                if !can_append {
                    flush_render_grid_span(&mut pending, &mut spans);
                    pending = Some(RenderGridRowSpan {
                        row,
                        column,
                        style_id,
                        text: String::new(),
                        cell_width: Some(0),
                    });
                }
                let span = pending.as_mut().expect("render span initialized");
                if has_text {
                    span.text.push(cell.c);
                    if let Some(zerowidth) = cell.zerowidth() {
                        span.text.extend(zerowidth);
                    }
                } else {
                    span.text.push(' ');
                }
                *span.cell_width.as_mut().expect("render span width") += cell_width;
                if owns_span {
                    flush_render_grid_span(&mut pending, &mut spans);
                }
            }
            flush_render_grid_span(&mut pending, &mut spans);
        }
        spans
    }

    fn render_grid_modes(&self) -> Vec<RenderGridMode> {
        self.mode_state.values.to_vec()
    }
}

fn flush_render_grid_span(
    pending: &mut Option<RenderGridRowSpan>,
    spans: &mut Vec<RenderGridRowSpan>,
) {
    if let Some(span) = pending.take() {
        spans.push(span);
    }
}

struct RenderGridStyleContext<'a> {
    theme: &'a TerminalTheme,
    colors: &'a Colors,
    default_style: &'a RenderGridStyle,
}

fn render_grid_style(cell: &Cell, context: &RenderGridStyleContext<'_>) -> RenderGridStyle {
    RenderGridStyle {
        id: 0,
        foreground: render_grid_color(
            cell.fg,
            context.theme,
            context.colors,
            true,
            context
                .default_style
                .foreground
                .as_deref()
                .expect("default foreground"),
        ),
        background: render_grid_color(
            cell.bg,
            context.theme,
            context.colors,
            false,
            context
                .default_style
                .background
                .as_deref()
                .expect("default background"),
        ),
        bold: cell.flags.contains(Flags::BOLD),
        faint: cell.flags.contains(Flags::DIM),
        italic: cell.flags.contains(Flags::ITALIC),
        underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
        blink: cell.flags.contains(Flags::BLINK),
        inverse: cell.flags.contains(Flags::INVERSE),
        invisible: cell.flags.contains(Flags::HIDDEN),
        strikethrough: cell.flags.contains(Flags::STRIKEOUT),
        overline: cell.flags.contains(Flags::OVERLINE),
    }
}

fn render_grid_color(
    color: AnsiColor,
    theme: &TerminalTheme,
    colors: &Colors,
    foreground: bool,
    default_color: &str,
) -> Option<String> {
    match color {
        AnsiColor::Spec(rgb) => Some(rgb_hex(rgb)),
        AnsiColor::Indexed(index) => Some(
            colors[index as usize]
                .map(rgb_hex)
                .unwrap_or_else(|| indexed_color(index, theme)),
        ),
        AnsiColor::Named(NamedColor::Foreground) if foreground => Some(default_color.to_owned()),
        AnsiColor::Named(NamedColor::Background) if !foreground => Some(default_color.to_owned()),
        AnsiColor::Named(named) => Some(
            colors[named]
                .map(rgb_hex)
                .unwrap_or_else(|| named_color(named, theme)),
        ),
    }
}

fn rgb_hex(rgb: alacritty_terminal::vte::ansi::Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb.r, rgb.g, rgb.b)
}

fn dynamic_color(colors: &Colors, named: NamedColor) -> Option<String> {
    colors[named].map(rgb_hex)
}

fn theme_color_hex(color: crate::theme::Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn render_grid_style_id(
    style: RenderGridStyle,
    styles: &mut Vec<RenderGridStyle>,
    style_ids: &mut HashMap<RenderGridStyle, usize>,
) -> usize {
    match style_ids.get(&style) {
        Some(id) => *id,
        None => {
            let id = styles.len();
            style_ids.insert(style.clone(), id);
            let mut stored = style.clone();
            stored.id = id;
            styles.push(stored);
            id
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PersistenceUnderline {
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PersistenceCellStyle {
    foreground: AnsiColor,
    background: AnsiColor,
    underline_color: Option<AnsiColor>,
    bold: bool,
    faint: bool,
    italic: bool,
    blink: bool,
    underline: Option<PersistenceUnderline>,
    inverse: bool,
    invisible: bool,
    strikethrough: bool,
    overline: bool,
}

impl PersistenceCellStyle {
    fn from_cell(cell: &Cell) -> Self {
        let underline = if cell.flags.contains(Flags::DOUBLE_UNDERLINE) {
            Some(PersistenceUnderline::Double)
        } else if cell.flags.contains(Flags::UNDERCURL) {
            Some(PersistenceUnderline::Curly)
        } else if cell.flags.contains(Flags::DOTTED_UNDERLINE) {
            Some(PersistenceUnderline::Dotted)
        } else if cell.flags.contains(Flags::DASHED_UNDERLINE) {
            Some(PersistenceUnderline::Dashed)
        } else if cell.flags.contains(Flags::UNDERLINE) {
            Some(PersistenceUnderline::Single)
        } else {
            None
        };
        Self {
            foreground: cell.fg,
            background: cell.bg,
            underline_color: cell.underline_color(),
            bold: cell.flags.contains(Flags::BOLD),
            faint: cell.flags.contains(Flags::DIM),
            italic: cell.flags.contains(Flags::ITALIC),
            blink: cell.flags.contains(Flags::BLINK),
            underline,
            inverse: cell.flags.contains(Flags::INVERSE),
            invisible: cell.flags.contains(Flags::HIDDEN),
            strikethrough: cell.flags.contains(Flags::STRIKEOUT),
            overline: cell.flags.contains(Flags::OVERLINE),
        }
    }

    fn is_default(self) -> bool {
        self.foreground == AnsiColor::Named(NamedColor::Foreground)
            && self.background == AnsiColor::Named(NamedColor::Background)
            && self.underline_color.is_none()
            && !self.bold
            && !self.faint
            && !self.italic
            && !self.blink
            && self.underline.is_none()
            && !self.inverse
            && !self.invisible
            && !self.strikethrough
            && !self.overline
    }
}

fn persistence_cell_has_content(cell: &Cell) -> bool {
    cell.c != ' '
        || cell
            .zerowidth()
            .is_some_and(|characters| !characters.is_empty())
        || cell.hyperlink().is_some()
        || !PersistenceCellStyle::from_cell(cell).is_default()
        || cell.flags.intersects(
            Flags::WIDE_CHAR | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER,
        )
}

fn persistence_vt_row(
    grid: &alacritty_terminal::grid::Grid<Cell>,
    line: Line,
    column_count: usize,
) -> String {
    let mut row = String::new();
    let mut active_style = None;
    let mut active_hyperlink = None;
    for column in 0..column_count {
        let cell = &grid[line][Column(column)];
        let hyperlink = cell.hyperlink();
        if hyperlink != active_hyperlink {
            if active_hyperlink.is_some() {
                row.push_str("\x1b]8;;\x1b\\");
            }
            if let Some(hyperlink) = hyperlink.as_ref() {
                row.push_str("\x1b]8;id=");
                row.push_str(hyperlink.id());
                row.push(';');
                row.push_str(hyperlink.uri());
                row.push_str("\x1b\\");
            }
            active_hyperlink = hyperlink;
        }

        let style = PersistenceCellStyle::from_cell(cell);
        if active_style != Some(style) {
            if style.is_default() {
                if active_style.is_some_and(|previous| !previous.is_default()) {
                    row.push_str("\x1b[0m");
                }
            } else {
                row.push_str(&persistence_style_sgr(style));
            }
            active_style = Some(style);
        }

        if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            row.push(cell.c);
            if let Some(zerowidth) = cell.zerowidth() {
                row.extend(zerowidth);
            }
        }
    }
    if active_hyperlink.is_some() {
        row.push_str("\x1b]8;;\x1b\\");
    }
    if active_style.is_some_and(|style| !style.is_default()) {
        row.push_str("\x1b[0m");
    }
    row
}

fn persistence_style_sgr(style: PersistenceCellStyle) -> String {
    let mut codes = vec!["0".to_owned()];
    for (enabled, code) in [
        (style.bold, "1"),
        (style.faint, "2"),
        (style.italic, "3"),
        (style.blink, "5"),
        (style.inverse, "7"),
        (style.invisible, "8"),
        (style.strikethrough, "9"),
        (style.overline, "53"),
    ] {
        if enabled {
            codes.push(code.to_owned());
        }
    }
    if let Some(underline) = style.underline {
        codes.push(
            match underline {
                PersistenceUnderline::Single => "4",
                PersistenceUnderline::Double => "4:2",
                PersistenceUnderline::Curly => "4:3",
                PersistenceUnderline::Dotted => "4:4",
                PersistenceUnderline::Dashed => "4:5",
            }
            .to_owned(),
        );
    }
    push_persistence_color(&mut codes, style.foreground, 38, 39, true);
    push_persistence_color(&mut codes, style.background, 48, 49, false);
    if let Some(color) = style.underline_color {
        push_persistence_color(&mut codes, color, 58, 59, true);
    }
    format!("\x1b[{}m", codes.join(";"))
}

fn push_persistence_color(
    codes: &mut Vec<String>,
    color: AnsiColor,
    extended_code: u8,
    default_code: u8,
    foreground: bool,
) {
    match color {
        AnsiColor::Spec(rgb) => {
            codes.push(format!("{extended_code};2;{};{};{}", rgb.r, rgb.g, rgb.b));
        }
        AnsiColor::Indexed(index) => codes.push(format!("{extended_code};5;{index}")),
        AnsiColor::Named(named) => match named_color_index(named) {
            Some(index) if extended_code == 38 && foreground && index < 8 => {
                codes.push((30 + index).to_string());
            }
            Some(index) if extended_code == 38 && foreground => {
                codes.push((90 + index - 8).to_string());
            }
            Some(index) if extended_code == 48 && !foreground && index < 8 => {
                codes.push((40 + index).to_string());
            }
            Some(index) if extended_code == 48 && !foreground => {
                codes.push((100 + index - 8).to_string());
            }
            Some(index) => codes.push(format!("{extended_code};5;{index}")),
            None => codes.push(default_code.to_string()),
        },
    }
}

fn named_color_index(color: NamedColor) -> Option<u8> {
    match color {
        NamedColor::Black | NamedColor::DimBlack => Some(0),
        NamedColor::Red | NamedColor::DimRed => Some(1),
        NamedColor::Green | NamedColor::DimGreen => Some(2),
        NamedColor::Yellow | NamedColor::DimYellow => Some(3),
        NamedColor::Blue | NamedColor::DimBlue => Some(4),
        NamedColor::Magenta | NamedColor::DimMagenta => Some(5),
        NamedColor::Cyan | NamedColor::DimCyan => Some(6),
        NamedColor::White | NamedColor::DimWhite => Some(7),
        NamedColor::BrightBlack => Some(8),
        NamedColor::BrightRed => Some(9),
        NamedColor::BrightGreen => Some(10),
        NamedColor::BrightYellow => Some(11),
        NamedColor::BrightBlue => Some(12),
        NamedColor::BrightMagenta => Some(13),
        NamedColor::BrightCyan => Some(14),
        NamedColor::BrightWhite => Some(15),
        NamedColor::Foreground
        | NamedColor::Background
        | NamedColor::Cursor
        | NamedColor::BrightForeground
        | NamedColor::DimForeground => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VtEscapeState {
    Text,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

fn advance_vt_escape_state(state: VtEscapeState, byte: u8) -> VtEscapeState {
    match state {
        VtEscapeState::Text if byte == 0x1b => VtEscapeState::Escape,
        VtEscapeState::Text => VtEscapeState::Text,
        VtEscapeState::Escape if byte == b'[' => VtEscapeState::Csi,
        VtEscapeState::Escape if byte == b']' => VtEscapeState::Osc,
        VtEscapeState::Escape => VtEscapeState::Text,
        VtEscapeState::Csi if (0x40..=0x7e).contains(&byte) => VtEscapeState::Text,
        VtEscapeState::Csi => VtEscapeState::Csi,
        VtEscapeState::Osc if byte == 0x07 => VtEscapeState::Text,
        VtEscapeState::Osc if byte == 0x1b => VtEscapeState::OscEscape,
        VtEscapeState::Osc => VtEscapeState::Osc,
        VtEscapeState::OscEscape if byte == b'\\' => VtEscapeState::Text,
        VtEscapeState::OscEscape if byte == 0x1b => VtEscapeState::OscEscape,
        VtEscapeState::OscEscape => VtEscapeState::Osc,
    }
}

fn tail_vt_characters(value: &str, max_characters: usize) -> &str {
    let count = value.chars().count();
    if count <= max_characters {
        return value;
    }
    let skipped = count - max_characters;
    let mut start = value
        .char_indices()
        .nth(skipped)
        .map_or(value.len(), |(index, _)| index);
    let mut state = VtEscapeState::Text;
    for byte in value.as_bytes()[..start].iter().copied() {
        state = advance_vt_escape_state(state, byte);
    }
    while state != VtEscapeState::Text && start < value.len() {
        state = advance_vt_escape_state(state, value.as_bytes()[start]);
        start += 1;
    }
    &value[start..]
}

fn vt_has_visible_text(value: &str) -> bool {
    let mut state = VtEscapeState::Text;
    let mut index = 0;
    while index < value.len() {
        let character = value[index..].chars().next().expect("valid UTF-8 suffix");
        let byte = value.as_bytes()[index];
        if state == VtEscapeState::Text
            && byte != 0x1b
            && !character.is_control()
            && !character.is_whitespace()
        {
            return true;
        }
        state = advance_vt_escape_state(state, byte);
        index += character.len_utf8();
    }
    false
}

fn named_color(color: NamedColor, theme: &TerminalTheme) -> String {
    let palette_index = match color {
        NamedColor::Black | NamedColor::DimBlack => Some(0),
        NamedColor::Red | NamedColor::DimRed => Some(1),
        NamedColor::Green | NamedColor::DimGreen => Some(2),
        NamedColor::Yellow | NamedColor::DimYellow => Some(3),
        NamedColor::Blue | NamedColor::DimBlue => Some(4),
        NamedColor::Magenta | NamedColor::DimMagenta => Some(5),
        NamedColor::Cyan | NamedColor::DimCyan => Some(6),
        NamedColor::White | NamedColor::DimWhite => Some(7),
        NamedColor::BrightBlack => Some(8),
        NamedColor::BrightRed => Some(9),
        NamedColor::BrightGreen => Some(10),
        NamedColor::BrightYellow => Some(11),
        NamedColor::BrightBlue => Some(12),
        NamedColor::BrightMagenta => Some(13),
        NamedColor::BrightCyan => Some(14),
        NamedColor::BrightWhite => Some(15),
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => None,
        NamedColor::Background => return theme_color_hex(theme.background),
        NamedColor::Cursor => return theme_color_hex(theme.cursor),
    };
    palette_index
        .map(|index| theme_color_hex(theme.palette_color(index)))
        .unwrap_or_else(|| theme_color_hex(theme.foreground))
}

fn indexed_color(index: u8, theme: &TerminalTheme) -> String {
    match index {
        0..=15 => theme_color_hex(theme.palette_color(index as usize)),
        16..=231 => {
            let index = index - 16;
            let component = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
            let red = component(index / 36);
            let green = component((index % 36) / 6);
            let blue = component(index % 6);
            format!("#{red:02X}{green:02X}{blue:02X}")
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            format!("#{value:02X}{value:02X}{value:02X}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Color;

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
        let defaults = term.render_grid_snapshot("surface", 0).modes;
        assert_eq!(defaults.len(), 31);
        assert!(defaults
            .iter()
            .any(|mode| mode.code == 12 && mode.ansi && mode.on));
        assert!(defaults
            .iter()
            .any(|mode| mode.code == 7 && !mode.ansi && mode.on));

        term.advance(b"\x1b[4h\x1b[?1h\x1b[?5h\x1b[?1002h\x1b[?1006h\x1b[?2004h\x1b=\x1b[?1049h");
        assert!(term.bracketed_paste_enabled());
        assert!(term.application_cursor_keys_enabled());
        assert!(term.alternate_screen_enabled());
        assert!(term.mouse_reporting_enabled());
        assert!(term.sgr_mouse_enabled());
        let changed = term.render_grid_snapshot("surface", 1).modes;
        for (code, ansi) in [(4, true), (1, false), (5, false), (66, false)] {
            assert!(changed
                .iter()
                .any(|mode| mode.code == code && mode.ansi == ansi && mode.on));
        }

        term.advance(
            b"\x1b[4l\x1b[?1l\x1b[?5s\x1b[?5l\x1b[?5r\x1b[?1002l\x1b[?1006l\x1b[?2004l\x1b>\x1b[?1049l",
        );
        assert!(!term.bracketed_paste_enabled());
        assert!(!term.application_cursor_keys_enabled());
        assert!(!term.alternate_screen_enabled());
        assert!(!term.mouse_reporting_enabled());
        assert!(!term.sgr_mouse_enabled());
        assert!(term
            .render_grid_snapshot("surface", 2)
            .modes
            .iter()
            .any(|mode| mode.code == 5 && !mode.ansi && mode.on));

        term.advance(b"\x1bc");
        assert!(term
            .render_grid_snapshot("surface", 3)
            .modes
            .iter()
            .any(|mode| mode.code == 5 && !mode.ansi && !mode.on));
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
            (
                snapshot.styles[0].foreground.as_deref(),
                snapshot.styles[0].background.as_deref()
            ),
            (Some("#E5E5E5"), Some("#000000"))
        );
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
        assert!(json.get("cleared_rows").is_none());
        assert!(json.get("row_spans").is_some());
        assert!(json.get("scrollback_spans").is_some());
        assert_eq!(json["row_spans"][0]["cell_width"], 3);
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
        assert_eq!(
            scrolled
                .cursor
                .as_ref()
                .map(|cursor| (cursor.row, cursor.column, cursor.visible)),
            Some((0, 4, false)),
            "canonical producer retains a hidden cursor object off viewport"
        );
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
        term.advance("A界Be\u{301}".as_bytes());
        term.advance(b"\x1b[2;1H\x1b[1;2;3;4;5;7;8;9;53;38;2;1;2;3;48;5;4mA");

        let snapshot = term.render_grid_snapshot("surface", 1);
        assert_eq!(
            snapshot
                .row_spans
                .iter()
                .filter(|span| span.row == 0)
                .map(|span| (span.column, span.text.as_str(), span.cell_width))
                .collect::<Vec<_>>(),
            vec![
                (0, "A", Some(1)),
                (1, "界", Some(2)),
                (3, "B", Some(1)),
                (4, "e\u{301}", Some(1))
            ]
        );

        let styled = snapshot
            .row_spans
            .iter()
            .find(|span| span.row == 1)
            .expect("styled span");
        let style = &snapshot.styles[styled.style_id];
        assert_eq!(style.foreground.as_deref(), Some("#010203"));
        assert_eq!(style.background.as_deref(), Some("#0000EE"));
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
        assert_eq!(primary.styles[0].foreground.as_deref(), Some("#112233"));
        assert_eq!(primary.styles[0].background.as_deref(), Some("#445566"));
        assert_eq!(
            primary
                .row_spans
                .iter()
                .map(|span| (span.row, span.text.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "primary")],
            "dynamic defaults must not turn blank cells into content"
        );

        term.advance(b"\x1b[?1049h\x1b[Halternate");
        let alternate = term.render_grid_snapshot_with_scrollback("surface", 2, 240);
        assert_eq!(alternate.active_screen, RenderGridScreen::Alternate);
        assert_eq!(alternate.scrollback_rows, 0);
        assert!(alternate.scrollback_spans.is_empty());
        assert_eq!(alternate.row_spans[0].text, "alternate");
    }

    #[test]
    fn render_grid_uses_active_theme_palette_reverse_and_bold_policy() {
        let mut palette = vec![Color::rgb(0x10, 0x10, 0x10); 256];
        palette[1] = Color::rgb(0x11, 0x11, 0x11);
        palette[9] = Color::rgb(0x99, 0x99, 0x99);
        palette[42] = Color::rgb(0x2a, 0x2b, 0x2c);
        let theme = TerminalTheme {
            foreground: Color::rgb(0x01, 0x02, 0x03),
            background: Color::rgb(0x04, 0x05, 0x06),
            palette,
            ..TerminalTheme::default()
        };

        let mut term = TerminalGrid::new(GridSize::new(20, 2));
        term.set_theme(theme.clone())
            .expect("valid 256-color theme");
        term.set_bold_color(Some(RenderGridBoldColor::Bright));
        term.advance(b"D\x1b[1mB\x1b[31mR\x1b[38;5;42mI");

        let color_for = |frame: &RenderGridFrame, text: &str| {
            let span = frame
                .row_spans
                .iter()
                .find(|span| span.text == text)
                .expect("separate styled span");
            frame.styles[span.style_id].foreground.clone()
        };
        let bright = term.render_grid_snapshot("surface", 1);
        assert_eq!(bright.styles[0].foreground.as_deref(), Some("#010203"));
        assert_eq!(bright.styles[0].background.as_deref(), Some("#040506"));
        assert_eq!(color_for(&bright, "D").as_deref(), Some("#010203"));
        assert_eq!(color_for(&bright, "B").as_deref(), Some("#010203"));
        assert_eq!(color_for(&bright, "R").as_deref(), Some("#999999"));
        assert_eq!(color_for(&bright, "I").as_deref(), Some("#2A2B2C"));
        assert_eq!(bright.terminal_foreground, None);
        assert_eq!(bright.terminal_background, None);

        term.set_bold_color(Some(RenderGridBoldColor::Color(Color::rgb(
            0xab, 0xcd, 0xef,
        ))));
        let explicit_bold = term.render_grid_snapshot("surface", 2);
        assert_eq!(color_for(&explicit_bold, "B").as_deref(), Some("#ABCDEF"));
        assert_eq!(color_for(&explicit_bold, "R").as_deref(), Some("#999999"));

        term.advance(b"\x1b[?5h");
        let reversed = term.render_grid_snapshot("surface", 3);
        assert_eq!(reversed.styles[0].foreground.as_deref(), Some("#040506"));
        assert_eq!(reversed.styles[0].background.as_deref(), Some("#010203"));

        let mut invalid = theme;
        invalid.palette.pop();
        assert!(term.set_theme(invalid).is_err());
        assert_eq!(term.theme().palette.len(), 256, "failed update is atomic");
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
            .any(|style| style.bold && style.foreground.as_deref() == Some("#CD0000")));
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
