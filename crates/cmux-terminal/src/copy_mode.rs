//! Vim-style scrollback copy-mode state machine (pure core).
//!
//! Foundation-only port of the macOS `CmuxTerminalCore/CopyMode/*` resolver.
//! It turns one keyboard input sequence (`keyCode`, `charactersIgnoringModifiers`,
//! modifiers) into a semantic [`CopyModeAction`], tracks numeric prefixes and
//! two-key `gg`/`yy` sequences in [`CopyModeInputState`], and provides the
//! selection-move, visual-line-selection, and clipped-vs-backing grid geometry
//! math the terminal host applies on top of those actions.
//!
//! All AppKit/`NSEvent`/Ghostty references are documentation only. Non-pure seams
//! (the physical-key ASCII-layout lookup used for non-ASCII input sources) are
//! injected as a closure so the whole module is headless-testable. The AppKit
//! `NSEvent.ModifierFlags` -> [`CopyModeModifiers`] mapping and the Ghostty
//! `adjust_selection` bridge live in the app target and are intentionally out of
//! scope here.
//!
//! Swift parity sources (mirrored 1:1):
//! - `Packages/macOS/CmuxTerminalCore/Sources/CmuxTerminalCore/CopyMode/TerminalKeyboardCopyModeAction.swift`
//! - `.../TerminalKeyboardCopyModeCount.swift`
//! - `.../TerminalKeyboardCopyModeCursor.swift`
//! - `.../TerminalKeyboardCopyModeGeometry.swift`
//! - `.../TerminalKeyboardCopyModeInputState.swift`
//! - `.../TerminalKeyboardCopyModeKeyResolution.swift`
//! - `.../TerminalKeyboardCopyModeModifiers.swift`
//! - `.../TerminalKeyboardCopyModeResolution.swift`
//! - `.../TerminalKeyboardCopyModeSelectionMove.swift`
//! - `.../TerminalKeyboardCopyModeVisualLineSelection.swift`

use std::ops::{BitOr, RangeInclusive};

/// A cursor or visual-selection movement supported by terminal keyboard copy mode.
///
/// Describes the movement component of [`CopyModeAction::AdjustSelection`]. The
/// terminal host applies the same cases to the visible copy-mode cursor outside
/// visual mode and to Ghostty's selection endpoint while visual mode is active.
///
/// Swift: `TerminalKeyboardCopyModeSelectionMove` (SelectionMove.swift:14-44).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyModeSelectionMove {
    /// Moves one or more cells left.
    Left,
    /// Moves one or more cells right.
    Right,
    /// Moves one or more rows up.
    Up,
    /// Moves one or more rows down.
    Down,
    /// Moves one or more pages up.
    PageUp,
    /// Moves one or more pages down.
    PageDown,
    /// Moves to the top-left cell.
    Home,
    /// Moves to the bottom-right cell.
    End,
    /// Moves to the first cell in the current row.
    BeginningOfLine,
    /// Moves to the last cell in the current row.
    EndOfLine,
}

impl CopyModeSelectionMove {
    /// The Swift `RawValue` string (Ghostty `adjust_selection` argument).
    ///
    /// Preserves the exact raw values from
    /// `TerminalKeyboardCopyModeSelectionMove` (SelectionMove.swift:14-44) so the
    /// (out-of-scope) Ghostty C-API bridge can round-trip them unchanged.
    pub fn as_str(&self) -> &'static str {
        match self {
            CopyModeSelectionMove::Left => "left",
            CopyModeSelectionMove::Right => "right",
            CopyModeSelectionMove::Up => "up",
            CopyModeSelectionMove::Down => "down",
            CopyModeSelectionMove::PageUp => "page_up",
            CopyModeSelectionMove::PageDown => "page_down",
            CopyModeSelectionMove::Home => "home",
            CopyModeSelectionMove::End => "end",
            CopyModeSelectionMove::BeginningOfLine => "beginning_of_line",
            CopyModeSelectionMove::EndOfLine => "end_of_line",
        }
    }
}

/// A terminal copy-mode command resolved from one keyboard input sequence.
///
/// The semantic command layer between raw keyboard events and the terminal host.
/// [`copy_mode_action`] / [`copy_mode_resolve`] produce these; the AppKit
/// integration decides how to apply the resulting scroll, cursor movement,
/// search, copy, or exit action.
///
/// Swift: `TerminalKeyboardCopyModeAction` (Action.swift:24-74).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyModeAction {
    /// Leaves keyboard copy mode.
    Exit,
    /// Starts visual selection at the current copy-mode cursor.
    StartSelection,
    /// Starts visual-line selection at the current copy-mode cursor row.
    StartLineSelection,
    /// Clears the active visual selection while staying in copy mode.
    ClearSelection,
    /// Copies the active visual selection and exits copy mode.
    CopyAndExit,
    /// Copies one or more full viewport lines and exits copy mode.
    CopyLineAndExit,
    /// Scrolls the viewport by a signed number of lines.
    ScrollLines(i32),
    /// Scrolls the viewport by a signed number of pages.
    ScrollPage(i32),
    /// Scrolls the viewport by a signed number of half pages.
    ScrollHalfPage(i32),
    /// Jumps the viewport and cursor to the top-left cell.
    ScrollToTop,
    /// Jumps the viewport and cursor to the bottom-right cell.
    ScrollToBottom,
    /// Jumps by a signed number of shell prompts.
    JumpToPrompt(i32),
    /// Opens terminal search from copy mode.
    StartSearch,
    /// Moves to the next search result.
    SearchNext,
    /// Moves to the previous search result.
    SearchPrevious,
    /// Moves the copy-mode cursor or extends visual selection.
    AdjustSelection(CopyModeSelectionMove),
}

/// The result of resolving a keyboard copy-mode key event.
///
/// Distinguishes an event that should perform a counted [`CopyModeAction`] from
/// one that should only update pending resolver state.
///
/// Swift: `TerminalKeyboardCopyModeResolution` (Resolution.swift:19-29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyModeResolution {
    /// Performs a resolved action with a clamped repeat count.
    Perform(CopyModeAction, i32),
    /// Consumes the key event without performing an immediate action.
    Consume,
}

/// Modifier keys relevant to terminal keyboard copy-mode command resolution.
///
/// A small, platform-neutral option set that lets the resolver avoid depending on
/// AppKit event types. The app target maps `NSEvent.ModifierFlags` into this type
/// before calling [`copy_mode_action`] / [`copy_mode_resolve`] (that mapping is
/// out of scope for this crate).
///
/// Swift: `TerminalKeyboardCopyModeModifiers` (Modifiers.swift:16-45), a
/// `UInt8`-backed `OptionSet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyModeModifiers(pub u8);

impl CopyModeModifiers {
    /// The empty modifier set (Swift `[]`).
    pub const EMPTY: Self = Self(0);
    /// The Command modifier.
    pub const COMMAND: Self = Self(1 << 0);
    /// The Shift modifier.
    pub const SHIFT: Self = Self(1 << 1);
    /// The Control modifier.
    pub const CONTROL: Self = Self(1 << 2);
    /// The numeric-pad modifier, ignored during command matching.
    pub const NUMERIC_PAD: Self = Self(1 << 3);
    /// The function-key modifier, ignored during command matching.
    pub const FUNCTION: Self = Self(1 << 4);
    /// The Caps Lock modifier, ignored during command matching.
    pub const CAPS_LOCK: Self = Self(1 << 5);

