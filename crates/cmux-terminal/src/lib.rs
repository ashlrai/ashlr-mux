//! OSC 133 shell-integration command segmentation.
//!
//! Incremental parser that turns a raw PTY character stream into
//! `TerminalCommandBlock`s using OSC 133 prompt/command/output/end markers
//! (`A`/`B`/`C`/`D`), with BEL and `ESC \` (ST) terminators, carriage-return
//! progress folding, alt-screen interactive detection, and split-escape
//! carry-over across `consume` calls. Input is validated UTF-8; the byte->str
//! decode is owned by the M2/M3 PTY pump. Consumed by the terminal renderer
//! (M2) and agent transcripts (M8).
//!
//! Swift parity source:
//! `Packages/Shared/CmuxAgentChat/Sources/CmuxAgentChat/Parsing/OSC133CommandParser.swift`

pub mod conpty;
pub mod engine;
pub mod geometry;
pub mod links;
pub mod sanitize;
pub mod surface;
pub mod theme;
pub mod top_label;
pub mod transcript;

pub use sanitize::sanitize_external_committed_text;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCommandBlock {
    pub id: i32,
    pub command: String,
    #[serde(default, rename = "exit_code", skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default = "default_true", rename = "is_running")]
    pub is_running: bool,
    #[serde(default, rename = "is_interactive")]
    pub is_interactive: bool,
    #[serde(default)]
    pub output: String,
}

impl TerminalCommandBlock {
    pub fn new(id: i32, command: impl Into<String>) -> Self {
        Self {
            id,
            command: command.into(),
            exit_code: None,
            is_running: true,
            is_interactive: false,
            output: String::new(),
        }
    }

