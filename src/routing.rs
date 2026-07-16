//! Routing: a pure function from (event, context, config, focus, clock) to a
//! delivery plan. The whole default policy is three rows:
//!   Critical → always notify · Normal → notify when unfocused · Low → log only
//! plus two structural rules: a harness with its own notification UI owns the
//! pane and suppresses the system banner, and quiet hours silence everything
//! below critical.

use crate::config::{Config, Policy};
use crate::context::{Context, Harness};
use crate::event::{Event, Urgency};
use crate::sinks::cmux::CmuxSink;
use crate::sinks::exec::ExecSink;
use crate::sinks::herdr::HerdrSink;
use crate::sinks::osc::OscSink;
use crate::sinks::system::SystemSink;
use crate::sinks::{Action, Sink};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    Deliver,
    /// Policy for this kind/urgency says log-only.
    SuppressedPolicy,
    /// The emitting terminal is frontmost; the human is already looking.
    SuppressedFocus,
    /// A harness owns this pane and covers the event through its own UI
    /// (orca pane state), so no banner.
    SuppressedHarness,
    /// Inside configured quiet hours and below critical.
    SuppressedQuiet,
    /// Identical (source, kind) delivered moments ago; coalesced.
    /// Emitted by the dispatcher's burst check, never by routing itself.
    SuppressedBurst,
}

#[derive(Debug, Serialize)]
pub struct Delivery {
    pub sink: String,
    pub actions: Vec<Action>,
}

#[derive(Debug, Serialize)]
pub struct RoutePlan {
    pub decision: Decision,
    pub deliveries: Vec<Delivery>,
}

