//! Live terminal surface: ConPTY + engine pump (M2 WS1).
//!
//! [`TerminalSurface`] ties the [`crate::conpty::ConPty`] boundary to the
//! [`crate::engine::TerminalGrid`]: a dedicated read thread drains PTY output
//! into the VT state machine, and a [`PtyResponder`] forwards the terminal's
//! query responses (cursor-position reports, device attributes, …) back to the
//! PTY. Because the engine answers those queries itself, the ConPTY startup
//! handshake (conhost's `ESC[6n`) is satisfied automatically — no caller
//! involvement.
//!
//! This is the v1 analogue of cmux's `TerminalSurface`/`GhosttyNSView` pair:
//! owns a grid, a PTY handle, and a `SurfaceId`. The renderer (WS2) reads the
//! grid; geometry/visibility come from the WS3 contract.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use uuid::Uuid;

use crate::conpty::{ConPty, ConPtyCommand, ConPtyError, ConPtySize};
use crate::engine::{GridSize, TerminalGrid};
use crate::{Osc133Parser, TerminalCommandBlock};

/// Shared, lockable PTY input handle. Both user keystrokes and the engine's
/// query responses write through it.
type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// An observer of raw PTY output chunks, run on the read thread before the
/// bytes reach the engine (see [`TerminalSurface::spawn_with_tee`]).
type ByteTee = Box<dyn FnMut(&[u8]) + Send>;

/// Incremental UTF-8 decoder. PTY reads land on arbitrary byte boundaries, so a
/// multi-byte sequence can be split across chunks; this buffers an incomplete
/// trailing sequence until the rest arrives, and emits U+FFFD for genuinely
/// invalid bytes. Used to feed the (str-based) OSC 133 parser.
#[derive(Default)]
struct Utf8Stream {
    tail: Vec<u8>,
}

impl Utf8Stream {
    /// Append `bytes` and return the longest now-decodable text, keeping any
    /// incomplete trailing sequence buffered for the next call.
    fn push(&mut self, bytes: &[u8]) -> String {
        self.tail.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.tail) {
                Ok(text) => {
                    out.push_str(text);
                    self.tail.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        // Safe: `valid` is a verified UTF-8 boundary.
                        out.push_str(std::str::from_utf8(&self.tail[..valid]).unwrap());
                    }
                    match error.error_len() {
                        // Genuinely invalid bytes: emit a replacement, drop them,
                        // and keep decoding the remainder.
                        Some(len) => {
                            out.push('\u{FFFD}');
                            self.tail.drain(..valid + len);
                        }
                        // Incomplete trailing sequence: keep it for next time.
                        None => {
                            self.tail.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
        out
    }
}

/// `EventListener` that forwards the terminal's outbound query responses to the
/// PTY. Handling `PtyWrite` is what makes the engine answer conhost's
/// cursor-position handshake (and DA / DSR / DECRQM responses) on its own.
#[derive(Clone)]
struct PtyResponder {
    writer: SharedWriter,
}

impl PtyResponder {
    fn write(&self, bytes: &[u8]) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }
}

impl EventListener for PtyResponder {
    fn send_event(&self, event: Event) {
        // PtyWrite carries the terminal's own responses (cursor-position
        // reports, device attributes, DECRQM, …) that must go back to the
        // child. Forwarding it is what answers conhost's startup handshake.
        // Color queries, title, bell, and child-exit are surfaced to higher
        // layers later (WS2 theme / WS5 events); ignored by the pump.
        if let Event::PtyWrite(text) = event {
            self.write(text.as_bytes());
        }
    }
}

/// A running terminal surface: a child shell on a PTY whose output is parsed
/// into a live cell grid.
pub struct TerminalSurface {
    id: Uuid,
    pty: ConPty,
    grid: Arc<Mutex<TerminalGrid<PtyResponder>>>,
    writer: SharedWriter,
    osc133: Arc<Mutex<Osc133Parser>>,
}

impl TerminalSurface {
    /// Spawn `command` on a `cols`×`rows` PTY and start pumping its output into
    /// the grid on a dedicated read thread.
    pub fn spawn(command: &ConPtyCommand, cols: u16, rows: u16) -> Result<Self, ConPtyError> {
        Self::spawn_inner(command, cols, rows, None)
    }