    pub fn failed(&self) -> bool {
        self.exit_code.is_some_and(|code| code != 0)
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Prompt,
    Command,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscapeAction {
    PromptStart,
    CommandStart,
    OutputStart,
    CommandEnd(Option<i32>),
    EnterAltScreen,
    LeaveAltScreen,
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Osc133Parser {
    pub blocks: Vec<TerminalCommandBlock>,
    phase: Phase,
    command_buffer: String,
    folded_output: String,
    open_line: String,
    pending: String,
    next_id: i32,
    open_index: Option<usize>,
}

impl Default for Osc133Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Osc133Parser {
    const MAX_ESCAPE_LENGTH: usize = 8192;

    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            phase: Phase::Idle,
            command_buffer: String::new(),
            folded_output: String::new(),
            open_line: String::new(),
            pending: String::new(),
            next_id: 0,
            open_index: None,
        }
    }

    pub fn consume(&mut self, text: &str) {
        let stream = format!("{}{}", self.pending, text);
        self.pending.clear();

        let chars: Vec<char> = stream.chars().collect();
        let mut index = 0;

        while index < chars.len() {
            let ch = chars[index];
            if ch != '\u{1b}' {
                self.append_text(ch);
                index += 1;
                continue;
            }

            match self.parse_escape(&chars, index) {
                Some((next, action)) => {
                    self.apply(action);
                    index = next;
                }
                None => {
                    self.flush_open_output();
                    self.pending = chars[index..].iter().collect();
                    return;
                }
            }
        }

        self.flush_open_output();
    }

    fn flush_open_output(&mut self) {
        if self.phase == Phase::Output {
            if let Some(open_index) = self.open_index {
                self.blocks[open_index].output =
                    format!("{}{}", self.folded_output, Self::fold_line(&self.open_line));
            }
        }
    }

    fn parse_escape(&self, chars: &[char], start: usize) -> Option<(usize, EscapeAction)> {
        let after_esc = start + 1;
        if after_esc >= chars.len() {
            return None;
        }

        match chars[after_esc] {
            ']' => self.parse_osc(chars, after_esc + 1),
            '[' => self.parse_csi(chars, after_esc + 1),
            _ => Some((after_esc + 1, EscapeAction::Ignore)),
        }
    }

    fn parse_osc(&self, chars: &[char], mut index: usize) -> Option<(usize, EscapeAction)> {
        let mut body = String::new();
        while index < chars.len() {
            if body.len() >= Self::MAX_ESCAPE_LENGTH {
                return Some((index, EscapeAction::Ignore));
            }
            match chars[index] {
                '\u{07}' => return Some((index + 1, Self::osc_action(&body))),
                '\u{1b}' => {
                    if index + 1 >= chars.len() {
                        return None;
                    }
                    if chars[index + 1] == '\\' {
                        return Some((index + 2, Self::osc_action(&body)));
                    }
                    return Some((index, Self::osc_action(&body)));
                }
                ch => body.push(ch),
            }
            index += 1;
        }
        None
    }

    fn parse_csi(&self, chars: &[char], mut index: usize) -> Option<(usize, EscapeAction)> {
        let mut params = String::new();
        while index < chars.len() {
            if params.len() >= Self::MAX_ESCAPE_LENGTH {
                return Some((index, EscapeAction::Ignore));
            }
            let ch = chars[index];
            if ('@'..='~').contains(&ch) {
                let action = if Self::csi_alt_screen(&params) && ch == 'h' {
                    EscapeAction::EnterAltScreen
                } else if Self::csi_alt_screen(&params) && ch == 'l' {
                    EscapeAction::LeaveAltScreen
                } else {
                    EscapeAction::Ignore
                };
                return Some((index + 1, action));
            }
            params.push(ch);
            index += 1;
        }
        None
    }

    fn osc_action(body: &str) -> EscapeAction {
        let Some(rest) = body.strip_prefix("133;") else {
            return EscapeAction::Ignore;
        };
        let Some(kind) = rest.chars().next() else {
            return EscapeAction::Ignore;
        };
        match kind {
            'A' => EscapeAction::PromptStart,
            'B' => EscapeAction::CommandStart,
            'C' => EscapeAction::OutputStart,
            'D' => {
                let mut parts = rest.split(';');
                let _ = parts.next();
                let exit_code = parts.next().and_then(|value| value.parse::<i32>().ok());
                EscapeAction::CommandEnd(exit_code)
            }
            _ => EscapeAction::Ignore,
        }
    }

    fn csi_alt_screen(params: &str) -> bool {
        params
            .strip_prefix('?')
            .map(|rest| rest.split(';').any(|value| value == "1049" || value == "1047"))
            .unwrap_or(false)
    }

    fn apply(&mut self, action: EscapeAction) {
        match action {
            EscapeAction::PromptStart => {
                self.finalize_open_output();
                self.phase = Phase::Prompt;
            }
            EscapeAction::CommandStart => {
                self.command_buffer.clear();
                self.phase = Phase::Command;
            }
            EscapeAction::OutputStart => {
                self.open_block();
                self.folded_output.clear();
                self.open_line.clear();
                self.phase = Phase::Output;
            }
            EscapeAction::CommandEnd(exit_code) => {
                self.close_block(exit_code);
                self.phase = Phase::Idle;
            }
            EscapeAction::EnterAltScreen => {
                if let Some(open_index) = self.open_index {
                    self.blocks[open_index].is_interactive = true;
                }
            }
            EscapeAction::LeaveAltScreen | EscapeAction::Ignore => {}
        }
    }

    fn append_text(&mut self, ch: char) {
        match self.phase {
            Phase::Command => self.command_buffer.push(ch),
            Phase::Output => {
                if ch == '\n' {
                    self.folded_output.push_str(&Self::fold_line(&self.open_line));
                    self.folded_output.push('\n');
                    self.open_line.clear();
                } else {
                    self.open_line.push(ch);
                }
            }
            Phase::Idle | Phase::Prompt => {}
        }
    }

    fn open_block(&mut self) {
        let block = TerminalCommandBlock::new(self.next_id, self.command_buffer.trim());
        self.next_id += 1;
        self.blocks.push(block);
        self.open_index = Some(self.blocks.len() - 1);
    }

    fn close_block(&mut self, exit_code: Option<i32>) {
        if let Some(open_index) = self.open_index {
            self.blocks[open_index].output =
                format!("{}{}", self.folded_output, Self::fold_line(&self.open_line));
            self.blocks[open_index].exit_code = exit_code;
            self.blocks[open_index].is_running = false;
        }
        self.open_index = None;
        self.folded_output.clear();
        self.open_line.clear();
    }

    fn finalize_open_output(&mut self) {
        if self.open_index.is_some() {
            self.close_block(None);
        }
    }

    fn fold_line(line: &str) -> String {
        let mut trimmed = line;
        if let Some(stripped) = trimmed.strip_suffix('\r') {
            trimmed = stripped;
        }
        match trimmed.rsplit_once('\r') {
            Some((_, tail)) => tail.to_owned(),
            None => trimmed.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc(body: &str) -> String {
        format!("\u{1b}]{}\u{7}", body)
    }

    fn mark(kind: &str) -> String {
        esc(&format!("133;{}", kind))
    }

    #[test]
    fn happy_path() {
        let mut parser = Osc133Parser::new();
        parser.consume(&(mark("A") + "user@host$ " + &mark("B") + "echo hi" + &mark("C") + "hi\n" + &mark("D;0")));
        assert_eq!(parser.blocks.len(), 1);
        let block = &parser.blocks[0];
        assert_eq!(block.command, "echo hi");
        assert_eq!(block.output, "hi\n");
        assert_eq!(block.exit_code, Some(0));
        assert!(!block.is_running);
        assert!(!block.failed());
    }

    #[test]
    fn failure_marks_block_failed() {
        let mut parser = Osc133Parser::new();
        parser.consume(&(mark("A") + &mark("B") + "false" + &mark("C") + &mark("D;1")));
        assert_eq!(parser.blocks[0].exit_code, Some(1));
        assert!(parser.blocks[0].failed());
    }

    #[test]
    fn running_until_next_prompt() {
        let mut parser = Osc133Parser::new();
        parser.consume(&(mark("A") + &mark("B") + "sleep 5" + &mark("C") + "working"));
        assert_eq!(parser.blocks.len(), 1);
        assert!(parser.blocks[0].is_running);
        assert_eq!(parser.blocks[0].output, "working");
        parser.consume(&mark("A"));
        assert!(!parser.blocks[0].is_running);
        assert_eq!(parser.blocks[0].exit_code, None);
    }

    #[test]
    fn split_escape_parses_when_completed() {
        let mut parser = Osc133Parser::new();
        let full = mark("A") + &mark("B") + "id" + &mark("C") + "uid=0\n" + &mark("D;0");
        let mid = 3;
        parser.consume(&full[..mid]);
        parser.consume(&full[mid..]);
        assert_eq!(parser.blocks.len(), 1);
        assert_eq!(parser.blocks[0].command, "id");
        assert_eq!(parser.blocks[0].output, "uid=0\n");
        assert_eq!(parser.blocks[0].exit_code, Some(0));
    }

    #[test]
    fn carriage_return_progress_folds() {
        let mut parser = Osc133Parser::new();
        parser.consume(&(mark("A") + &mark("B") + "dl" + &mark("C") + "10%\r50%\r100%\n" + &mark("D;0")));
        assert_eq!(parser.blocks[0].output, "100%\n");
    }

    #[test]
    fn alt_screen_marks_interactive() {
        let mut parser = Osc133Parser::new();
        parser.consume(&(mark("A") + &mark("B") + "vim" + &mark("C") + "\u{1b}[?1049h"));
        assert!(parser.blocks[0].is_interactive);
    }

    #[test]
    fn strips_other_sequences() {
        let mut parser = Osc133Parser::new();
        let noise = "\u{1b}]0;my title\u{7}\u{1b}[31mred\u{1b}[0m";
        parser.consume(&(mark("A") + &mark("B") + "x" + &mark("C") + noise + "\n" + &mark("D;0")));
        assert_eq!(parser.blocks[0].output, "red\n");
    }

    #[test]
    fn crlf_is_preserved() {
        let mut parser = Osc133Parser::new();
        parser.consume(&(mark("A") + &mark("B") + "x" + &mark("C") + "line1\r\nline2\r\n" + &mark("D;0")));
        assert_eq!(parser.blocks[0].output, "line1\nline2\n");
    }

    #[test]
    fn byte_at_a_time_matches_full_stream() {
        let mut parser = Osc133Parser::new();
        let full = mark("A")
            + &mark("B")
            + "run"
            + &mark("C")
            + "start\r\n10%\r99%\r100%\r\ndone\r\n"
            + &mark("D;0");
        for ch in full.chars() {
            parser.consume(&ch.to_string());
        }
        assert_eq!(parser.blocks.len(), 1);
        assert_eq!(parser.blocks[0].command, "run");
        assert_eq!(parser.blocks[0].output, "start\n100%\ndone\n");
        assert_eq!(parser.blocks[0].exit_code, Some(0));
    }

    #[test]
    fn runaway_unterminated_escape_is_bounded_and_recovers() {
        let mut parser = Osc133Parser::new();
        // An OSC sequence whose body never terminates and exceeds the runaway
        // guard must be abandoned rather than accumulated unboundedly, and the
        // parser must keep working on the next well-formed sequence.
        let runaway = format!("\u{1b}]{}", "x".repeat(Osc133Parser::MAX_ESCAPE_LENGTH + 500));
        parser.consume(&runaway);
        assert!(parser.blocks.is_empty());

        parser.consume(&(mark("A") + &mark("B") + "echo ok" + &mark("C") + "ok\n" + &mark("D;0")));
        assert_eq!(parser.blocks.len(), 1);
        assert_eq!(parser.blocks[0].command, "echo ok");
        assert_eq!(parser.blocks[0].output, "ok\n");
        assert_eq!(parser.blocks[0].exit_code, Some(0));
    }

    #[test]
    fn st_terminator_parses_like_bel() {
        // OSC 133 markers may be terminated by ST (ESC \) instead of BEL; both
        // must segment identically.
        let st = |body: &str| format!("\u{1b}]{}\u{1b}\\", body);
        let mut parser = Osc133Parser::new();
        parser.consume(
            &(st("133;A") + &st("133;B") + "echo hi" + &st("133;C") + "hi\n" + &st("133;D;0")),
        );
        assert_eq!(parser.blocks.len(), 1);
        assert_eq!(parser.blocks[0].command, "echo hi");
        assert_eq!(parser.blocks[0].output, "hi\n");
        assert_eq!(parser.blocks[0].exit_code, Some(0));
    }
}
