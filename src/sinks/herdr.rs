//! herdr sink: in-muxer notifications via `herdr notification show`.
//! herald is the first client of this herdr capability. Pane *state* is
//! herdr's own business (it paints agent state from its integrations);
//! `report_state` exists as an opt-in only.

use super::{Action, Availability, Sink};
use crate::config::HerdrSink as HerdrConfig;
use crate::context::Context;
use crate::event::{Event, Urgency};

pub struct HerdrSink {
    cfg: HerdrConfig,
}

impl HerdrSink {
    pub fn new(cfg: HerdrConfig) -> Self {
        HerdrSink { cfg }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }
}

impl Sink for HerdrSink {
    fn name(&self) -> String {
        "herdr".into()
    }

    fn available(&self, _ctx: &Context) -> Availability {
        if !self.cfg.enabled {
            return Availability::No("disabled in config".into());
        }
        match super::system::which("herdr") {
            Some(_) => Availability::Yes,
            None => Availability::No("herdr CLI not on PATH".into()),
        }
    }

    fn plan(&self, ev: &Event, _ctx: &Context) -> Vec<Action> {
        let sound = match ev.urgency() {
            Urgency::Critical => "request",
            Urgency::Normal => "done",
            Urgency::Low => "none",
        };
        let mut actions = vec![Action::Spawn {
            argv: vec![
                "herdr".into(),
                "notification".into(),
                "show".into(),
                ev.resolved_title(),
                "--body".into(),
                ev.body.clone(),
                "--sound".into(),
                sound.into(),
            ],
        }];
        if self.cfg.report_state {
            if let Ok(pane_id) = std::env::var("HERDR_PANE_ID") {
                let state = match ev.urgency() {
                    Urgency::Critical => "blocked",
                    _ => "idle",
                };
                actions.push(Action::Spawn {
                    argv: vec![
                        "herdr".into(),
                        "pane".into(),
                        "report-agent".into(),
                        pane_id,
                        "--source".into(),
                        "herald".into(),
                        "--agent".into(),
                        ev.source.clone(),
                        "--state".into(),
                        state.into(),
                        "--message".into(),
                        ev.body.clone(),
                    ],
                });
            }
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Context, Harness};
    use crate::event::EventKind;

    fn ctx() -> Context {
        Context {
            harness: Harness::Herdr,
            terminal_bundle_id: None,
            headless: false,
        }
    }

    #[test]
    fn plan_uses_notification_show_with_urgency_sound() {
        let sink = HerdrSink::new(HerdrConfig { enabled: true, report_state: false });
        let ev = Event::new("claude", EventKind::Attention, "permission needed");
        let actions = sink.plan(&ev, &ctx());
        assert_eq!(actions.len(), 1);
        let Action::Spawn { argv } = &actions[0] else { panic!() };
        assert_eq!(&argv[..3], &["herdr", "notification", "show"]);
        assert!(argv.contains(&"request".to_string()));
    }

    #[test]
    fn no_state_report_by_default() {
        let sink = HerdrSink::new(HerdrConfig::default());
        let ev = Event::new("claude", EventKind::TurnComplete, "done");
        assert_eq!(sink.plan(&ev, &ctx()).len(), 1);
    }
}
