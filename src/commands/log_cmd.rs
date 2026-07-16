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
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();

    if args.stats {
        print_stats(&lines);
        return Ok(());
    }

    let start = lines.len().saturating_sub(args.lines);
    for line in &lines[start..] {
        print_line(line, args.raw);
    }

    if args.follow {
        follow(&path, text.len() as u64, args.raw)?;
    }
    Ok(())
}

/// Aggregate view over the current log generation (rotated .1 not included).
fn print_stats(lines: &[&str]) {
    use std::collections::BTreeMap;
    let mut by_decision: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_source: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_sink: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = 0usize;
    let mut total = 0usize;

    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        total += 1;
        let bump = |map: &mut BTreeMap<String, usize>, key: Option<&str>| {
            *map.entry(key.unwrap_or("?").to_string()).or_default() += 1;
        };
        bump(&mut by_decision, v["decision"].as_str());
        bump(&mut by_kind, v["kind"].as_str());
        bump(&mut by_source, v["source"].as_str());
        if let Some(ds) = v["deliveries"].as_array() {
            for d in ds {
                bump(&mut by_sink, d["sink"].as_str());
                if d["ok"].as_bool() == Some(false) {
                    failures += 1;
                }
            }
        }
    }

    let section = |title: &str, map: &BTreeMap<String, usize>| {
        println!("{title}");
        let mut rows: Vec<(&String, &usize)> = map.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        for (key, count) in rows {
            println!("  {count:>6}  {key}");
        }
    };

    println!("events: {total} (window: current log generation)");
    println!();
    section("by decision", &by_decision);
    println!();
    section("by kind", &by_kind);
    println!();
    section("by source", &by_source);
    println!();
    section("by sink", &by_sink);
    if failures > 0 {
        println!();
        println!("delivery failures: {failures}");
    }
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
                        d["backend"]
                            .as_str()
                            .map(|b| format!("/{b}"))
                            .unwrap_or_default(),
                        if d["ok"].as_bool() == Some(true) {
                            ""
                        } else {
                            " FAILED"
                        },
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
