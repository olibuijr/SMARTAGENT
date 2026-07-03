//! `menu` — interactive terminal menus for shell scripts.
//!
//!   menu select      --title T [--] item...     print chosen item (stdin if none)
//!   menu multiselect --title T [--] item...     print chosen items, newline-sep
//!   menu input       --title T [--default D]    print entered line
//!   menu password    --title T                  print entered secret (no echo)
//!   menu confirm     --title T [--default yes|no]   exit 0=yes 1=no; prints yes/no
//!
//! Cancel (Esc/Ctrl-C) exits 130 with no stdout. The interactive UI renders to
//! /dev/tty, so `x=$(menu select --title Pick -- a b c)` captures only the pick.

use std::io::{self, BufRead, Write};
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        usage();
        exit(if args.is_empty() { 2 } else { 0 });
    }
    let cmd = args[0].clone();
    let (title, default, items) = parse(&args[1..]);

    let code = match cmd.as_str() {
        "select" => run_select(&title, load_items(items), false),
        "multiselect" => run_select(&title, load_items(items), true),
        "input" => run_input(&title, default.as_deref(), false),
        "password" => run_input(&title, None, true),
        "confirm" => run_confirm(&title, default.as_deref()),
        other => {
            eprintln!("menu: unknown command '{other}' (see --help)");
            2
        }
    };
    exit(code);
}

/// Parse `--title`, `--default`, and trailing items (after `--` or bare).
fn parse(args: &[String]) -> (String, Option<String>, Vec<String>) {
    let mut title = String::new();
    let mut default = None;
    let mut items = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--title" | "-t" => {
                if i + 1 < args.len() {
                    title = args[i + 1].clone();
                    i += 2;
                    continue;
                }
            }
            "--default" | "-d" => {
                if i + 1 < args.len() {
                    default = Some(args[i + 1].clone());
                    i += 2;
                    continue;
                }
            }
            "--" => {
                items.extend_from_slice(&args[i + 1..]);
                break;
            }
            other => items.push(other.to_string()),
        }
        i += 1;
    }
    if title.is_empty() {
        title = "select".to_string();
    }
    (title, default, items)
}

/// Use CLI items, or read newline-separated items from stdin when none given.
fn load_items(items: Vec<String>) -> Vec<String> {
    if !items.is_empty() {
        return items;
    }
    let mut out = Vec::new();
    for line in io::stdin().lock().lines().map_while(Result::ok) {
        let t = line.trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
    }
    out
}

fn run_select(title: &str, items: Vec<String>, multi: bool) -> i32 {
    if items.is_empty() {
        eprintln!("menu: no items to select");
        return 2;
    }
    let res = if multi {
        menu::multiselect(title, &items)
    } else {
        menu::select(title, &items).map(|o| o.map(|i| vec![i]))
    };
    match res {
        Ok(Some(idxs)) => {
            let mut out = io::stdout().lock();
            for i in idxs {
                let _ = writeln!(out, "{}", items[i]);
            }
            0
        }
        Ok(None) => 130,
        Err(e) => {
            eprintln!("menu: {e}");
            1
        }
    }
}

fn run_input(title: &str, default: Option<&str>, mask: bool) -> i32 {
    let res = if mask {
        menu::password(title)
    } else {
        menu::input(title, default)
    };
    match res {
        Ok(Some(v)) => {
            println!("{v}");
            0
        }
        Ok(None) => 130,
        Err(e) => {
            eprintln!("menu: {e}");
            1
        }
    }
}

fn run_confirm(title: &str, default: Option<&str>) -> i32 {
    let default_yes = matches!(default, Some(d) if matches!(d.to_lowercase().as_str(), "yes" | "y" | "true"));
    match menu::confirm(title, default_yes) {
        Ok(Some(true)) => {
            println!("yes");
            0
        }
        Ok(Some(false)) => {
            println!("no");
            1
        }
        Ok(None) => 130,
        Err(e) => {
            eprintln!("menu: {e}");
            2
        }
    }
}

fn usage() {
    eprintln!(
        "menu — interactive terminal menus (std-only)\n\n\
  menu select      --title T [--] item...     print chosen item (stdin if none)\n\
  menu multiselect --title T [--] item...     print chosen items, newline-sep\n\
  menu input       --title T [--default D]    print entered line\n\
  menu password    --title T                  print entered secret (no echo)\n\
  menu confirm     --title T [--default yes|no]   exit 0=yes 1=no\n\n\
Cancel (Esc/Ctrl-C) exits 130. Non-interactive terminals get a numbered/line\n\
fallback reading from stdin."
    );
}
