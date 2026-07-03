//! Controlling-terminal access and raw mode — std only, no libc.
//!
//! We toggle raw/-echo by shelling out to `stty` on `/dev/tty`, and read keys
//! straight from `/dev/tty` so the menu still works when stdin/stdout are
//! redirected (e.g. `choice=$(menu select -- a b c)`). The interactive frames
//! are written to `/dev/tty` too, keeping stdout clean for the captured result.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};

/// Open the controlling terminal read+write. Fails when there is none.
pub fn open_tty() -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open("/dev/tty")
}

/// True when a real controlling terminal is available to drive interactively.
/// `stty -g` only succeeds against an actual tty, so it doubles as an isatty.
pub fn is_interactive() -> bool {
    match File::open("/dev/tty") {
        Ok(f) => Command::new("stty")
            .arg("-g")
            .stdin(f)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Terminal size as (rows, cols); falls back to a sane default.
pub fn term_size() -> (usize, usize) {
    if let Ok(f) = File::open("/dev/tty") {
        if let Ok(out) = Command::new("stty").arg("size").stdin(f).output() {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut it = s.split_whitespace();
            if let (Some(r), Some(c)) = (it.next(), it.next()) {
                if let (Ok(r), Ok(c)) = (r.parse(), c.parse()) {
                    return (r, c);
                }
            }
        }
    }
    (24, 80)
}

/// RAII raw-mode guard: enables raw/-echo on construction, restores the exact
/// prior terminal settings (and shows the cursor) on drop — even on panic.
pub struct RawMode {
    saved: String,
    tty: File,
}

impl RawMode {
    pub fn enable() -> io::Result<RawMode> {
        let probe = File::open("/dev/tty")?;
        let out = Command::new("stty").arg("-g").stdin(probe).output()?;
        if !out.status.success() {
            return Err(io::Error::new(io::ErrorKind::Other, "not a terminal"));
        }
        let saved = String::from_utf8_lossy(&out.stdout).trim().to_string();
        stty(&["raw", "-echo"])?;
        let mut tty = open_tty()?;
        let _ = tty.write_all(b"\x1b[?25l"); // hide cursor
        let _ = tty.flush();
        Ok(RawMode { saved, tty })
    }

    /// Mutable handle to the terminal for reading keys / writing frames.
    pub fn tty(&mut self) -> &mut File {
        &mut self.tty
    }

    /// Read the next key as a raw byte burst (arrows arrive as one read).
    pub fn read_burst(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.tty.read(buf)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = self.tty.write_all(b"\x1b[?25h"); // show cursor
        let _ = self.tty.flush();
        if self.saved.is_empty() {
            let _ = stty(&["sane"]);
        } else {
            let _ = stty(&[self.saved.as_str()]);
        }
    }
}

fn stty(args: &[&str]) -> io::Result<()> {
    let f = File::open("/dev/tty")?;
    let ok = Command::new("stty")
        .args(args)
        .stdin(f)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success();
    if ok {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "stty failed"))
    }
}