    /// Whether every bit in `other` is present (Swift `OptionSet.contains`).
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The set with every bit in `other` removed (Swift `subtracting`).
    pub const fn subtracting(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Whether no bits are set (Swift `OptionSet.isEmpty`).
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for CopyModeModifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Incremental keyboard state for multi-key terminal copy-mode commands.
///
/// Stores the small amount of resolver state that survives between key events:
/// numeric prefixes, pending `yy`, and pending `gg`. Pass one mutable instance
/// into [`copy_mode_resolve`] for the lifetime of a copy-mode session, then call
/// [`CopyModeInputState::reset`] when the host exits copy mode.
///
/// Swift: `TerminalKeyboardCopyModeInputState` (InputState.swift:16-52).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CopyModeInputState {
    /// The numeric prefix collected before a command.
    pub count_prefix: Option<i32>,
    /// Whether `y` has been pressed as a pending line-yank operator.
    pub pending_yank_line: bool,
    /// Whether `g` has been pressed as a pending jump prefix.
    pub pending_g: bool,
}

impl CopyModeInputState {
    /// Creates an input state snapshot (Swift `init(countPrefix:pendingYankLine:pendingG:)`).
    pub fn new(count_prefix: Option<i32>, pending_yank_line: bool, pending_g: bool) -> Self {
        Self {
            count_prefix,
            pending_yank_line,
            pending_g,
        }
    }

    /// Clears all pending multi-key command state (Swift `reset()`).
    pub fn reset(&mut self) {
        self.count_prefix = None;
        self.pending_yank_line = false;
        self.pending_g = false;
    }
}

/// The largest repeat count accepted by terminal keyboard copy mode.
///
/// Swift: `terminalKeyboardCopyModeMaxCount` (Count.swift:11).
pub const COPY_MODE_MAX_COUNT: i32 = 9_999;

/// Clamps a command repeat count into the range accepted by copy mode.
///
/// Values smaller than one become `1`; values larger than
/// [`COPY_MODE_MAX_COUNT`] become that maximum.
///
/// Swift: `terminalKeyboardCopyModeClampCount(_:)` (Count.swift:26-28).
pub fn copy_mode_clamp_count(value: i32) -> i32 {
    // Swift: `min(max(value, 1), terminalKeyboardCopyModeMaxCount)`.
    value.clamp(1, COPY_MODE_MAX_COUNT)
}

/// A viewport-relative cursor used while terminal keyboard copy mode is active.
///
/// The pure state model behind the visible copy-mode overlay. Hosts move it with
/// [`CopyModeCursor::move_cursor`], clamp it with [`CopyModeCursor::clamp`] when
/// the grid changes, and shift it with
/// [`CopyModeCursor::shift_for_viewport_scroll`] when the viewport scrolls.
///
/// Swift: `TerminalKeyboardCopyModeCursor` (Cursor.swift:13-205).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyModeCursor {
    /// The zero-based viewport row occupied by the cursor.
    pub row: i32,
    /// The zero-based viewport column occupied by the cursor.
    pub column: i32,
}

impl CopyModeCursor {
    /// Creates a cursor at a viewport cell, storing the values as-is.
    ///
    /// Swift: `init(row:column:)` (Cursor.swift:38-41).
    pub fn new(row: i32, column: i32) -> Self {
        Self { row, column }
    }

    /// Returns a cursor constrained to the supplied grid dimensions.
    ///
    /// Swift: `clamped(rows:columns:)` (Cursor.swift:57-61).
    pub fn clamped(self, rows: i32, columns: i32) -> Self {
        let mut copy = self;
        copy.clamp(rows, columns);
        copy
    }

    /// Constrains the cursor to the supplied grid dimensions.
    ///
    /// Swift: `clamp(rows:columns:)` (Cursor.swift:77-80).
    pub fn clamp(&mut self, rows: i32, columns: i32) {
        self.row = Self::clamp_value(self.row, rows);
        self.column = Self::clamp_value(self.column, columns);
    }

    /// Moves the cursor within the current grid and reports vertical scroll overflow.
    ///
    /// Horizontal moves stay inside the grid. Vertical moves clamp to the top or
    /// bottom edge and return the signed overflow line count so the host can
    /// scroll the viewport while the cursor remains visible.
    ///
    /// Swift: `move(_:count:rows:columns:)` (Cursor.swift:100-141). Named
    /// `move_cursor` because `move` is a Rust keyword.
    pub fn move_cursor(
        &mut self,
        direction: CopyModeSelectionMove,
        count: i32,
        rows: i32,
        columns: i32,
    ) -> i32 {
        let clamped_rows = rows.max(1);
        let clamped_columns = columns.max(1);
        let clamped_count = copy_mode_clamp_count(count);
        self.clamp(clamped_rows, clamped_columns);

        match direction {
            CopyModeSelectionMove::Left => {
                self.column = (self.column - clamped_count).max(0);
                0
            }
            CopyModeSelectionMove::Right => {
                self.column = (self.column + clamped_count).min(clamped_columns - 1);
                0
            }
            CopyModeSelectionMove::Up => self.move_vertically(-clamped_count, clamped_rows),
            CopyModeSelectionMove::Down => self.move_vertically(clamped_count, clamped_rows),
            CopyModeSelectionMove::PageUp => {
                self.move_vertically(-(clamped_rows * clamped_count), clamped_rows)
            }
            CopyModeSelectionMove::PageDown => {
                self.move_vertically(clamped_rows * clamped_count, clamped_rows)
            }
            CopyModeSelectionMove::Home => {
                self.row = 0;
                self.column = 0;
                0
            }
            CopyModeSelectionMove::End => {
                self.row = clamped_rows - 1;
                self.column = clamped_columns - 1;
                0
            }
            CopyModeSelectionMove::BeginningOfLine => {
                self.column = 0;
                0
            }
            CopyModeSelectionMove::EndOfLine => {
                self.column = clamped_columns - 1;
                0
            }
        }
    }

    /// Moves the cursor after Ghostty has adjusted a visual-selection endpoint.
    ///
    /// Ghostty owns viewport scrolling for `adjust_selection`; this keeps the
    /// visible cursor model in step without asking callers to apply the overflow
    /// returned by [`CopyModeCursor::move_cursor`].
    ///
    /// Swift: `moveAfterTerminalSelectionAdjustment(_:count:rows:columns:)`
    /// (Cursor.swift:159-166).
    pub fn move_after_terminal_selection_adjustment(
        &mut self,
        direction: CopyModeSelectionMove,
        count: i32,
        rows: i32,
        columns: i32,
    ) {
        let _ = self.move_cursor(direction, count, rows, columns);
    }

    /// Shifts the visible cursor row after the viewport scrolls, without moving text.
    ///
    /// Positive line deltas scroll the viewport downward (same text on a smaller
    /// visible row); negative deltas scroll upward.
    ///
    /// Swift: `shiftForViewportScroll(lineDelta:rows:columns:)` (Cursor.swift:183-186).
    pub fn shift_for_viewport_scroll(&mut self, line_delta: i32, rows: i32, columns: i32) {
        self.row -= line_delta;
        self.clamp(rows, columns);
    }

    /// Swift: private `moveVertically(delta:rows:)` (Cursor.swift:188-200).
    fn move_vertically(&mut self, delta: i32, rows: i32) -> i32 {
        let target = self.row + delta;
        if target < 0 {
            self.row = 0;
            return target;
        }
        if target >= rows {
            self.row = rows - 1;
            return target - (rows - 1);
        }
        self.row = target;
        0
    }

    /// Swift: private static `clamp(_:upperBound:)` (Cursor.swift:202-204):
    /// `max(0, min(max(upperBound, 1) - 1, value))`.
    fn clamp_value(value: i32, upper_bound: i32) -> i32 {
        value.clamp(0, upper_bound.max(1) - 1)
    }
}

