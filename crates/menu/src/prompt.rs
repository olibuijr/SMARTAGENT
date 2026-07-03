//! Line input, masked (password) input, and yes/no confirm — with numbered/
//! line fallbacks for non-interactive terminals.

use std::io::{self, BufRead, Write};

use crate::key::{parse, Key};
use crate::render::{self, Block};
use crate::tty::{is_interactive, RawMode};

/// Read a line of text. `mask` hides the input (for secrets). Returns None on
/// cancel (Esc/Ctrl-C). A blank submit with a `default` returns the default.
pub fn input(title: &str, default: Option<&str>, mask: bool) -> io::Result<Option<String>> {
    if is_interactive() {
        interactive_input(title, default, mask)
    } else {
        line_fallback(title, default, mask)
    }
}

fn interactive_input(
    title: &str,
    default: Option<&str>,
    mask: bool,
) -> io::Result<Option<String>> {
    let mut raw = RawMode::enable()?;
    let mut block = Block::new();
    let mut buf = String::new();
    let mut read = [0u8; 16];

    loop {
        let shown = if mask {
            "•".repeat(buf.chars().count())
        } else {
            buf.clone()
        };
        let hint = match (buf.is_empty(), default) {
            (true, Some(d)) if !mask => format!("{}[{d}]{} ", render::DIM, render::RESET),
            _ => String::new(),
        };
        let line = format!(
            "{}{}{} {hint}{}{}{}▏",
            render::BOLD,
            title,
            render::RESET,
            render::CYAN,
            shown,
            render::RESET
        );
        block.draw(raw.tty(), &[line])?;

        let n = raw.read_burst(&mut read)?;
        if n == 0 {
            continue;
        }
        match parse(&read[..n]) {
            Key::Enter => {
                block.clear(raw.tty())?;
                let val = if buf.is_empty() {
                    default.unwrap_or("").to_string()
                } else {
                    buf.clone()
                };
                let display = if mask { "••••••••".to_string() } else { val.clone() };
                writeln!(
                    raw.tty(),
                    "{}✔{} {}: {}",
                    render::GREEN,
                    render::RESET,
                    title,
                    render::truncate(&display, 60)
                )?;
                return Ok(Some(val));
            }
            Key::Esc | Key::CtrlC => {
                block.clear(raw.tty())?;
                return Ok(None);
            }
            Key::Backspace => {
                buf.pop();
            }
            Key::CtrlU => buf.clear(),
            Key::Space => buf.push(' '),
            Key::Char(c) => buf.push(c),
            _ => {}
        }
    }
}

fn line_fallback(title: &str, default: Option<&str>, mask: bool) -> io::Result<Option<String>> {
    let stderr = io::stderr();
    let mut e = stderr.lock();
    match default {
        Some(d) if !mask => write!(e, "{title} [{d}]: ")?,
        _ => write!(e, "{title}: ")?,
    }
    e.flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let line = line.trim_end_matches(['\n', '\r']);
    if line.is_empty() {
        match default {
            Some(d) => Ok(Some(d.to_string())),
            None => Ok(Some(String::new())),
        }
    } else {
        Ok(Some(line.to_string()))
    }
}

/// Yes/no confirm. Returns None on cancel.
pub fn confirm(title: &str, default_yes: bool) -> io::Result<Option<bool>> {
    if !is_interactive() {
        let stderr = io::stderr();
        let mut e = stderr.lock();
        let d = if default_yes { "Y/n" } else { "y/N" };
        write!(e, "{title} [{d}]: ")?;
        e.flush()?;
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        return Ok(Some(match line.trim().to_lowercase().as_str() {
            "y" | "yes" => true,
            "n" | "no" => false,
            "" => default_yes,
            _ => default_yes,
        }));
    }

    let mut raw = RawMode::enable()?;
    let mut block = Block::new();
    let d = if default_yes { "Y/n" } else { "y/N" };
    let line = format!(
        "{}{}{} {}[{d}]{}",
        render::BOLD,
        title,
        render::RESET,
        render::DIM,
        render::RESET
    );
    block.draw(raw.tty(), &[line])?;
    let mut read = [0u8; 8];
    loop {
        let n = raw.read_burst(&mut read)?;
        if n == 0 {
            continue;
        }
        let ans = match parse(&read[..n]) {
            Key::Char('y') | Key::Char('Y') => Some(true),
            Key::Char('n') | Key::Char('N') => Some(false),
            Key::Enter => Some(default_yes),
            Key::Esc | Key::CtrlC => {
                block.clear(raw.tty())?;
                return Ok(None);
            }
            _ => None,
        };
        if let Some(v) = ans {
            block.clear(raw.tty())?;
            writeln!(
                raw.tty(),
                "{}✔{} {}: {}",
                render::GREEN,
                render::RESET,
                title,
                if v { "yes" } else { "no" }
            )?;
            return Ok(Some(v));
        }
    }
}
