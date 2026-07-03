//! Interactive single/multi select with a live fuzzy filter, plus a numbered
//! fallback for non-interactive terminals.

use std::io::{self, BufRead, Write};

use crate::fuzzy;
use crate::key::{parse, Key};
use crate::render::{self, Block};
use crate::tty::{is_interactive, term_size, RawMode};

/// Outcome of a selection: the chosen original indices, or None if cancelled.
pub type Choice = Option<Vec<usize>>;

pub fn select(title: &str, items: &[String], multi: bool) -> io::Result<Choice> {
    if items.is_empty() {
        return Ok(None);
    }
    if is_interactive() {
        interactive(title, items, multi)
    } else {
        numbered(title, items, multi)
    }
}

fn interactive(title: &str, items: &[String], multi: bool) -> io::Result<Choice> {
    let mut raw = RawMode::enable()?;
    let (rows, cols) = term_size();
    let cols = cols.max(20);
    let max_visible = rows.saturating_sub(4).clamp(3, 20);

    let mut query = String::new();
    let mut cursor = 0usize; // index into the filtered list
    let mut top = 0usize; // scroll offset into the filtered list
    let mut selected = vec![false; items.len()];
    let mut block = Block::new();
    let mut buf = [0u8; 16];

    loop {
        let filtered = fuzzy::filter(items, &query);
        if cursor >= filtered.len() {
            cursor = filtered.len().saturating_sub(1);
        }
        if cursor < top {
            top = cursor;
        } else if cursor >= top + max_visible {
            top = cursor + 1 - max_visible;
        }

        let mut lines = Vec::new();
        lines.push(header(title, &query, cols));
        let end = (top + max_visible).min(filtered.len());
        for (row, &orig) in filtered[top..end].iter().enumerate() {
            let idx = top + row;
            lines.push(row_line(
                &items[orig],
                idx == cursor,
                multi && selected[orig],
                multi,
                cols,
            ));
        }
        if filtered.is_empty() {
            lines.push(format!("  {}(no matches){}", render::DIM, render::RESET));
        }
        lines.push(footer(multi, filtered.len(), items.len()));
        block.draw(raw.tty(), &lines)?;

        let n = raw.read_burst(&mut buf)?;
        if n == 0 {
            continue;
        }
        match parse(&buf[..n]) {
            Key::Up | Key::CtrlP => cursor = cursor.saturating_sub(1),
            Key::Down | Key::CtrlN => {
                if cursor + 1 < filtered.len() {
                    cursor += 1;
                }
            }
            Key::PageUp | Key::Home => cursor = 0,
            Key::PageDown | Key::End => cursor = filtered.len().saturating_sub(1),
            Key::Space if multi => {
                if let Some(&orig) = filtered.get(cursor) {
                    selected[orig] = !selected[orig];
                }
            }
            Key::Char(c) => {
                query.push(c);
                cursor = 0;
                top = 0;
            }
            Key::Backspace => {
                query.pop();
                cursor = 0;
                top = 0;
            }
            Key::CtrlU => {
                query.clear();
                cursor = 0;
                top = 0;
            }
            Key::Enter => {
                let out: Vec<usize> = if multi {
                    let picked: Vec<usize> = (0..items.len()).filter(|&i| selected[i]).collect();
                    if picked.is_empty() {
                        filtered.get(cursor).copied().into_iter().collect()
                    } else {
                        picked
                    }
                } else {
                    match filtered.get(cursor) {
                        Some(&orig) => vec![orig],
                        None => Vec::new(),
                    }
                };
                block.clear(raw.tty())?;
                if out.is_empty() {
                    return Ok(None);
                }
                summary(raw.tty(), title, items, &out)?;
                return Ok(Some(out));
            }
            Key::Esc | Key::CtrlC => {
                block.clear(raw.tty())?;
                return Ok(None);
            }
            _ => {}
        }
    }
}

fn header(title: &str, query: &str, cols: usize) -> String {
    let q = if query.is_empty() {
        format!("{}type to filter{}", render::DIM, render::RESET)
    } else {
        format!("{}{query}{}", render::CYAN, render::RESET)
    };
    let t = render::truncate(title, cols.saturating_sub(4));
    format!("{}{}{} {}", render::BOLD, t, render::RESET, q)
}

fn row_line(label: &str, is_cursor: bool, checked: bool, multi: bool, cols: usize) -> String {
    let marker = if is_cursor { "❯" } else { " " };
    let box_ = if multi {
        if checked {
            format!("{}◉{} ", render::GREEN, render::RESET)
        } else {
            "◯ ".to_string()
        }
    } else {
        String::new()
    };
    let text = render::truncate(label, cols.saturating_sub(6));
    if is_cursor {
        format!(
            " {}{} {}{}{}{}",
            render::CYAN,
            marker,
            render::REVERSE,
            box_,
            text,
            render::RESET
        )
    } else {
        format!(" {marker} {box_}{text}")
    }
}

fn footer(multi: bool, shown: usize, total: usize) -> String {
    let keys = if multi {
        "↑↓ move · space select · enter confirm · esc cancel"
    } else {
        "↑↓ move · enter select · esc cancel"
    };
    format!("{}{keys}  [{shown}/{total}]{}", render::DIM, render::RESET)
}

fn summary<W: Write>(w: &mut W, title: &str, items: &[String], picked: &[usize]) -> io::Result<()> {
    let names: Vec<&str> = picked.iter().map(|&i| items[i].as_str()).collect();
    let joined = names.join(", ");
    writeln!(
        w,
        "{}✔{} {}: {}",
        render::GREEN,
        render::RESET,
        title,
        render::truncate(&joined, 70)
    )
}

/// Numbered prompt for non-interactive terminals. Reads a line from stdin.
fn numbered(title: &str, items: &[String], multi: bool) -> io::Result<Choice> {
    let stderr = io::stderr();
    let mut e = stderr.lock();
    writeln!(e, "{title}")?;
    for (i, it) in items.iter().enumerate() {
        writeln!(e, "  {:>2}) {}", i + 1, it)?;
    }
    if multi {
        write!(e, "select (comma-separated numbers, blank=cancel): ")?;
    } else {
        write!(e, "select (number, blank=cancel): ")?;
    }
    e.flush()?;

    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    for tok in line.split(',') {
        if let Ok(n) = tok.trim().parse::<usize>() {
            if n >= 1 && n <= items.len() {
                out.push(n - 1);
            }
        }
        if !multi {
            break;
        }
    }
    if out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}