/// Resolves the row count that is actually visible in the terminal host view.
///
/// Ghostty can report a backing grid taller than the clipped AppKit host view by
/// a few rows. Vim-mode cursor movement should use the visible rows so edge
/// scrolling begins at the visible edge rather than after moving into clipped
/// backing rows.
///
/// Swift: `terminalKeyboardCopyModeVisibleViewportRows(backingRows:viewHeight:cellHeight:)`
/// (Geometry.swift:23-33).
pub fn copy_mode_visible_viewport_rows(
    backing_rows: i32,
    view_height: f64,
    cell_height: f64,
) -> i32 {
    let clamped_backing_rows = backing_rows.max(1);
    let has_metrics = view_height > 0.0 && cell_height > 0.0;
    if !has_metrics {
        return clamped_backing_rows;
    }
    let fitted_rows = ((view_height / cell_height).floor() as i32).max(1);
    clamped_backing_rows.min(fitted_rows)
}

/// Resolves the initial copy-mode cursor row from Ghostty's IME point.
///
/// Ghostty exposes the live cursor as an IME rectangle (top-origin Y) rather than
/// as viewport cell coordinates; this converts it into the row used by
/// [`CopyModeCursor`] when keyboard copy mode starts.
///
/// Swift: `terminalKeyboardCopyModeInitialViewportRow(rows:imePointY:imeCellHeight:topPadding:)`
/// (Geometry.swift:55-66). Swift's `topPadding` defaults to `0`.
pub fn copy_mode_initial_viewport_row(
    rows: i32,
    ime_point_y: f64,
    ime_cell_height: f64,
    top_padding: f64,
) -> i32 {
    let clamped_rows = rows.max(1);
    let has_height = ime_cell_height > 0.0;
    if !has_height {
        return clamped_rows - 1;
    }
    let estimated_row = (((ime_point_y - top_padding) / ime_cell_height) - 1.0).floor() as i32;
    estimated_row.min(clamped_rows - 1).max(0)
}

/// Resolves the initial copy-mode cursor column from Ghostty's IME point.
///
/// Ghostty reports the cursor X at the cell midpoint; this converts it into the
/// zero-based column used by [`CopyModeCursor`].
///
/// Swift: `terminalKeyboardCopyModeInitialViewportColumn(columns:imePointX:imeCellWidth:leftPadding:)`
/// (Geometry.swift:89-100). Swift's `leftPadding` defaults to `0`.
pub fn copy_mode_initial_viewport_column(
    columns: i32,
    ime_point_x: f64,
    ime_cell_width: f64,
    left_padding: f64,
) -> i32 {
    let clamped_columns = columns.max(1);
    let has_width = ime_cell_width > 0.0;
    if !has_width {
        return 0;
    }
    let estimated_column = ((ime_point_x - left_padding) / ime_cell_width).floor() as i32;
    estimated_column.min(clamped_columns - 1).max(0)
}

/// Chooses a nonzero horizontal drag range within a visible cursor cell.
///
/// Used when the host must synthesize a drag inside the current copy-mode cursor
/// cell. The returned range is clamped to the view bounds and keeps
/// `start_x < end_x` so callers can issue a normal left-to-right drag even when
/// the cell is partially clipped.
///
/// Swift: `terminalKeyboardCopyModeCursorSelectionXRange(rectMinX:rectMaxX:boundsWidth:)`
/// (Geometry.swift:122-145).
pub fn copy_mode_cursor_selection_x_range(
    rect_min_x: f64,
    rect_max_x: f64,
    bounds_width: f64,
) -> Option<(f64, f64)> {
    let max_x = bounds_width - 1.0;
    let has_room = max_x > 0.0;
    if !has_room {
        return None;
    }

    let visible_min_x = rect_min_x.max(0.0).min(max_x);
    let visible_max_x = rect_max_x.max(0.0).min(max_x);
    let start_x = (visible_min_x + 0.5).max(0.0).min(max_x);
    let end_x = (visible_max_x - 0.5).max(0.0).min(max_x);
    if end_x > start_x {
        return Some((start_x, end_x));
    }

    let midpoint_x = ((visible_min_x + visible_max_x) / 2.0).max(0.0).min(max_x);
    if midpoint_x < max_x {
        return Some((midpoint_x, (midpoint_x + 1.0).min(max_x)));
    }
    let fallback_end_x = (midpoint_x - 1.0).max(0.0);
    if fallback_end_x < midpoint_x {
        Some((fallback_end_x, midpoint_x))
    } else {
        None
    }
}

// MARK: - Key resolution
// Swift: `TerminalKeyboardCopyModeKeyResolution.swift`.

/// Swift: private `terminalKeyboardCopyModeNormalizedModifiers(_:)` (KeyResolution.swift:1-5).
fn normalized_modifiers(modifiers: CopyModeModifiers) -> CopyModeModifiers {
    modifiers.subtracting(
        CopyModeModifiers::NUMERIC_PAD
            .bitor(CopyModeModifiers::FUNCTION)
            .bitor(CopyModeModifiers::CAPS_LOCK),
    )
}

/// Swift: private `terminalKeyboardCopyModeChars(_:keyCode:asciiCharacterProvider:)`
/// (KeyResolution.swift:7-19).
fn resolve_chars(
    characters_ignoring_modifiers: Option<&str>,
    key_code: u16,
    ascii_character_provider: &dyn Fn(u16) -> Option<String>,
) -> String {
    let raw: String = characters_ignoring_modifiers
        .and_then(|s| s.chars().next())
        .map(|c| c.to_string())
        .unwrap_or_default();
    if raw.is_ascii() {
        return raw;
    }
    if let Some(scalar) = ascii_character_provider(key_code).and_then(|s| s.chars().next()) {
        return scalar.to_string();
    }
    raw
}

/// Swift: private `terminalKeyboardCopyModeIsUppercaseCommand(_:modifiers:normalizedModifiers:)`
/// (KeyResolution.swift:25-41).
fn is_uppercase_command(
    chars: &str,
    modifiers: CopyModeModifiers,
    normalized: CopyModeModifiers,
) -> bool {
    if normalized == CopyModeModifiers::SHIFT {
        return true;
    }
    if modifiers.contains(CopyModeModifiers::CAPS_LOCK) {
        return false;
    }
    match chars.chars().next() {
        Some(c) if c.is_ascii() => (65..=90).contains(&(c as u32)),
        _ => false,
    }
}

/// Returns whether copy mode should bypass an event so app-level shortcuts handle it.
///
/// Copy mode owns ordinary navigation keys but must not swallow app-level
/// shortcuts such as Command-C or Command-Shift-M. Use this before invoking
/// [`copy_mode_resolve`].
///
/// Swift: `terminalKeyboardCopyModeShouldBypassForShortcut(modifiers:)`
/// (KeyResolution.swift:57-62).
pub fn copy_mode_should_bypass_for_shortcut(modifiers: CopyModeModifiers) -> bool {
    normalized_modifiers(modifiers).contains(CopyModeModifiers::COMMAND)
}

/// Resolves a single key event to a terminal copy-mode action, without ASCII fallback.
///
/// Convenience wrapper over [`copy_mode_action_with_ascii_fallback`] mirroring the
/// Swift default `asciiCharacterProvider: { _ in nil }`.
pub fn copy_mode_action(
    key_code: u16,
    characters_ignoring_modifiers: Option<&str>,
    modifiers: CopyModeModifiers,
    has_selection: bool,
) -> Option<CopyModeAction> {
    copy_mode_action_with_ascii_fallback(
        key_code,
        characters_ignoring_modifiers,
        modifiers,
        has_selection,
        |_| None,
    )
}

