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

/// Shared, lockable PTY input handle. Both user keystrokes and the engine's
/// query responses write through it.
type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

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
}

impl TerminalSurface {
    /// Spawn `command` on a `cols`×`rows` PTY and start pumping its output into
    /// the grid on a dedicated read thread.
    pub fn spawn(command: &ConPtyCommand, cols: u16, rows: u16) -> Result<Self, ConPtyError> {
        let pty = ConPty::spawn(command, ConPtySize::new(cols, rows))?;
        let writer: SharedWriter = Arc::new(Mutex::new(pty.take_writer()?));
        let responder = PtyResponder {
            writer: writer.clone(),
        };
        let grid = Arc::new(Mutex::new(TerminalGrid::with_listener(
            GridSize::new(cols as usize, rows as usize),
            responder,
        )));

        let mut pty_reader = pty.reader()?;
        let grid_for_thread = grid.clone();
        // Detached pump thread: it holds Arc clones (grid, reader) so it stays
        // memory-safe regardless of surface lifetime, and exits when the PTY is
        // dropped (read returns EOF). Not joined — see Drop.
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match pty_reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(mut grid) = grid_for_thread.lock() {
                            grid.advance(&buf[..n]);
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
}
