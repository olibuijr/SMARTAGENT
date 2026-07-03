//! ANSI rendering for the interactive menu block.

use std::io::{self, Write};

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const CYAN: &str = "\x1b[36m";
pub const GREEN: &str = "\x1b[32m";
pub const REVERSE: &str = "\x1b[7m";

/// Truncate a plain string to `max` columns, adding an ellipsis if cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// Redraws a block of lines in place: moves up over the previously drawn block,
/// clears each line, writes the new content, then erases anything left below.
pub struct Block {
    prev: usize,
}

impl Block {
    pub fn new() -> Block {
        Block { prev: 0 }
    }

    pub fn draw<W: Write>(&mut self, w: &mut W, lines: &[String]) -> io::Result<()> {
        if self.prev > 0 {
            write!(w, "\x1b[{}A", self.prev)?; // cursor to top of prior block
        }
        for line in lines {
            write!(w, "\r\x1b[2K{line}\r\n")?; // CR + clear line + content
        }
        write!(w, "\x1b[0J")?; // erase any stale lines below
        self.prev = lines.len();
        w.flush()
    }

    /// Tear the block down (used on cancel) leaving the cursor where it started.
    pub fn clear<W: Write>(&mut self, w: &mut W) -> io::Result<()> {
        if self.prev > 0 {
            write!(w, "\x1b[{}A", self.prev)?;
            write!(w, "\x1b[0J")?;
        }
        self.prev = 0;
        w.flush()
    }
}