/// Resolves a single key event to a terminal copy-mode action.
///
/// Stateless resolver handling one key at a time. For count prefixes and two-key
/// commands such as `gg` and `yy`, use [`copy_mode_resolve`].
///
/// Swift: `terminalKeyboardCopyModeAction(keyCode:charactersIgnoringModifiers:modifiers:hasSelection:asciiCharacterProvider:)`
/// (KeyResolution.swift:86-199).
pub fn copy_mode_action_with_ascii_fallback(
    key_code: u16,
    characters_ignoring_modifiers: Option<&str>,
    modifiers: CopyModeModifiers,
    has_selection: bool,
    ascii_character_provider: impl Fn(u16) -> Option<String>,
) -> Option<CopyModeAction> {
    use CopyModeAction::*;
    use CopyModeSelectionMove::*;

    let normalized = normalized_modifiers(modifiers);
    let chars = resolve_chars(characters_ignoring_modifiers, key_code, &ascii_character_provider);
    let lowercased = chars.to_lowercase();
    let is_uppercase = is_uppercase_command(&chars, modifiers, normalized);

    if key_code == 53 {
        return Some(Exit);
    }

    match key_code {
        126 => return Some(AdjustSelection(Up)),
        125 => return Some(AdjustSelection(Down)),
        123 => return Some(AdjustSelection(Left)),
        124 => return Some(AdjustSelection(Right)),
        116 => {
            return Some(if has_selection {
                AdjustSelection(PageUp)
            } else {
                ScrollPage(-1)
            })
        }
        121 => {
            return Some(if has_selection {
                AdjustSelection(PageDown)
            } else {
                ScrollPage(1)
            })
        }
        115 => {
            return Some(if has_selection {
                AdjustSelection(Home)
            } else {
                ScrollToTop
            })
        }
        119 => {
            return Some(if has_selection {
                AdjustSelection(End)
            } else {
                ScrollToBottom
            })
        }
        _ => {}
    }

    if normalized == CopyModeModifiers::CONTROL {
        if lowercased == "u" || chars == "\u{15}" {
            return Some(if has_selection {
                AdjustSelection(PageUp)
            } else {
                ScrollHalfPage(-1)
            });
        }
        if lowercased == "d" || chars == "\u{04}" {
            return Some(if has_selection {
                AdjustSelection(PageDown)
            } else {
                ScrollHalfPage(1)
            });
        }
        if lowercased == "b" || chars == "\u{02}" {
            return Some(if has_selection {
                AdjustSelection(PageUp)
            } else {
                ScrollPage(-1)
            });
        }
        if lowercased == "f" || chars == "\u{06}" {
            return Some(if has_selection {
                AdjustSelection(PageDown)
            } else {
                ScrollPage(1)
            });
        }
        if lowercased == "y" || chars == "\u{19}" {
            return Some(if has_selection {
                AdjustSelection(Up)
            } else {
                ScrollLines(-1)
            });
        }
        if lowercased == "e" || chars == "\u{05}" {
            return Some(if has_selection {
                AdjustSelection(Down)
            } else {
                ScrollLines(1)
            });
        }
        return None;
    }

    if !(normalized.is_empty() || normalized == CopyModeModifiers::SHIFT) {
        return None;
    }

    match lowercased.as_str() {
        "q" => Some(Exit),
        "v" => {
            if is_uppercase {
                Some(StartLineSelection)
            } else if has_selection {
                Some(ClearSelection)
            } else {
                Some(StartSelection)
            }
        }
        "y" => {
            if is_uppercase && !has_selection {
                Some(CopyLineAndExit)
            } else if has_selection {
                Some(CopyAndExit)
            } else {
                None
            }
        }
        "j" => Some(AdjustSelection(Down)),
        "k" => Some(AdjustSelection(Up)),
        "h" => Some(AdjustSelection(Left)),
        "l" => Some(AdjustSelection(Right)),
        "g" => {
            if is_uppercase {
                Some(if has_selection {
                    AdjustSelection(End)
                } else {
                    ScrollToBottom
                })
            } else {
                None
            }
        }
        "0" | "^" => Some(AdjustSelection(BeginningOfLine)),
        "$" | "4" => {
            if chars == "$" || normalized == CopyModeModifiers::SHIFT {
                Some(AdjustSelection(EndOfLine))
            } else {
                None
            }
        }
        "{" | "[" => {
            if chars == "{" || normalized == CopyModeModifiers::SHIFT {
                Some(JumpToPrompt(-1))
            } else {
                None
            }
        }
        "}" | "]" => {
            if chars == "}" || normalized == CopyModeModifiers::SHIFT {
                Some(JumpToPrompt(1))
            } else {
                None
            }
        }
        "/" => Some(StartSearch),
        "n" => Some(if is_uppercase { SearchPrevious } else { SearchNext }),
        _ => None,
    }
}

/// Resolves a key event and any pending prefix state to a copy-mode command,
/// without ASCII fallback.
///
/// Convenience wrapper over [`copy_mode_resolve_with_ascii_fallback`] mirroring
/// the Swift default `asciiCharacterProvider: { _ in nil }`.
pub fn copy_mode_resolve(
    key_code: u16,
    characters_ignoring_modifiers: Option<&str>,
    modifiers: CopyModeModifiers,
    has_selection: bool,
    state: &mut CopyModeInputState,
) -> CopyModeResolution {
    copy_mode_resolve_with_ascii_fallback(
        key_code,
        characters_ignoring_modifiers,
        modifiers,
        has_selection,
        state,
        |_| None,
    )
}

/// Resolves a key event and any pending prefix state to a copy-mode command.
///
/// The stateful resolver used by the terminal host. Consumes numeric prefixes,
/// tracks pending `gg` and `yy` sequences in [`CopyModeInputState`], and returns
/// either a counted action or a consume-only result.
///
/// Swift: `terminalKeyboardCopyModeResolve(keyCode:charactersIgnoringModifiers:modifiers:hasSelection:state:asciiCharacterProvider:)`
/// (KeyResolution.swift:234-334).
pub fn copy_mode_resolve_with_ascii_fallback(
    key_code: u16,
    characters_ignoring_modifiers: Option<&str>,
    modifiers: CopyModeModifiers,
    has_selection: bool,
    state: &mut CopyModeInputState,
    ascii_character_provider: impl Fn(u16) -> Option<String>,
) -> CopyModeResolution {
    use CopyModeAction::*;
    use CopyModeSelectionMove::*;

    let normalized = normalized_modifiers(modifiers);
    let chars = resolve_chars(characters_ignoring_modifiers, key_code, &ascii_character_provider);
    let lowercased = chars.to_lowercase();
    let is_uppercase = is_uppercase_command(&chars, modifiers, normalized);

    if key_code == 53 {
        state.reset();
        return CopyModeResolution::Perform(Exit, 1);
    }

    if state.pending_yank_line {
        if lowercased == "y" && (normalized.is_empty() || normalized == CopyModeModifiers::SHIFT) {
            let count = copy_mode_clamp_count(state.count_prefix.unwrap_or(1));
            state.reset();
            return CopyModeResolution::Perform(CopyLineAndExit, count);
        }
        state.reset();
    }

    if state.pending_g {
        if lowercased == "g" && normalized.is_empty() && !is_uppercase {
            let count = copy_mode_clamp_count(state.count_prefix.unwrap_or(1));
            let action = if has_selection {
                AdjustSelection(Home)
            } else {
                ScrollToTop
            };
            state.reset();
            return CopyModeResolution::Perform(action, count);
        }
        state.reset();
    }

    let digit: Option<i32> = if normalized.is_empty() {
        lowercased
            .chars()
            .next()
            .filter(|c| c.is_ascii())
            .map(|c| c as u32)
            .filter(|v| (48..=57).contains(v))
            .map(|v| (v - 48) as i32)
    } else {
        None
    };
    if let Some(digit) = digit {
        if digit == 0 {
            if let Some(current) = state.count_prefix {
                state.count_prefix = Some(copy_mode_clamp_count(current * 10));
                return CopyModeResolution::Consume;
            }
        } else {
            let current = state.count_prefix.unwrap_or(0);
            state.count_prefix = Some(copy_mode_clamp_count((current * 10) + digit));
            return CopyModeResolution::Consume;
        }
    }

    if !has_selection && lowercased == "y" && is_uppercase {
        let count = copy_mode_clamp_count(state.count_prefix.unwrap_or(1));
        state.reset();
        return CopyModeResolution::Perform(CopyLineAndExit, count);
    }

    if lowercased == "g" && is_uppercase {
        let count = copy_mode_clamp_count(state.count_prefix.unwrap_or(1));
        let action = if has_selection {
            AdjustSelection(End)
        } else {
            ScrollToBottom
        };
        state.reset();
        return CopyModeResolution::Perform(action, count);
    }

    if !has_selection && lowercased == "y" && normalized.is_empty() {
        state.pending_yank_line = true;
        return CopyModeResolution::Consume;
    }

    if lowercased == "g" && normalized.is_empty() {
        state.pending_g = true;
        return CopyModeResolution::Consume;
    }

    match copy_mode_action_with_ascii_fallback(
        key_code,
        characters_ignoring_modifiers,
        modifiers,
        has_selection,
        ascii_character_provider,
    ) {
        Some(action) => {
            let count = copy_mode_clamp_count(state.count_prefix.unwrap_or(1));
            state.reset();
            CopyModeResolution::Perform(action, count)
        }
        None => {
            state.reset();
            CopyModeResolution::Consume
        }
    }
}