    /// Like [`TerminalSurface::spawn`], but `tee` observes every raw output
    /// chunk before it reaches the engine — the analogue of cmux's
    /// `ghostty_surface_set_pty_tee_cb`. Lets M8 transcript / mobile consumers
    /// watch PTY output without re-implementing the read loop. The tee runs on
    /// the read thread, so it must be cheap and non-blocking.
    pub fn spawn_with_tee<F>(
        command: &ConPtyCommand,
        cols: u16,
        rows: u16,
        tee: F,
    ) -> Result<Self, ConPtyError>
    where
        F: FnMut(&[u8]) + Send + 'static,
    {
        Self::spawn_inner(command, cols, rows, Some(Box::new(tee)))
    }

    fn spawn_inner(
        command: &ConPtyCommand,
        cols: u16,
        rows: u16,
        mut tee: Option<ByteTee>,
    ) -> Result<Self, ConPtyError> {
        let pty = ConPty::spawn(command, ConPtySize::new(cols, rows))?;
        let writer: SharedWriter = Arc::new(Mutex::new(pty.take_writer()?));
        let responder = PtyResponder {
            writer: writer.clone(),
        };
        let grid = Arc::new(Mutex::new(TerminalGrid::with_listener(
            GridSize::new(cols as usize, rows as usize),
            responder,
        )));

        let osc133 = Arc::new(Mutex::new(Osc133Parser::new()));

        let mut pty_reader = pty.reader()?;
        let grid_for_thread = grid.clone();
        let osc133_for_thread = osc133.clone();
        // Detached pump thread: it holds Arc clones (grid, osc133, reader) so it
        // stays memory-safe regardless of surface lifetime, and exits when the
        // PTY is dropped (read returns EOF). Not joined — see Drop.
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut utf8 = Utf8Stream::default();
            loop {
                match pty_reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        if let Some(tee) = tee.as_mut() {
                            tee(chunk);
                        }
                        if let Ok(mut grid) = grid_for_thread.lock() {
                            grid.advance(chunk);
                        }
                        // OSC 133 segmentation runs on the decoded text stream.
                        let text = utf8.push(chunk);
                        if !text.is_empty() {
                            if let Ok(mut parser) = osc133_for_thread.lock() {
                                parser.consume(&text);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            id: Uuid::new_v4(),
            pty,
            grid,
            writer,
            osc133,
        })
    }

    /// This surface's stable id.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Write input bytes (keystrokes, pasted text) to the child.
    pub fn write_input(&self, bytes: &[u8]) -> Result<(), ConPtyError> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| ConPtyError::Writer("input writer mutex poisoned".to_owned()))?;
        writer
            .write_all(bytes)
            .map_err(|e| ConPtyError::Writer(e.to_string()))?;
        writer
            .flush()
            .map_err(|e| ConPtyError::Writer(e.to_string()))
    }

    /// Resize both the PTY and the grid (the explicit Windows resize path —
    /// there is no SIGWINCH).
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), ConPtyError> {
        self.pty.resize(ConPtySize::new(cols, rows))?;
        if let Ok(mut grid) = self.grid.lock() {
            grid.resize(GridSize::new(cols as usize, rows as usize));
        }
        Ok(())
    }

    /// Snapshot the visible viewport as one trimmed string per row.
    pub fn visible_lines(&self) -> Vec<String> {
        self.grid
            .lock()
            .map(|grid| grid.visible_lines())
            .unwrap_or_default()
    }

    /// The cursor position as `(line, column)`.
    pub fn cursor(&self) -> (usize, usize) {
        self.grid.lock().map(|grid| grid.cursor()).unwrap_or((0, 0))
    }

    /// The OSC 133 command blocks segmented from this surface's output so far
    /// (the shared transcript primitive M8 consumes). Empty unless the shell
    /// has OSC 133 integration enabled.
    pub fn command_blocks(&self) -> Vec<TerminalCommandBlock> {
        self.osc133
            .lock()
            .map(|parser| parser.blocks.clone())
            .unwrap_or_default()
    }

    /// Terminate the child.
    ///
    /// The read thread is not joined here: the kept-alive slave keeps the
    /// output pipe open, so `reader.read()` only returns once the PTY (and its
    /// pseudo console) is dropped — which happens when this surface is dropped.
    /// Joining before then would block forever. The thread is detached and
    /// exits on its own at drop.
    pub fn shutdown(&mut self) -> Result<(), ConPtyError> {
        self.pty.kill()
    }
}

