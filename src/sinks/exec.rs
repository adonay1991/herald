//! Third-party terminal integration: a declarative [[sinks.exec]] entry in
//! config runs an arbitrary command with the canonical Event JSON on stdin
//! whenever `when_env` is present in the environment. This is the same
//! contract the built-in sinks implement with optimized transports.
//! See docs/SINKS.md.

use super::{Action, Availability, Sink};
use crate::config::ExecSink as ExecConfig;
use crate::context::Context;
use crate::event::Event;

pub struct ExecSink {
    cfg: ExecConfig,
}

impl ExecSink {
    pub fn new(cfg: ExecConfig) -> Self {
        ExecSink { cfg }
    }

    pub fn active(&self) -> bool {
        !self.cfg.command.is_empty()
            && std::env::var_os(&self.cfg.when_env).is_some_and(|v| !v.is_empty())
    }

    pub fn exclusive(&self) -> bool {
        self.cfg.exclusive
    }

    pub fn accepts(&self, ev: &Event) -> bool {
        ev.urgency() >= self.cfg.min_urgency
    }
}

impl Sink for ExecSink {
    fn name(&self) -> String {
        format!("exec:{}", self.cfg.name)
    }

    fn available(&self, _ctx: &Context) -> Availability {
        if self.active() {
            Availability::Yes
        } else {
            Availability::No(format!("{} not set in environment", self.cfg.when_env))
        }
    }

    fn plan(&self, ev: &Event, _ctx: &Context) -> Vec<Action> {
        let stdin = serde_json::to_string(ev).unwrap_or_else(|_| "{}".into());
        vec![Action::SpawnStdin { argv: self.cfg.command.clone(), stdin }]
    }
}
