//! Shared dispatch path: context → routing → burst check → execution →
//! logbook. Every entry point (hook, emit, test) funnels through here so
//! behavior, --dry-run and logging stay identical.

use crate::config::{Config, Policy};
use crate::context::{self, Harness};
use crate::event::Event;
use crate::logbook;
use crate::routing::{self, Decision, RoutePlan};
use crate::sinks::{self, Outcome};
use crate::{config, platform};
use anyhow::Result;
use serde::Serialize;
use time::OffsetDateTime;

#[derive(Serialize)]
pub struct Report {
    pub decision: Decision,
    pub deliveries: Vec<(String, bool, Option<String>)>,
}

pub fn dispatch(ev: Event, cfg: &Config, dry_run: bool) -> Result<Report> {
    dispatch_filtered(ev, cfg, dry_run, None)
}

pub fn dispatch_filtered(
    ev: Event,
    cfg: &Config,
    dry_run: bool,
    only_sink: Option<&str>,
) -> Result<Report> {
    let ctx = context::current();

    // The focus probe shells out; only pay for it when it can matter.
    // Inside tmux the terminal-app answer lies (the app can be frontmost
    // while the user sits in another tmux window) → treat as unknown.
    let needs_focus = ctx.harness == Harness::Plain
        && !ctx.headless
        && !ctx.tmux
        && cfg.policy(ev.kind, ev.urgency()) == Policy::Unfocused;
    let frontmost = if needs_focus {
        platform::terminal_is_frontmost(&ctx)
    } else {
        None
    };

    let mut plan = routing::plan(&ev, &ctx, cfg, frontmost, minute_of_day());
    if let Some(name) = only_sink {
        plan.deliveries.retain(|d| d.sink == name);
    }

    if dry_run {
        print_plan(&ev, &ctx, &plan, frontmost)?;
        return Ok(Report {
            decision: plan.decision,
            deliveries: plan
                .deliveries
                .iter()
                .map(|d| (d.sink.clone(), true, None))
                .collect(),
        });
    }

    // Burst coalescing: an identical (source, kind) delivered inside the
    // window means fan-outs and parallel sessions produce one banner, not N.
    let log_path = config::log_path();
    if plan.decision == Decision::Deliver && is_burst_duplicate(&ev, cfg, &log_path) {
        plan = RoutePlan {
            decision: Decision::SuppressedBurst,
            deliveries: vec![],
        };
    }

    let outcomes: Vec<(String, Outcome)> = plan
        .deliveries
        .iter()
        .map(|d| (d.sink.clone(), sinks::execute(&d.actions)))
        .collect();

    if let Err(err) = logbook::append(&log_path, &ev, &ctx, &plan, &outcomes) {
        eprintln!("herald: could not write log: {err:#}");
    }

    Ok(Report {
        decision: plan.decision,
        deliveries: outcomes
            .into_iter()
            .map(|(sink, o)| (sink, o.ok, o.backend))
            .collect(),
    })
}

/// Local minutes since midnight; UTC fallback when the local offset is
/// unavailable (threaded contexts).
fn minute_of_day() -> u16 {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    now.hour() as u16 * 60 + now.minute() as u16
}

fn is_burst_duplicate(ev: &Event, cfg: &Config, log_path: &std::path::Path) -> bool {
    let window_ms = cfg.burst_window_ms();
    if window_ms == 0 {
        return false;
    }
    let Some(last) = logbook::last_entry(log_path) else {
        return false;
    };
    if last["decision"] != "deliver" {
        return false;
    }
    if last["source"] != ev.source.as_str() {
        return false;
    }
    let kind = serde_json::to_value(ev.kind).unwrap_or_default();
    if last["kind"] != kind {
        return false;
    }
    let Some(ts) = logbook::entry_ts(&last) else {
        return false;
    };
    let age = OffsetDateTime::now_utc() - ts;
    age >= time::Duration::ZERO && age < time::Duration::milliseconds(window_ms as i64)
}

fn print_plan(
    ev: &Event,
    ctx: &context::Context,
    plan: &RoutePlan,
    frontmost: Option<bool>,
) -> Result<()> {
    #[derive(Serialize)]
    struct DryRun<'a> {
        event: &'a Event,
        resolved_urgency: crate::event::Urgency,
        resolved_title: String,
        harness: &'static str,
        headless: bool,
        tmux: bool,
        frontmost: Option<bool>,
        plan: &'a RoutePlan,
    }
    let out = DryRun {
        event: ev,
        resolved_urgency: ev.urgency(),
        resolved_title: ev.resolved_title(),
        harness: ctx.harness.name(),
        headless: ctx.headless,
        tmux: ctx.tmux,
        frontmost,
        plan,
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

impl Report {
    pub fn summary(&self) -> String {
        match self.decision {
            Decision::Deliver => {
                let parts: Vec<String> = self
                    .deliveries
                    .iter()
                    .map(|(sink, ok, backend)| {
                        let via = backend.as_deref().unwrap_or("-");
                        let status = if *ok { "ok" } else { "FAILED" };
                        format!("{sink} via {via}: {status}")
                    })
                    .collect();
                format!("delivered — {}", parts.join(", "))
            }
            Decision::SuppressedPolicy => "suppressed (policy: log only)".into(),
            Decision::SuppressedFocus => "suppressed (terminal is frontmost)".into(),
            Decision::SuppressedHarness => "suppressed (harness paints state)".into(),
            Decision::SuppressedQuiet => "suppressed (quiet hours)".into(),
            Decision::SuppressedBurst => {
                "suppressed (burst: identical event just delivered)".into()
            }
        }
    }
}
