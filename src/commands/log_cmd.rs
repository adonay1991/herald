//! `herald log` — read the JSONL logbook, compact by default, raw on demand,
//! with a simple polling -f mode.

use crate::cli::LogArgs;
use crate::config;
use anyhow::{Context as _, Result};
use serde_json::Value;
use std::io::{Read as _, Seek as _, SeekFrom};

pub fn run(args: LogArgs) -> Result<()> {
    let path = config::log_path();
    if !path.exists() {
        println!("(no events logged yet at {})", path.display());
        return Ok(());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(args.lines);
    for line in &lines[start..] {
        print_line(line, args.raw);
    }

    if args.follow {
        follow(&path, text.len() as u64, args.raw)?;
    }
    Ok(())
}

fn follow(path: &std::path::Path, mut offset: u64, raw: bool) -> Result<()> {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len < offset {
            offset = 0; // truncated/rotated
        }
        if len > offset {
            file.seek(SeekFrom::Start(offset))?;
            let mut chunk = String::new();
            file.read_to_string(&mut chunk)?;
            offset = len;
            for line in chunk.lines().filter(|l| !l.trim().is_empty()) {
                print_line(line, raw);
            }
        }
    }
}

fn print_line(line: &str, raw: bool) {
    if raw {
        println!("{line}");
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        println!("{line}");
        return;
    };
    let deliveries = v["deliveries"]
        .as_array()
        .map(|ds| {
            ds.iter()
                .map(|d| {
                    format!(
                        "{}{}{}",
                        d["sink"].as_str().unwrap_or("?"),
                        d["backend"].as_str().map(|b| format!("/{b}")).unwrap_or_default(),
                        if d["ok"].as_bool() == Some(true) { "" } else { " FAILED" },
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "-".into());
    println!(
        "{}  {:13} {:12} [{}] {} → {}  {}",
        v["ts"].as_str().unwrap_or("-"),
        v["kind"].as_str().unwrap_or("?"),
        v["source"].as_str().unwrap_or("?"),
        v["harness"].as_str().unwrap_or("?"),
        v["decision"].as_str().unwrap_or("?"),
        deliveries,
        v["title"].as_str().unwrap_or(""),
    );
}
