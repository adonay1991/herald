//! Always-on JSONL log at ~/.local/state/herald/events.jsonl — the permanent
//! debug channel (successor of NOTIFY_DEBUG_LOG) and the source for `herald log`.

use crate::context::Context;
use crate::event::{Event, truncate_body};
use crate::routing::{Decision, RoutePlan};
use crate::sinks::Outcome;
use anyhow::{Context as _, Result};
use serde::Serialize;
use std::io::Write as _;
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

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
    let entry = LogEntry {
        ts: OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_default(),
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
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let line = serde_json::to_string(&entry)?;
    writeln!(file, "{line}")?;
    Ok(())
}