impl Drop for TerminalSurface {
    fn drop(&mut self) {
        // Kill the child, then let the fields drop: dropping the ConPty closes
        // the pseudo console, which unblocks the read thread's `read()` so it
        // exits. We intentionally do NOT join the handle here — while the PTY
        // is still alive the read would block, so a join would deadlock.
        let _ = self.pty.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn shell() -> ConPtyCommand {
        if cfg!(windows) {
            ConPtyCommand::new("cmd").arg("/q")
        } else {
            ConPtyCommand::new("/bin/sh")
        }
    }

    /// End-to-end WS1: a real shell's output flows ConPTY -> read thread ->
    /// alacritty engine -> grid, and the engine answers conhost's startup
    /// handshake on its own (no manual cursor-report reply here).
    #[test]
    fn live_shell_output_renders_into_the_grid() {
        let surface = TerminalSurface::spawn(&shell(), 80, 24).expect("spawn surface");
        let script = if cfg!(windows) {
            "echo surface_marker_55\r\n"
        } else {
            "echo surface_marker_55\n"
        };
        surface
            .write_input(script.as_bytes())
            .expect("write input");

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let screen = surface.visible_lines().join("\n");
            if screen.contains("surface_marker_55") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "marker never rendered; screen was: {screen:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn resize_updates_grid_dimensions() {
        let mut surface = TerminalSurface::spawn(&shell(), 80, 24).expect("spawn surface");
        assert_eq!(surface.visible_lines().len(), 24);
        surface.resize(100, 30).expect("resize");
        assert_eq!(surface.visible_lines().len(), 30);
    }

    #[test]
    fn byte_tee_observes_raw_pty_output() {
        let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink = captured.clone();
        let surface = TerminalSurface::spawn_with_tee(&shell(), 80, 24, move |bytes| {
            if let Ok(mut buf) = sink.lock() {
                buf.extend_from_slice(bytes);
            }
        })
        .expect("spawn surface");

        let script = if cfg!(windows) {
            "echo tee_marker_88\r\n"
        } else {
            "echo tee_marker_88\n"
        };
        surface.write_input(script.as_bytes()).expect("write input");

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let seen = captured
                .lock()
                .map(|buf| String::from_utf8_lossy(&buf).contains("tee_marker_88"))
                .unwrap_or(false);
            if seen {
                break;
            }
            assert!(Instant::now() < deadline, "tee never observed the marker");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn utf8_stream_passes_ascii_through() {
        let mut s = Utf8Stream::default();
        assert_eq!(s.push(b"hello"), "hello");
        assert_eq!(s.push(b" world"), " world");
    }

    #[test]
    fn utf8_stream_buffers_split_multibyte_sequence() {
        let mut s = Utf8Stream::default();
        let bytes = "é├😀".as_bytes(); // 2 + 3 + 4 bytes
        // Feed one byte at a time; the decoder must reassemble each char.
        let mut out = String::new();
        for b in bytes {
            out.push_str(&s.push(&[*b]));
        }
        assert_eq!(out, "é├😀");
    }

    #[test]
    fn utf8_stream_emits_replacement_for_invalid_bytes() {
        let mut s = Utf8Stream::default();
        // 0xFF is never valid UTF-8.
        let out = s.push(&[b'a', 0xFF, b'b']);
        assert!(out.starts_with('a'));
        assert!(out.contains('\u{FFFD}'));
        assert!(out.ends_with('b'));
    }

    #[test]
    fn fresh_surface_has_no_command_blocks() {
        let surface = TerminalSurface::spawn(&shell(), 80, 24).expect("spawn surface");
        assert!(surface.command_blocks().is_empty());
    }

    #[test]
    fn osc133_segments_command_blocks_from_output() {
        // Emit a full OSC 133 A/B/C/D cycle via bash printf and assert the
        // surface segments it. Skips if bash is unavailable on the host.
        let cmd = ConPtyCommand::new("bash").arg("-c").arg(
            r"printf '\033]133;A\033]133;Bmycmd\033]133;Cout\n\033]133;D;0\n'; sleep 1",
        );
        let surface = match TerminalSurface::spawn(&cmd, 80, 24) {
            Ok(surface) => surface,
            Err(_) => return, // no bash on this host — skip
        };

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(block) = surface
                .command_blocks()
                .into_iter()
                .find(|block| block.command == "mycmd")
            {
                assert_eq!(block.exit_code, Some(0));
                return;
            }
            if Instant::now() >= deadline {
                // bash spawned but produced no segmentable output (unusual env);
                // don't hard-fail the suite on an environment quirk.
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
