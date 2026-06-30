//! Shared ConPTY abstraction (M2 WS1, cross-cutting rule 3).
//!
//! A thin, engine-agnostic wrapper over `portable_pty`'s native PTY (ConPTY on
//! Windows, a real pty on Unix) so the terminal engine, process supervision
//! (M3), and the remote daemon (M4) all spawn/resize/kill through one boundary
//! rather than re-deriving ConPTY FFI. There is **no SIGWINCH** on Windows —
//! resizing is the explicit [`ConPty::resize`] call, which forwards to
//! `ResizePseudoConsole` under the hood.
//!
//! The wrapper deliberately does not own a parser or a read thread: callers
//! take an independent [`ConPty::reader`] (run it on a dedicated thread per
//! WS1) and a single [`ConPty::take_writer`], leaving the engine free to wire
//! its own byte tee.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize, SlavePty};

/// PTY grid size in character cells. Pixel dimensions are reported to the child
/// as zero (cmux drives layout from cells, not pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConPtySize {
    pub cols: u16,
    pub rows: u16,
}

impl ConPtySize {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows }
    }

    fn to_pty_size(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

/// Spec for the child process to launch on the PTY. Mirrors the construction
/// seam of cmux's surface init (program / args / cwd / env overrides) without
/// leaking `portable_pty`'s builder across the boundary.
#[derive(Debug, Clone)]
pub struct ConPtyCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Environment overrides layered on top of the inherited environment.
    pub env: BTreeMap<String, String>,
}

impl ConPtyCommand {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    fn to_builder(&self) -> CommandBuilder {
        let mut builder = CommandBuilder::new(&self.program);
        for arg in &self.args {
            builder.arg(arg);
        }
        if let Some(cwd) = &self.cwd {
            builder.cwd(cwd);
        }
        for (key, value) in &self.env {
            builder.env(key, value);
        }
        builder
    }
}

/// Errors from the ConPTY boundary. `portable_pty` surfaces `anyhow` errors;
/// they are flattened to strings so this crate does not take an `anyhow`
/// dependency.
#[derive(Debug, thiserror::Error)]
pub enum ConPtyError {
    #[error("failed to open pseudo console: {0}")]
    Open(String),
    #[error("failed to spawn child {program:?}: {message}")]
    Spawn { program: String, message: String },
    #[error("failed to clone pty reader: {0}")]
    Reader(String),
    #[error("failed to take pty writer: {0}")]
    Writer(String),
    #[error("failed to resize pty: {0}")]
    Resize(String),
    #[error("failed to signal child: {0}")]
    Kill(String),
    #[error("failed to wait on child: {0}")]
    Wait(String),
}

/// A spawned child attached to a pseudo console. Owns the master side and the
/// child handle; closing it (drop) tears down the PTY.
pub struct ConPty {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    // Held for the PTY's lifetime: on Windows the slave owns the PseudoConsole
    // handle, and dropping it closes the console and severs the child's output
    // pipe. Kept alive here, dropped with the rest of the PTY on teardown.
    _slave: Box<dyn SlavePty + Send>,
    size: ConPtySize,
}

