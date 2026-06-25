//! Golden-file parity for `cmux-terminal` OSC 133 block segmentation.
//!
//! Feeds captured PTY chunk sequences through `Osc133Parser::consume` and pins
//! the resulting `Vec<TerminalCommandBlock>` (counts, exit codes, command text,
//! folded output, running/interactive flags) as canonical JSON.
//!
//! Each transcript is replayed two ways — as one chunk and byte-at-a-time — and
//! both MUST yield identical blocks, guarding the split-escape / CRLF-fold rules
//! across chunk boundaries.
//!
//! Fixtures are Rust-seeded placeholders. The macOS Swift exporter
//! (`OSC133CommandParser` in `Packages/Shared/CmuxAgentChat`) is authoritative;
//! `TerminalCommandBlock`'s `Codable` keys (`exit_code`, `is_running`,
//! `is_interactive`) match the Rust `#[serde(rename = …)]` shape.

mod support;

use cmux_terminal::{Osc133Parser, TerminalCommandBlock};
use support::assert_canonical_fixture;

const DOMAIN: &str = "osc133";

fn esc(body: &str) -> String {
    format!("\u{1b}]{body}\u{07}")
}

fn mark(kind: &str) -> String {
    esc(&format!("133;{kind}"))
}

fn blocks_value(blocks: &[TerminalCommandBlock]) -> serde_json::Value {
    serde_json::to_value(blocks).expect("blocks serialize")
}

/// Replay `transcript` whole and byte-at-a-time; assert identical blocks; then
/// pin the canonical JSON of the blocks.
fn assert_transcript(name: &str, transcript: &str) {
    let mut whole = Osc133Parser::new();
    whole.consume(transcript);

    let mut streamed = Osc133Parser::new();
    for ch in transcript.chars() {
        streamed.consume(&ch.to_string());
    }

    assert_eq!(
        whole.blocks, streamed.blocks,
        "chunked vs whole replay diverged for {name}"
    );

    assert_canonical_fixture(DOMAIN, name, &blocks_value(&whole.blocks));
}

#[test]
fn happy_path_single_command() {
    let t = mark("A") + "user@host$ " + &mark("B") + "echo hi" + &mark("C") + "hi\n" + &mark("D;0");
    assert_transcript("happy_path", &t);
}

#[test]
fn nonzero_exit_marks_failure() {
    let t = mark("A") + &mark("B") + "false" + &mark("C") + &mark("D;1");
    assert_transcript("nonzero_exit", &t);
}

#[test]
fn two_commands_sequenced() {
    let t = mark("A")
        + &mark("B")
        + "pwd"
        + &mark("C")
        + "/home/u\n"
        + &mark("D;0")
        + &mark("A")
        + &mark("B")
        + "ls"
        + &mark("C")
        + "a\nb\n"
        + &mark("D;0");
    assert_transcript("two_commands", &t);
}

#[test]
fn running_command_without_exit() {
    // Output started but no D mark: block stays running with accumulated output.
    let t = mark("A") + &mark("B") + "sleep 5" + &mark("C") + "working";
    assert_transcript("running_no_exit", &t);
}

#[test]
fn carriage_return_progress_folds() {
    let t = mark("A") + &mark("B") + "dl" + &mark("C") + "10%\r50%\r100%\n" + &mark("D;0");
    assert_transcript("cr_fold", &t);
}

#[test]
fn crlf_preserved_as_lf() {
    let t = mark("A") + &mark("B") + "x" + &mark("C") + "line1\r\nline2\r\n" + &mark("D;0");
    assert_transcript("crlf_lf", &t);
}

#[test]
fn alt_screen_marks_interactive() {
    let t = mark("A") + &mark("B") + "vim" + &mark("C") + "\u{1b}[?1049h";
    assert_transcript("alt_screen_interactive", &t);
}

#[test]
fn strips_unrelated_sequences() {
    // OSC 0 title set + SGR color codes must be stripped from output text.
    let noise = "\u{1b}]0;my title\u{07}\u{1b}[31mred\u{1b}[0m";
    let t = mark("A") + &mark("B") + "x" + &mark("C") + noise + "\n" + &mark("D;0");
    assert_transcript("strips_noise", &t);
}