/// `now_minute_of_day` is local minutes since midnight, injected so the
/// function stays pure (and quiet-hours testable).
pub fn plan(
    ev: &Event,
    ctx: &Context,
    cfg: &Config,
    frontmost: Option<bool>,
    now_minute_of_day: u16,
) -> RoutePlan {
    let policy = cfg.policy(ev.kind, ev.urgency());
    if policy == Policy::Never {
        return RoutePlan {
            decision: Decision::SuppressedPolicy,
            deliveries: vec![],
        };
    }
    if ev.urgency() != Urgency::Critical
        && let Some(quiet) = cfg.quiet_hours()
        && quiet.contains(now_minute_of_day)
    {
        return RoutePlan {
            decision: Decision::SuppressedQuiet,
            deliveries: vec![],
        };
    }

    let mut deliveries = Vec::new();
    let mut exclusive_taken = false;

    // Exclusive exec sinks act as a harness regardless of the built-in detection.
    for exec_cfg in &cfg.sinks.exec {
        let sink = ExecSink::new(exec_cfg.clone());
        if sink.active() && sink.accepts(ev) {
            if sink.exclusive() {
                exclusive_taken = true;
            }
            deliveries.push(Delivery {
                sink: sink.name(),
                actions: sink.plan(ev, ctx),
            });
        }
    }

    // Opt-in OSC delivery: only meaningful in a plain terminal (muxers own
    // their panes) with a controlling tty.
    if cfg.sinks.osc.enabled && ctx.harness == Harness::Plain && !ctx.headless {
        let sink = OscSink::new(cfg.sinks.osc.clone());
        if cfg.sinks.osc.exclusive {
            exclusive_taken = true;
        }
        deliveries.push(Delivery {
            sink: sink.name(),
            actions: sink.plan(ev, ctx),
        });
    }

    let decision = match ctx.harness {
        Harness::Herdr if cfg.sinks.herdr.enabled => {
            let sink = HerdrSink::new(cfg.sinks.herdr.clone());
            deliveries.push(Delivery {
                sink: sink.name(),
                actions: sink.plan(ev, ctx),
            });
            Decision::Deliver
        }
        Harness::Cmux if cfg.sinks.cmux.enabled => {
            let sink = CmuxSink::new(cfg.sinks.cmux.clone());
            deliveries.push(Delivery {
                sink: sink.name(),
                actions: sink.plan(ev, ctx),
            });
            Decision::Deliver
        }
        // orca paints pane state through its own hook channel. Only
        // must-act-now events earn a system banner; the rest would be
        // duplicate noise (this fixes the historical double-notification).
        Harness::Orca if policy != Policy::Always => Decision::SuppressedHarness,
        _ if exclusive_taken => Decision::Deliver,
        _ => {
            // Plain terminal (or harness without its sink): system banner
            // gated on focus for Unfocused policy.
            let suppressed_by_focus =
                policy == Policy::Unfocused && !ctx.headless && frontmost == Some(true);
            if suppressed_by_focus {
                Decision::SuppressedFocus
            } else {
                let sink = SystemSink::from_config(cfg);
                deliveries.push(Delivery {
                    sink: sink.name(),
                    actions: sink.plan(ev, ctx),
                });
                Decision::Deliver
            }
        }
    };

    if decision != Decision::Deliver && !deliveries.is_empty() {
        // Non-exclusive exec/osc sinks still deliver even when the system
        // banner is suppressed; the decision reported is Deliver in that case.
        return RoutePlan {
            decision: Decision::Deliver,
            deliveries,
        };
    }
    RoutePlan {
        decision,
        deliveries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Harness;
    use crate::event::{Event, EventKind};

    const NOON: u16 = 12 * 60;

    fn ctx(harness: Harness, headless: bool) -> Context {
        Context {
            harness,
            terminal_bundle_id: if headless {
                None
            } else {
                Some("com.apple.Terminal".into())
            },
            headless,
            tmux: false,
        }
    }

    fn sinks_of(plan: &RoutePlan) -> Vec<&str> {
        plan.deliveries.iter().map(|d| d.sink.as_str()).collect()
    }

    #[test]
    fn attention_in_plain_terminal_always_notifies_even_focused() {
        let ev = Event::new("claude", EventKind::Attention, "permission");
        let plan = plan(
            &ev,
            &ctx(Harness::Plain, false),
            &Config::default(),
            Some(true),
            NOON,
        );
        assert_eq!(plan.decision, Decision::Deliver);
        assert_eq!(sinks_of(&plan), vec!["system"]);
    }

    #[test]
    fn turn_complete_suppressed_when_frontmost() {
        let ev = Event::new("claude", EventKind::TurnComplete, "done");
        let plan = plan(
            &ev,
            &ctx(Harness::Plain, false),
            &Config::default(),
            Some(true),
            NOON,
        );
        assert_eq!(plan.decision, Decision::SuppressedFocus);
        assert!(plan.deliveries.is_empty());
    }

    #[test]
    fn turn_complete_delivers_when_unfocused_or_unknown() {
        let ev = Event::new("claude", EventKind::TurnComplete, "done");
        for focus in [Some(false), None] {
            let plan = plan(
                &ev,
                &ctx(Harness::Plain, false),
                &Config::default(),
                focus,
                NOON,
            );
            assert_eq!(plan.decision, Decision::Deliver, "focus={focus:?}");
        }
    }

    #[test]
    fn headless_always_delivers_normal_events() {
        let ev = Event::new("cron:backup", EventKind::TurnComplete, "done");
        let plan = plan(
            &ev,
            &ctx(Harness::Plain, true),
            &Config::default(),
            None,
            NOON,
        );
        assert_eq!(plan.decision, Decision::Deliver);
    }

    #[test]
    fn herdr_owns_the_pane() {
        let ev = Event::new("claude", EventKind::Attention, "permission");
        let plan = plan(
            &ev,
            &ctx(Harness::Herdr, false),
            &Config::default(),
            None,
            NOON,
        );
        assert_eq!(sinks_of(&plan), vec!["herdr"]);
    }

    #[test]
    fn cmux_owns_the_pane() {
        let ev = Event::new("codex", EventKind::TurnComplete, "done");
        let plan = plan(
            &ev,
            &ctx(Harness::Cmux, false),
            &Config::default(),
            None,
            NOON,
        );
        assert_eq!(sinks_of(&plan), vec!["cmux"]);
    }

    #[test]
    fn orca_gets_banner_only_for_actionable() {
        let attention = Event::new("claude", EventKind::Attention, "permission");
        let plan_a = plan(
            &attention,
            &ctx(Harness::Orca, false),
            &Config::default(),
            None,
            NOON,
        );
        assert_eq!(plan_a.decision, Decision::Deliver);
        assert_eq!(sinks_of(&plan_a), vec!["system"]);

        let stop = Event::new("claude", EventKind::TurnComplete, "done");
        let plan_s = plan(
            &stop,
            &ctx(Harness::Orca, false),
            &Config::default(),
            None,
            NOON,
        );
        assert_eq!(plan_s.decision, Decision::SuppressedHarness);
        assert!(plan_s.deliveries.is_empty());
    }

    #[test]
    fn low_urgency_is_log_only_everywhere() {
        let ev = Event::new("claude", EventKind::Info, "fyi");
        for harness in [Harness::Plain, Harness::Herdr, Harness::Cmux, Harness::Orca] {
            let plan = plan(&ev, &ctx(harness, false), &Config::default(), None, NOON);
            assert_eq!(plan.decision, Decision::SuppressedPolicy, "{harness:?}");
        }
    }

    #[test]
    fn disabled_herdr_sink_falls_through_to_system() {
        let cfg: Config =
            toml::from_str("[sinks.herdr]\nenabled = false\nreport_state = false\n").unwrap();
        let ev = Event::new("claude", EventKind::Attention, "permission");
        let plan = plan(&ev, &ctx(Harness::Herdr, false), &cfg, None, NOON);
        assert_eq!(sinks_of(&plan), vec!["system"]);
    }

    #[test]
    fn config_override_can_silence_turn_complete() {
        let cfg: Config = toml::from_str("[routing]\nturn-complete = \"never\"\n").unwrap();
        let ev = Event::new("claude", EventKind::TurnComplete, "done");
        let plan = plan(&ev, &ctx(Harness::Plain, false), &cfg, Some(false), NOON);
        assert_eq!(plan.decision, Decision::SuppressedPolicy);
    }

    #[test]
    fn quiet_hours_silence_normal_but_not_critical() {
        let cfg: Config = toml::from_str("[routing]\nquiet-hours = \"23:00-08:00\"\n").unwrap();
        let night: u16 = 23 * 60 + 30;
        let stop = Event::new("claude", EventKind::TurnComplete, "done");
        let plan_s = plan(&stop, &ctx(Harness::Plain, false), &cfg, Some(false), night);
        assert_eq!(plan_s.decision, Decision::SuppressedQuiet);

        let attention = Event::new("claude", EventKind::Attention, "permission");
        let plan_a = plan(
            &attention,
            &ctx(Harness::Plain, false),
            &cfg,
            Some(false),
            night,
        );
        assert_eq!(plan_a.decision, Decision::Deliver);

        // outside the window everything is back to normal
        let plan_day = plan(&stop, &ctx(Harness::Plain, false), &cfg, Some(false), NOON);
        assert_eq!(plan_day.decision, Decision::Deliver);
    }

    #[test]
    fn osc_enabled_delivers_in_plain_terminal_only() {
        let cfg: Config = toml::from_str("[sinks.osc]\nenabled = true\n").unwrap();
        let ev = Event::new("claude", EventKind::Attention, "permission");
        let plan_plain = plan(&ev, &ctx(Harness::Plain, false), &cfg, None, NOON);
        assert_eq!(sinks_of(&plan_plain), vec!["osc", "system"]);

        let plan_herdr = plan(&ev, &ctx(Harness::Herdr, false), &cfg, None, NOON);
        assert_eq!(sinks_of(&plan_herdr), vec!["herdr"]);

        let plan_headless = plan(&ev, &ctx(Harness::Plain, true), &cfg, None, NOON);
        assert_eq!(sinks_of(&plan_headless), vec!["system"]);
    }

    #[test]
    fn exclusive_osc_replaces_system_banner() {
        let cfg: Config =
            toml::from_str("[sinks.osc]\nenabled = true\nexclusive = true\n").unwrap();
        let ev = Event::new("claude", EventKind::Attention, "permission");
        let plan = plan(&ev, &ctx(Harness::Plain, false), &cfg, None, NOON);
        assert_eq!(sinks_of(&plan), vec!["osc"]);
        assert_eq!(plan.decision, Decision::Deliver);
    }
}
