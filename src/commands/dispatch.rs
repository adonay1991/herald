//! Shared dispatch path: context → routing → execution → logbook.
//! Every entry point (hook, emit, test) funnels through here so behavior,
//! --dry-run and logging stay identical.

use crate::config::{Config, Policy};
use crate::context::{self, Harness};
use crate::event::Event;
use crate::logbook;
use crate::routing::{self, Decision, RoutePlan};
use crate::sinks::{self, Outcome};
use crate::{config, platform};
use anyhow::Result;
use serde::Serialize;

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

    // The focus probe shells out twice; only pay for it when it can matter.
    let needs_focus = ctx.harness == Harness::Plain
        && !ctx.headless
        && cfg.policy(ev.kind, ev.urgency()) == Policy::Unfocused;
    let frontmost = if needs_focus {
        platform::terminal_is_frontmost(&ctx)
    } else {
        None
    };

    let mut plan = routing::plan(&ev, &ctx, cfg, frontmost);
    if let Some(name) = only_sink {
        plan.deliveries.retain(|d| d.sink == name);
    }

    if dry_run {
        print_plan(&ev, &ctx, &plan, frontmost)?;
        return Ok(Report {
            decision: plan.decision,
            deliveries: plan.deliveries.iter().map(|d| (d.sink.clone(), true, None)).collect(),
        });
    }

    let outcomes: Vec<(String, Outcome)> = plan
        .deliveries
        .iter()
        .map(|d| (d.sink.clone(), sinks::execute(&d.actions)))
        .collect();

    if let Err(err) = logbook::append(&config::log_path(), &ev, &ctx, &plan, &outcomes) {
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
        frontmost: Option<bool>,
        plan: &'a RoutePlan,
    }
    let out = DryRun {
        event: ev,
        resolved_urgency: ev.urgency(),
        resolved_title: ev.resolved_title(),
        harness: ctx.harness.name(),
        headless: ctx.headless,
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
        }
    }
}