/// A linewise visual selection tracked in absolute terminal screen rows.
///
/// The host keeps the canonical selection here and derives viewport-relative
/// cursors only for rendering, so copy ranges stay stable when the viewport
/// scrolls away from the selection endpoint.
///
/// Swift: `TerminalKeyboardCopyModeVisualLineSelection` (VisualLineSelection.swift:22-356).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyModeVisualLineSelection {
    /// The absolute screen row where the linewise selection started.
    pub anchor_screen_row: u64,
    /// The absolute screen row where the linewise selection currently ends.
    pub endpoint_screen_row: u64,
}

impl CopyModeVisualLineSelection {
    /// Creates a linewise visual selection in absolute screen rows.
    ///
    /// Swift: `init(anchorScreenRow:endpointScreenRow:)` (VisualLineSelection.swift:34-37).
    pub fn new(anchor_screen_row: u64, endpoint_screen_row: u64) -> Self {
        Self {
            anchor_screen_row,
            endpoint_screen_row,
        }
    }

    /// The selected absolute screen-row range, independent of selection direction.
    ///
    /// Swift: `selectedRows` (VisualLineSelection.swift:40-42).
    pub fn selected_rows(&self) -> RangeInclusive<u64> {
        self.anchor_screen_row.min(self.endpoint_screen_row)
            ..=self.anchor_screen_row.max(self.endpoint_screen_row)
    }

    /// Replaces the selected rows while preserving the selection direction.
    ///
    /// Swift: `replaceSelectedRows(_:)` (VisualLineSelection.swift:48-56).
    pub fn replace_selected_rows(&mut self, selected_rows: RangeInclusive<u64>) {
        if self.anchor_screen_row <= self.endpoint_screen_row {
            self.anchor_screen_row = *selected_rows.start();
            self.endpoint_screen_row = *selected_rows.end();
        } else {
            self.anchor_screen_row = *selected_rows.end();
            self.endpoint_screen_row = *selected_rows.start();
        }
    }