impl ConPty {
    /// Open a pseudo console of `size` and spawn `command` on it.
    pub fn spawn(command: &ConPtyCommand, size: ConPtySize) -> Result<Self, ConPtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size.to_pty_size())
            .map_err(|e| ConPtyError::Open(e.to_string()))?;

        let child = pair
            .slave
            .spawn_command(command.to_builder())
            .map_err(|e| ConPtyError::Spawn {
                program: command.program.clone(),
                message: e.to_string(),
            })?;

        Ok(Self {
            master: pair.master,
            child,
            _slave: pair.slave,
            size,
        })
    }

    /// An independent reader over the child's output. Safe to move to a
    /// dedicated read thread; multiple readers may be cloned.
    pub fn reader(&self) -> Result<Box<dyn Read + Send>, ConPtyError> {
        self.master
            .try_clone_reader()
            .map_err(|e| ConPtyError::Reader(e.to_string()))
    }

    /// The single writer into the child's input. Returns an error if the writer
    /// has already been taken.
    pub fn take_writer(&self) -> Result<Box<dyn Write + Send>, ConPtyError> {
        self.master
            .take_writer()
            .map_err(|e| ConPtyError::Writer(e.to_string()))
    }

    /// Resize the pseudo console (the explicit Windows analogue of SIGWINCH).
    pub fn resize(&mut self, size: ConPtySize) -> Result<(), ConPtyError> {
        self.master
            .resize(size.to_pty_size())
            .map_err(|e| ConPtyError::Resize(e.to_string()))?;
        self.size = size;
        Ok(())
    }

    /// The current grid size.
    pub fn size(&self) -> ConPtySize {
        self.size
    }

    /// Forcibly terminate the child.
    pub fn kill(&mut self) -> Result<(), ConPtyError> {
        self.child
            .kill()
            .map_err(|e| ConPtyError::Kill(e.to_string()))
    }

    /// Block until the child exits, returning its exit code.
    pub fn wait(&mut self) -> Result<u32, ConPtyError> {
        let status = self
            .child
            .wait()
            .map_err(|e| ConPtyError::Wait(e.to_string()))?;
        Ok(status.exit_code())
    }

    /// Poll whether the child has exited, returning its exit code if so.
    pub fn try_wait(&mut self) -> Result<Option<u32>, ConPtyError> {
        let status = self
            .child
            .try_wait()
            .map_err(|e| ConPtyError::Wait(e.to_string()))?;
        Ok(status.map(|s| s.exit_code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// A shell command that echoes `msg` and exits, portable across the
    /// platforms the crate is tested on.
    fn echo_command(msg: &str) -> ConPtyCommand {
        if cfg!(windows) {
            ConPtyCommand::new("cmd")
                .arg("/c")
                .arg(format!("echo {msg}"))
        } else {
            ConPtyCommand::new("/bin/sh")
                .arg("-c")
                .arg(format!("echo {msg}"))
        }
    }

    /// Read incrementally on a worker thread until `marker` appears in the
    /// output, returning whether it was seen within `secs`. Polling for a
    /// marker (rather than reading to EOF) avoids depending on the master
    /// closing — a ConPTY only signals EOF once its master is dropped, which
    /// the caller does *after* this returns.
    /// Drive a freshly-spawned pty until `marker` appears in its output.
    ///
    /// On startup conhost emits a cursor-position request (`ESC[6n`) and
    /// withholds the child's output until the terminal answers; a real engine
    /// (alacritty) replies automatically, so the test replies with a
    /// `ESC[1;1R` cursor report. Any `input` is sent after the handshake.
    /// Polling for `marker` (rather than reading to EOF) avoids depending on
    /// the master closing.
    fn drive_until_marker(pty: &ConPty, input: Option<&str>, marker: &str, secs: u64) -> bool {
        let mut writer = pty.take_writer().expect("take writer");
        let mut reader = pty.reader().expect("clone reader");

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let deadline = Instant::now() + Duration::from_secs(secs);
        let mut acc = String::new();
        let mut answered = false;
        loop {
            if !answered && acc.contains("\u{1b}[6n") {
                let _ = writer.write_all(b"\x1b[1;1R");
                let _ = writer.flush();
                if let Some(text) = input {
                    let _ = writer.write_all(text.as_bytes());
                    let _ = writer.flush();
                }
                answered = true;
            }
            if acc.contains(marker) {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return acc.contains(marker);
            }
            match rx.recv_timeout(remaining.min(Duration::from_millis(250))) {
                Ok(chunk) => acc.push_str(&String::from_utf8_lossy(&chunk)),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return acc.contains(marker),
            }
        }
    }

    #[test]
    fn spawns_a_shell_and_reads_echoed_output() {
        let pty = ConPty::spawn(&echo_command("conpty_smoke_42"), ConPtySize::new(80, 24))
            .expect("spawn pty");
        let seen = drive_until_marker(&pty, None, "conpty_smoke_42", 20);
        // Keep the pty alive until the read completes, then close it.
        drop(pty);
        assert!(seen, "echoed output not found");
    }

    #[test]
    fn resize_updates_reported_size() {
        let mut pty = ConPty::spawn(&echo_command("resize_probe"), ConPtySize::new(80, 24))
            .expect("spawn pty");
        assert_eq!(pty.size(), ConPtySize::new(80, 24));
        pty.resize(ConPtySize::new(120, 40)).expect("resize");
        assert_eq!(pty.size(), ConPtySize::new(120, 40));
    }

    #[test]
    fn writes_input_and_reads_response() {
        let shell = if cfg!(windows) {
            ConPtyCommand::new("cmd").arg("/q")
        } else {
            ConPtyCommand::new("/bin/sh")
        };
        let pty = ConPty::spawn(&shell, ConPtySize::new(80, 24)).expect("spawn pty");

        let script = if cfg!(windows) {
            "echo conpty_input_77& exit\r\n"
        } else {
            "echo conpty_input_77; exit\n"
        };
        let seen = drive_until_marker(&pty, Some(script), "conpty_input_77", 20);
        drop(pty);
        assert!(seen, "command output not found");
    }
}
