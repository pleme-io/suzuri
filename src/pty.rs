use portable_pty::{CommandBuilder, MasterPty, PtyPair, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use crate::errors::{Result, SuzuriError};

/// Messages from the PTY reader thread to the main event loop.
pub enum PtyEvent {
    /// Output bytes from the child process.
    Output(Vec<u8>),
    /// Child process exited.
    Exit(i32),
}

/// Manages the pseudo-terminal and child process.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

impl Pty {
    /// Spawn a shell process inside a new PTY.
    ///
    /// Returns `(Pty, Receiver<PtyEvent>)`. The receiver delivers output
    /// bytes and exit notifications from a background reader thread.
    pub fn spawn(
        shell: &str,
        args: &[String],
        cols: u16,
        rows: u16,
    ) -> Result<(Self, mpsc::Receiver<PtyEvent>)> {
        let pty_system = native_pty_system();

        let pair: PtyPair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| SuzuriError::Pty(e.to_string()))?;

        let mut cmd = CommandBuilder::new(shell);
        for arg in args {
            cmd.arg(arg);
        }

        // Inherit environment
        for (key, val) in std::env::vars() {
            cmd.env(key, val);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| SuzuriError::Pty(e.to_string()))?;

        // Drop slave side — master owns the connection now
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| SuzuriError::Pty(e.to_string()))?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| SuzuriError::Pty(e.to_string()))?;

        let (tx, rx) = mpsc::channel();

        // Reader thread: read PTY output and send to main loop
        let tx_output = tx.clone();
        thread::Builder::new()
            .name("suzuri-pty-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if tx_output
                                .send(PtyEvent::Output(buf[..n].to_vec()))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::debug!("PTY read error: {e}");
                            break;
                        }
                    }
                }
            })
            .map_err(|e| SuzuriError::Pty(e.to_string()))?;

        // Waiter thread: wait for child exit
        let tx_exit = tx;
        thread::Builder::new()
            .name("suzuri-pty-waiter".into())
            .spawn(move || {
                let status = child.wait().ok();
                let code = status
                    .map(|s| s.exit_code() as i32)
                    .unwrap_or(-1);
                let _ = tx_exit.send(PtyEvent::Exit(code));
            })
            .map_err(|e| SuzuriError::Pty(e.to_string()))?;

        Ok((
            Self {
                master: pair.master,
                writer,
            },
            rx,
        ))
    }

    /// Write bytes to the PTY (keyboard input → shell).
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.writer
            .write_all(data)
            .map_err(|e| SuzuriError::Pty(e.to_string()))?;
        self.writer
            .flush()
            .map_err(|e| SuzuriError::Pty(e.to_string()))?;
        Ok(())
    }

    /// Resize the PTY to match the terminal grid.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| SuzuriError::Pty(e.to_string()))
    }
}
