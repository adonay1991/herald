//! Always-on JSONL log at ~/.local/state/herald/events.jsonl — the permanent
//! debug channel (successor of NOTIFY_DEBUG_LOG) and the source for
//! `herald log`. Rotates once past MAX_LOG_BYTES (current → .1, one
//! generation kept) so hook traffic can never grow it unbounded.

use crate::context::Context;
use crate::event::{Event, truncate_body};
use crate::routing::{Decision, RoutePlan};
use crate::sinks::Outcome;
use anyhow::{Context as _, Result};
use serde::Serialize;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct LogEntry<'a> {
    pub ts: String,
    pub source: &'a str,
    pub kind: crate::event::EventKind,
    pub urgency: crate::event::Urgency,
    pub harness: &'static str,
    pub headless: bool,
    pub decision: Decision,
    pub title: String,
    pub body: String,
    pub deliveries: Vec<DeliveryOutcome>,
}

#[derive(Debug, Serialize)]
pub struct DeliveryOutcome {
    pub sink: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub fn append(
    path: &Path,
    ev: &Event,
    ctx: &Context,
    plan: &RoutePlan,
    outcomes: &[(String, Outcome)],
) -> Result<()> {
    append_with_limit(path, ev, ctx, plan, outcomes, MAX_LOG_BYTES)
}

pub fn append_with_limit(
    path: &Path,
    ev: &Event,
    ctx: &Context,
    plan: &RoutePlan,
    outcomes: &[(String, Outcome)],
    max_bytes: u64,
) -> Result<()> {
    let entry = LogEntry {
        ts: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default(),
        source: &ev.source,
        kind: ev.kind,
        urgency: ev.urgency(),
        harness: ctx.harness.name(),
        headless: ctx.headless,
        decision: plan.decision,
        title: ev.resolved_title(),
        body: truncate_body(&ev.body, 120),
        deliveries: outcomes
            .iter()
            .map(|(sink, o)| DeliveryOutcome {
                sink: sink.clone(),
                ok: o.ok,
                backend: o.backend.clone(),
                detail: o.detail.clone(),
            })
            .collect(),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    rotate_if_needed(path, max_bytes);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let line = serde_json::to_string(&entry)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// Best-effort rotation: a failed rename must never block a notification.
fn rotate_if_needed(path: &Path, max_bytes: u64) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() < max_bytes {
        return;
    }
    let mut rotated = path.as_os_str().to_owned();
    rotated.push(".1");
    let _ = std::fs::rename(path, std::path::PathBuf::from(rotated));
}

/// Last JSONL entry, read from the file tail without loading the whole log.
/// Used by the burst-coalescing check on every dispatch.
pub fn last_entry(path: &Path) -> Option<serde_json::Value> {
    const TAIL: i64 = 4096;
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len() as i64;
    let start = (len - TAIL).max(0);
    file.seek(SeekFrom::Start(start as u64)).ok()?;
    let mut tail = String::new();
    file.read_to_string(&mut tail).ok()?;
    let line = tail.lines().rev().find(|l| !l.trim().is_empty())?;
    serde_json::from_str(line).ok()
}

/// Parse an entry's RFC3339 timestamp.
pub fn entry_ts(entry: &serde_json::Value) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(entry["ts"].as_str()?, &Rfc3339).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Context, Harness};
    use crate::event::EventKind;
    use crate::routing::RoutePlan;

    fn fixture() -> (Event, Context, RoutePlan) {
        let ev = Event::new("test", EventKind::Info, "hello");
        let ctx = Context {
            harness: Harness::Plain,
            terminal_bundle_id: None,
            headless: true,
            tmux: false,
        };
        let plan = RoutePlan {
            decision: Decision::SuppressedPolicy,
            deliveries: vec![],
        };
        (ev, ctx, plan)
    }

    #[test]
    fn append_writes_one_json_line_and_last_entry_reads_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let (ev, ctx, plan) = fixture();
        append(&path, &ev, &ctx, &plan, &[]).unwrap();
        append(&path, &ev, &ctx, &plan, &[]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        let last = last_entry(&path).unwrap();
        assert_eq!(last["source"], "test");
        assert!(entry_ts(&last).is_some());
    }

    #[test]
    fn rotates_when_over_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let (ev, ctx, plan) = fixture();
        append_with_limit(&path, &ev, &ctx, &plan, &[], 64).unwrap();
        // second append sees a file over the tiny limit → rotates first
        append_with_limit(&path, &ev, &ctx, &plan, &[], 64).unwrap();
        let rotated = dir.path().join("events.jsonl.1");
        assert!(rotated.exists(), "expected rotation to events.jsonl.1");
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);
    }
}
