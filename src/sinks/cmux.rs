//! cmux sink: in-muxer notifications via `cmux notify`. cmux decides
//! presentation (badge/toast); herald just hands over title and body.

use super::{Action, Availability, Sink};
use crate::config::CmuxSink as CmuxConfig;
use crate::context::Context;
use crate::event::Event;

pub struct CmuxSink {
    cfg: CmuxConfig,
}

impl CmuxSink {
    pub fn new(cfg: CmuxConfig) -> Self {
        CmuxSink { cfg }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }
}

impl Sink for CmuxSink {
    fn name(&self) -> String {
        "cmux".into()
    }

    fn available(&self, _ctx: &Context) -> Availability {
        if !self.cfg.enabled {
            return Availability::No("disabled in config".into());
        }
        match super::system::which("cmux") {
            Some(_) => Availability::Yes,
            None => Availability::No("cmux CLI not on PATH".into()),
        }
    }

    fn plan(&self, ev: &Event, _ctx: &Context) -> Vec<Action> {
        vec![Action::Spawn {
            argv: vec![
                "cmux".into(),
                "notify".into(),
                "--title".into(),
                ev.resolved_title(),
                "--body".into(),
                ev.body.clone(),
            ],
        }]
    }
}