    /// Moves the endpoint to a known scrollback boundary.
    ///
    /// Returns `true` when the endpoint was moved without needing viewport cursor
    /// projection.
    ///
    /// Swift: `moveEndpointToBoundary(_:totalRows:)` (VisualLineSelection.swift:65-80).
    pub fn move_endpoint_to_boundary(
        &mut self,
        direction: CopyModeSelectionMove,
        total_rows: Option<u64>,
    ) -> bool {
        match direction {
            CopyModeSelectionMove::Home => {
                self.endpoint_screen_row = 0;
                true
            }
            CopyModeSelectionMove::End => match total_rows {
                Some(total) if total > 0 => {
                    self.endpoint_screen_row = total - 1;
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Updates the endpoint from a viewport cursor and scroll offset.
    ///
    /// Swift: `updateEndpoint(from:viewportRows:scrollOffset:totalRows:)`
    /// (VisualLineSelection.swift:89-101).
    pub fn update_endpoint(
        &mut self,
        cursor: &CopyModeCursor,
        viewport_rows: i32,
        scroll_offset: u64,
        total_rows: Option<u64>,
    ) {
        self.endpoint_screen_row =
            Self::screen_row(cursor.row, viewport_rows, scroll_offset, total_rows);
    }

    /// Moves the endpoint and returns the cursor plus any viewport scroll delta needed.
    ///
    /// Endpoint movement starts from `endpoint_screen_row`, not from a clamped
    /// viewport cursor. If the endpoint is already offscreen, vertical movement
    /// changes the absolute endpoint without forcing a viewport jump.
    ///
    /// Swift: `moveEndpoint(_:count:currentColumn:viewportRows:viewportColumns:scrollOffset:totalRows:)`
    /// (VisualLineSelection.swift:118-173). Argument count mirrors the Swift signature.
    #[allow(clippy::too_many_arguments)]
    pub fn move_endpoint(
        &mut self,
        direction: CopyModeSelectionMove,
        count: i32,
        current_column: i32,
        viewport_rows: i32,
        viewport_columns: i32,
        scroll_offset: u64,
        total_rows: Option<u64>,
    ) -> (CopyModeCursor, i32) {
        let clamped_rows = viewport_rows.max(1);
        let clamped_columns = viewport_columns.max(1);
        let clamped_count = copy_mode_clamp_count(count);
        let visible_rows = Self::visible_screen_rows(scroll_offset, clamped_rows);
        let was_endpoint_visible = visible_rows.contains(&self.endpoint_screen_row);
        let mut column = current_column.min(clamped_columns - 1).max(0);

        match direction {
            CopyModeSelectionMove::Left => column = (column - clamped_count).max(0),
            CopyModeSelectionMove::Right => column = (column + clamped_count).min(clamped_columns - 1),
            CopyModeSelectionMove::BeginningOfLine => column = 0,
            CopyModeSelectionMove::EndOfLine => column = clamped_columns - 1,
            CopyModeSelectionMove::Up => {
                self.offset_endpoint_screen_row(-clamped_count, total_rows)
            }
            CopyModeSelectionMove::Down => {
                self.offset_endpoint_screen_row(clamped_count, total_rows)
            }
            CopyModeSelectionMove::PageUp => {
                self.offset_endpoint_screen_row(-(clamped_rows * clamped_count), total_rows)
            }
            CopyModeSelectionMove::PageDown => {
                self.offset_endpoint_screen_row(clamped_rows * clamped_count, total_rows)
            }
            CopyModeSelectionMove::Home | CopyModeSelectionMove::End => {}
        }

        let scroll_delta: i32 = if !was_endpoint_visible {
            0
        } else if self.endpoint_screen_row < *visible_rows.start() {
            -saturating_i32(*visible_rows.start() - self.endpoint_screen_row)
        } else if self.endpoint_screen_row > *visible_rows.end() {
            saturating_i32(self.endpoint_screen_row - *visible_rows.end())
        } else {
            0
        };
        let cursor_scroll_offset = if scroll_delta == 0 {
            scroll_offset
        } else {
            Self::pending_scroll_offset(scroll_offset, scroll_delta, total_rows)
        };
        let cursor = CopyModeCursor {
            row: Self::viewport_row(self.endpoint_screen_row, cursor_scroll_offset, clamped_rows),
            column,
        };
        (cursor, scroll_delta)
    }

    /// Derives a viewport cursor for the endpoint using the supplied column.
    ///
    /// Swift: `endpointCursor(column:viewportRows:viewportColumns:scrollOffset:)`
    /// (VisualLineSelection.swift:183-193).
    pub fn endpoint_cursor(
        &self,
        column: i32,
        viewport_rows: i32,
        viewport_columns: i32,
        scroll_offset: u64,
    ) -> CopyModeCursor {
        CopyModeCursor {
            row: Self::viewport_row(self.endpoint_screen_row, scroll_offset, viewport_rows),
            column: column.min(viewport_columns.max(1) - 1).max(0),
        }
    }

    /// Returns the selected rows that intersect the visible viewport.
    ///
    /// Swift: `visibleIntersection(scrollOffset:viewportRows:)`
    /// (VisualLineSelection.swift:201-207).
    pub fn visible_intersection(
        &self,
        scroll_offset: u64,
        viewport_rows: i32,
    ) -> Option<RangeInclusive<u64>> {
        let visible_rows = Self::visible_screen_rows(scroll_offset, viewport_rows);
        let selected = self.selected_rows();
        let visible_start = (*selected.start()).max(*visible_rows.start());
        let visible_end = (*selected.end()).min(*visible_rows.end());
        if visible_start <= visible_end {
            Some(visible_start..=visible_end)
        } else {
            None
        }
    }

    /// Returns whether the entire selected range is visible in the viewport.
    ///
    /// Swift: `fitsVisibleRows(scrollOffset:viewportRows:)`
    /// (VisualLineSelection.swift:215-219).
    pub fn fits_visible_rows(&self, scroll_offset: u64, viewport_rows: i32) -> bool {
        let visible_rows = Self::visible_screen_rows(scroll_offset, viewport_rows);
        let selected = self.selected_rows();
        *selected.start() >= *visible_rows.start() && *selected.end() <= *visible_rows.end()
    }

    /// Converts a viewport row into an absolute screen row.
    ///
    /// Swift: static `screenRow(forViewportRow:viewportRows:scrollOffset:totalRows:)`
    /// (VisualLineSelection.swift:229-241).
    pub fn screen_row(
        viewport_row: i32,
        viewport_rows: i32,
        scroll_offset: u64,
        total_rows: Option<u64>,
    ) -> u64 {
        let row_offset = viewport_row.min(viewport_rows.max(1) - 1).max(0) as u64;
        let unclamped_row = if scroll_offset > u64::MAX - row_offset {
            u64::MAX
        } else {
            scroll_offset + row_offset
        };
        match total_rows {
            Some(total) if total > 0 => unclamped_row.min(total - 1),
            _ => unclamped_row,
        }
    }

    /// Returns the absolute screen rows visible in a viewport.
    ///
    /// Swift: static `visibleScreenRows(scrollOffset:viewportRows:)`
    /// (VisualLineSelection.swift:249-255).
    pub fn visible_screen_rows(scroll_offset: u64, viewport_rows: i32) -> RangeInclusive<u64> {
        let row_count = viewport_rows.max(1) as u64;
        let upper_row = if scroll_offset > u64::MAX - (row_count - 1) {
            u64::MAX
        } else {
            scroll_offset + row_count - 1
        };
        scroll_offset..=upper_row
    }

    /// Converts an absolute screen row into a viewport row.
    ///
    /// Swift: static `viewportRow(forScreenRow:scrollOffset:viewportRows:)`
    /// (VisualLineSelection.swift:264-267).
    pub fn viewport_row(screen_row: u64, scroll_offset: u64, viewport_rows: i32) -> i32 {
        if screen_row <= scroll_offset {
            return 0;
        }
        (viewport_rows.max(1) - 1).min(saturating_i32(screen_row - scroll_offset))
    }

    /// Applies a pending scroll line delta to a scroll offset.
    ///
    /// Swift: static `pendingScrollOffset(baseOffset:lineDelta:totalRows:)`
    /// (VisualLineSelection.swift:276-286).
    pub fn pending_scroll_offset(base_offset: u64, line_delta: i32, total_rows: Option<u64>) -> u64 {
        let delta_magnitude = (line_delta as i64).unsigned_abs();
        if line_delta > 0 {
            let unclamped_offset = if base_offset > u64::MAX - delta_magnitude {
                u64::MAX
            } else {
                base_offset + delta_magnitude
            };
            match total_rows {
                Some(total) if total > 0 => unclamped_offset.min(total - 1),
                _ => unclamped_offset,
            }
        } else {
            base_offset.saturating_sub(delta_magnitude)
        }
    }

    /// Resolves the scroll delta needed for a home/end viewport jump.
    ///
    /// Returns `None` for non-boundary movements.
    ///
    /// Swift: static `boundaryFallbackLineDelta(_:scrollOffset:totalRows:visibleRows:)`
    /// (VisualLineSelection.swift:296-316).
    pub fn boundary_fallback_line_delta(
        direction: CopyModeSelectionMove,
        scroll_offset: u64,
        total_rows: u64,
        visible_rows: u64,
    ) -> Option<i32> {
        let target_offset = match direction {
            CopyModeSelectionMove::Home => 0,
            CopyModeSelectionMove::End => total_rows.saturating_sub(visible_rows),
            _ => return None,
        };

        if target_offset >= scroll_offset {
            Some(saturating_i32(target_offset - scroll_offset))
        } else {
            Some(-saturating_i32(scroll_offset - target_offset))
        }
    }

    /// Converts a selected range into bounded rows accepted by Ghostty's C API.
    ///
    /// Returns `None` when the selection is too large.
    ///
    /// Swift: static `boundedReadRows(selectedRows:columns:maxBytes:)`
    /// (VisualLineSelection.swift:325-339).
    pub fn bounded_read_rows(
        selected_rows: RangeInclusive<u64>,
        columns: i32,
        max_bytes: u64,
    ) -> Option<(u32, u32)> {
        if columns <= 0 {
            return None;
        }
        let lower_row = u32::try_from(*selected_rows.start()).ok()?;
        let upper_row = u32::try_from(*selected_rows.end()).ok()?;
        let selected_row_count = *selected_rows.end() - *selected_rows.start() + 1;
        let estimated_bytes_per_row = ((columns as u64) * 4) + 1;
        let max_estimated_rows = max_bytes / estimated_bytes_per_row;
        if max_estimated_rows == 0 || selected_row_count > max_estimated_rows {
            return None;
        }
        Some((lower_row, upper_row))
    }

    /// Swift: private mutating `offsetEndpointScreenRow(delta:totalRows:)`
    /// (VisualLineSelection.swift:341-355).
    fn offset_endpoint_screen_row(&mut self, delta: i32, total_rows: Option<u64>) {
        let magnitude = (delta as i64).unsigned_abs();
        let moved = if delta > 0 {
            if self.endpoint_screen_row > u64::MAX - magnitude {
                u64::MAX
            } else {
                self.endpoint_screen_row + magnitude
            }
        } else {
            self.endpoint_screen_row.saturating_sub(magnitude)
        };

        match total_rows {
            Some(total) if total > 0 => self.endpoint_screen_row = moved.min(total - 1),
            _ => self.endpoint_screen_row = moved,
        }
    }
}

/// Saturating `u64 -> i32` matching Swift's `Int(clamping:)` for the small
/// line-delta / viewport-row magnitudes used above.
fn saturating_i32(value: u64) -> i32 {
    value.min(i32::MAX as u64) as i32
}

#[cfg(test)]
mod tests {
    use super::CopyModeAction::*;
    use super::CopyModeResolution::*;
    use super::CopyModeSelectionMove::*;
    use super::*;

    // MARK: - Suite: "Terminal keyboard copy mode resolver"
    // Ported from TerminalKeyboardCopyModeTests.swift `TerminalKeyboardCopyModeResolverTests`.

    #[test]
    fn resolves_all_vim_keys_with_non_ascii_layout_fallback() {
        fn ascii_provider(key_code: u16) -> Option<String> {
            match key_code {
                4 => Some("h".to_string()),
                38 => Some("j".to_string()),
                40 => Some("k".to_string()),
                37 => Some("l".to_string()),
                _ => None,
            }
        }
        let cases: [(u16, &str, CopyModeAction); 4] = [
            (4, "ㅗ", AdjustSelection(Left)),
            (38, "ㅓ", AdjustSelection(Down)),
            (40, "ㅏ", AdjustSelection(Up)),
            (37, "ㅣ", AdjustSelection(Right)),
        ];
        for (key_code, characters, action) in cases {
            assert_eq!(
                copy_mode_action_with_ascii_fallback(
                    key_code,
                    Some(characters),
                    CopyModeModifiers::EMPTY,
                    false,
                    ascii_provider,
                ),
                Some(action)
            );
        }
    }

    #[test]
    fn ignores_caps_lock_for_all_vim_motion_keys() {
        let cases: [(u16, &str, CopyModeAction); 4] = [
            (4, "h", AdjustSelection(Left)),
            (38, "j", AdjustSelection(Down)),
            (40, "k", AdjustSelection(Up)),
            (37, "l", AdjustSelection(Right)),
        ];
        for (key_code, characters, action) in cases {
            assert_eq!(
                copy_mode_action(key_code, Some(characters), CopyModeModifiers::CAPS_LOCK, false),
                Some(action)
            );
        }
    }

    #[test]
    fn line_boundary_keys_move_cursor_outside_visual_mode() {
        assert_eq!(
            copy_mode_action(29, Some("0"), CopyModeModifiers::EMPTY, false),
            Some(AdjustSelection(BeginningOfLine))
        );
        assert_eq!(
            copy_mode_action(21, Some("4"), CopyModeModifiers::SHIFT, false),
            Some(AdjustSelection(EndOfLine))
        );
    }

    #[test]
    fn zero_without_existing_count_acts_as_beginning_of_line_motion() {
        let mut state = CopyModeInputState::default();
        assert_eq!(
            copy_mode_resolve(29, Some("0"), CopyModeModifiers::EMPTY, false, &mut state),
            Perform(AdjustSelection(BeginningOfLine), 1)
        );
        assert_eq!(state, CopyModeInputState::default());
    }

    #[test]
    fn unmatched_g_prefix_clears_count_before_resolving_followup() {
        let mut state = CopyModeInputState::new(Some(3), false, true);
        assert_eq!(
            copy_mode_resolve(38, Some("j"), CopyModeModifiers::EMPTY, false, &mut state),
            Perform(AdjustSelection(Down), 1)
        );
        assert_eq!(state, CopyModeInputState::default());
    }

    #[test]
    fn unmatched_yank_line_prefix_clears_count_before_resolving_followup() {
        let mut state = CopyModeInputState::new(Some(3), true, false);
        assert_eq!(
            copy_mode_resolve(40, Some("k"), CopyModeModifiers::EMPTY, false, &mut state),
            Perform(AdjustSelection(Up), 1)
        );
        assert_eq!(state, CopyModeInputState::default());
    }

    #[test]
    fn uppercase_raw_y_without_shift_modifier_yanks_line_immediately() {
        let mut state = CopyModeInputState::default();
        assert_eq!(
            copy_mode_resolve(16, Some("Y"), CopyModeModifiers::EMPTY, false, &mut state),
            Perform(CopyLineAndExit, 1)
        );
        assert_eq!(state, CopyModeInputState::default());
    }

    #[test]
    fn uppercase_raw_v_restarts_visual_line_selection_when_selection_exists() {
        assert_eq!(
            copy_mode_action(9, Some("V"), CopyModeModifiers::EMPTY, true),
            Some(StartLineSelection)
        );
    }

    #[test]
    fn uppercase_raw_v_starts_visual_line_selection() {
        assert_eq!(
            copy_mode_action(9, Some("V"), CopyModeModifiers::EMPTY, false),
            Some(StartLineSelection)
        );
    }

    #[test]
    fn shift_v_restarts_visual_line_selection_when_selection_exists() {
        assert_eq!(
            copy_mode_action(9, Some("v"), CopyModeModifiers::SHIFT, true),
            Some(StartLineSelection)
        );
    }

    #[test]
    fn shift_v_starts_visual_line_selection() {
        assert_eq!(
            copy_mode_action(9, Some("v"), CopyModeModifiers::SHIFT, false),
            Some(StartLineSelection)
        );
    }

    #[test]
    fn caps_lock_uppercase_v_starts_character_selection() {
        assert_eq!(
            copy_mode_action(9, Some("V"), CopyModeModifiers::CAPS_LOCK, false),
            Some(StartSelection)
        );
    }

    #[test]
    fn caps_lock_uppercase_y_starts_pending_yank_line() {
        let mut state = CopyModeInputState::default();
        assert_eq!(
            copy_mode_resolve(16, Some("Y"), CopyModeModifiers::CAPS_LOCK, false, &mut state),
            Consume
        );
        assert_eq!(state, CopyModeInputState::new(None, true, false));
    }

    #[test]
    fn caps_lock_uppercase_g_starts_pending_top_jump() {
        let mut state = CopyModeInputState::default();
        assert_eq!(
            copy_mode_resolve(5, Some("G"), CopyModeModifiers::CAPS_LOCK, false, &mut state),
            Consume
        );
        assert_eq!(state, CopyModeInputState::new(None, false, true));
    }

    #[test]
    fn pending_g_then_raw_uppercase_g_resolves_bottom_jump() {
        let mut state = CopyModeInputState::new(None, false, true);
        assert_eq!(
            copy_mode_resolve(5, Some("G"), CopyModeModifiers::EMPTY, false, &mut state),
            Perform(ScrollToBottom, 1)
        );
        assert_eq!(state, CopyModeInputState::default());
    }

    #[test]
    fn caps_lock_uppercase_n_searches_forward() {
        assert_eq!(
            copy_mode_action(45, Some("N"), CopyModeModifiers::CAPS_LOCK, false),
            Some(SearchNext)
        );
    }

    // MARK: - Suite: "Terminal keyboard copy mode cursor"
    // Ported from `TerminalKeyboardCopyModeCursorPackageTests`.

    #[test]
    fn motion_then_visual_selection_uses_moved_cursor_as_anchor() {
        let mut cursor = CopyModeCursor::new(8, 7);
        let move_action = copy_mode_action(38, Some("j"), CopyModeModifiers::EMPTY, false);
        assert_eq!(move_action, Some(AdjustSelection(Down)));
        if let Some(AdjustSelection(mv)) = move_action {
            assert_eq!(cursor.move_cursor(mv, 1, 20, 40), 0);
        }
        assert_eq!(
            copy_mode_action(9, Some("v"), CopyModeModifiers::EMPTY, false),
            Some(StartSelection)
        );
        assert_eq!(cursor.clamped(20, 40), CopyModeCursor::new(9, 7));
    }

    #[test]
    fn cursor_selection_x_range_keeps_left_to_right_drag_at_right_edge() {
        let range = copy_mode_cursor_selection_x_range(99.5, 120.0, 100.0)
            .expect("expected a nonzero drag range");
        assert!((range.0 - 98.0).abs() < 0.0001);
        assert!((range.1 - 99.0).abs() < 0.0001);
    }

    #[test]
    fn viewport_offset_delta_keeps_cursor_on_same_text_after_jump() {
        let mut cursor = CopyModeCursor::new(10, 4);
        cursor.shift_for_viewport_scroll(3, 20, 8);
        assert_eq!(cursor, CopyModeCursor::new(7, 4));
    }

    #[test]
    fn clipped_backing_rows_do_not_delay_edge_scroll() {
        let rows = copy_mode_visible_viewport_rows(12, 100.0, 10.0);
        let mut cursor = CopyModeCursor::new(rows - 1, 4);
        assert_eq!(rows, 10);
        assert_eq!(cursor.move_cursor(Down, 1, rows, 8), 1);
        assert_eq!(cursor, CopyModeCursor::new(rows - 1, 4));
    }

    #[test]
    fn visual_line_movement_keeps_offscreen_endpoint_absolute() {
        let mut selection = CopyModeVisualLineSelection::new(10, 50);
        let (cursor, scroll_delta) = selection.move_endpoint(Down, 1, 7, 20, 80, 100, Some(200));
        assert_eq!(selection.selected_rows(), 10..=51);
        assert_eq!(cursor, CopyModeCursor::new(0, 7));
        assert_eq!(scroll_delta, 0);
    }

    #[test]
    fn visual_line_movement_scrolls_only_visible_endpoint_overflow() {
        let mut selection = CopyModeVisualLineSelection::new(110, 119);
        let (cursor, scroll_delta) = selection.move_endpoint(Down, 1, 4, 20, 80, 100, Some(200));
        assert_eq!(selection.selected_rows(), 110..=120);
        assert_eq!(cursor, CopyModeCursor::new(19, 4));
        assert_eq!(scroll_delta, 1);
    }

    #[test]
    fn visual_line_boundary_movement_targets_last_screen_row() {
        let mut selection = CopyModeVisualLineSelection::new(40, 95);
        let moved = selection.move_endpoint_to_boundary(End, Some(100));
        assert!(moved);
        assert_eq!(selection.selected_rows(), 40..=99);
    }

    #[test]
    fn visual_line_runtime_rows_preserve_selection_direction() {
        let mut forward = CopyModeVisualLineSelection::new(10, 20);
        let mut reverse = CopyModeVisualLineSelection::new(20, 10);
        forward.replace_selected_rows(3..=7);
        reverse.replace_selected_rows(3..=7);
        assert_eq!(forward.anchor_screen_row, 3);
        assert_eq!(forward.endpoint_screen_row, 7);
        assert_eq!(reverse.anchor_screen_row, 7);
        assert_eq!(reverse.endpoint_screen_row, 3);
        assert_eq!(forward.selected_rows(), 3..=7);
        assert_eq!(reverse.selected_rows(), 3..=7);
    }

    #[test]
    fn visual_line_movement_keeps_clipped_bottom_endpoint_absolute() {
        let mut selection = CopyModeVisualLineSelection::new(40, 95);
        let (cursor, scroll_delta) = selection.move_endpoint(Down, 1, 4, 20, 80, 76, Some(100));
        assert_eq!(selection.selected_rows(), 40..=96);
        assert_eq!(cursor, CopyModeCursor::new(19, 4));
        assert_eq!(scroll_delta, 1);
    }

    // MARK: - Parity edges from lane notes / Swift doc examples.

    #[test]
    fn clamp_count_matches_swift_doc_bounds() {
        // Count.swift docs: clampCount(0) == 1; clampCount(20_000) == max.
        assert_eq!(copy_mode_clamp_count(0), 1);
        assert_eq!(copy_mode_clamp_count(20_000), COPY_MODE_MAX_COUNT);
        assert_eq!(copy_mode_clamp_count(-5), 1);
        assert_eq!(copy_mode_clamp_count(500), 500);
    }

    #[test]
    fn command_modifier_bypasses_for_app_shortcut() {
        // KeyResolution.swift:57-62 — Command survives modifier normalization.
        assert!(copy_mode_should_bypass_for_shortcut(
            CopyModeModifiers::COMMAND | CopyModeModifiers::SHIFT
        ));
        assert!(copy_mode_should_bypass_for_shortcut(CopyModeModifiers::COMMAND));
        assert!(!copy_mode_should_bypass_for_shortcut(CopyModeModifiers::SHIFT));
        assert!(!copy_mode_should_bypass_for_shortcut(CopyModeModifiers::EMPTY));
    }

    #[test]
    fn escape_exits_and_resets_state() {
        // keyCode 53 short-circuits to exit and clears any pending prefix.
        let mut state = CopyModeInputState::new(Some(9), true, true);
        assert_eq!(
            copy_mode_resolve(53, None, CopyModeModifiers::EMPTY, false, &mut state),
            Perform(Exit, 1)
        );
        assert_eq!(state, CopyModeInputState::default());
        assert_eq!(
            copy_mode_action(53, None, CopyModeModifiers::EMPTY, false),
            Some(Exit)
        );
    }

    #[test]
    fn digit_prefix_accumulates_and_applies_count() {
        // "3" then "j" performs Down with count 3 (KeyResolution.swift:279-333).
        let mut state = CopyModeInputState::default();
        assert_eq!(
            copy_mode_resolve(20, Some("3"), CopyModeModifiers::EMPTY, false, &mut state),
            Consume
        );
        assert_eq!(state.count_prefix, Some(3));
        assert_eq!(
            copy_mode_resolve(38, Some("j"), CopyModeModifiers::EMPTY, false, &mut state),
            Perform(AdjustSelection(Down), 3)
        );
        assert_eq!(state, CopyModeInputState::default());
    }

    #[test]
    fn control_half_page_and_page_scroll_without_selection() {
        // KeyResolution.swift:131-151 control-key scroll family (no selection).
        assert_eq!(
            copy_mode_action(0, Some("d"), CopyModeModifiers::CONTROL, false),
            Some(ScrollHalfPage(1))
        );
        assert_eq!(
            copy_mode_action(0, Some("u"), CopyModeModifiers::CONTROL, false),
            Some(ScrollHalfPage(-1))
        );
        assert_eq!(
            copy_mode_action(0, Some("f"), CopyModeModifiers::CONTROL, false),
            Some(ScrollPage(1))
        );
        // With selection, control-d adjusts the selection instead.
        assert_eq!(
            copy_mode_action(0, Some("d"), CopyModeModifiers::CONTROL, true),
            Some(AdjustSelection(PageDown))
        );
    }

    #[test]
    fn selection_move_raw_values_match_ghostty_contract() {
        assert_eq!(PageUp.as_str(), "page_up");
        assert_eq!(BeginningOfLine.as_str(), "beginning_of_line");
        assert_eq!(EndOfLine.as_str(), "end_of_line");
        assert_eq!(Left.as_str(), "left");
    }

    #[test]
    fn bounded_read_rows_rejects_oversized_selection() {
        // VisualLineSelection.swift:325-339.
        assert_eq!(
            CopyModeVisualLineSelection::bounded_read_rows(0..=1, 80, 100_000),
            Some((0, 1))
        );
        // Row estimate collapses to zero when the byte budget is tiny.
        assert_eq!(
            CopyModeVisualLineSelection::bounded_read_rows(0..=1, 80, 10),
            None
        );
        assert_eq!(
            CopyModeVisualLineSelection::bounded_read_rows(0..=1, 0, 100_000),
            None
        );
    }
}
