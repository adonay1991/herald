//! OSC sink: in-terminal notifications via escape sequences written to
//! /dev/tty. The terminal decides presentation and focus handling, which
//! makes this the most portable delivery there is — but only on terminals
//! that support it (kitty, Ghostty, iTerm2, WezTerm, ...), so it is opt-in.

use super::{Action, Availability, Sink};
use crate::config::{OscProtocol, OscSink as OscConfig};
use crate::context::Context;
use crate::event::Event;

pub struct OscSink {
    cfg: OscConfig,
}

impl OscSink {
    pub fn new(cfg: OscConfig) -> Self {
        OscSink { cfg }
    }
}

impl Sink for OscSink {
    fn name(&self) -> String {
        "osc".into()
    }

    fn available(&self, ctx: &Context) -> Availability {
        if !self.cfg.enabled {
            return Availability::No("disabled in config".into());
        }
        if ctx.headless {
            return Availability::No("no controlling terminal".into());
        }
        Availability::Yes
    }

    fn plan(&self, ev: &Event, _ctx: &Context) -> Vec<Action> {
        let title = sanitize(&ev.resolved_title());
        let body = sanitize(&ev.body);
        let data = match self.cfg.protocol {
            OscProtocol::Osc9 => format!("\u{1b}]9;{title} — {body}\u{7}"),
            OscProtocol::Osc777 => format!("\u{1b}]777;notify;{title};{body}\u{7}"),
        };
        vec![Action::WriteTty { data }]
    }
}

/// Strip control characters so event text can never smuggle its own escape
/// sequences into the terminal.
fn sanitize(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Context, Harness};
    use crate::event::EventKind;

    fn ctx() -> Context {
        Context {
            harness: Harness::Plain,
            terminal_bundle_id: None,
            headless: false,
            tmux: false,
        }
    }

    fn cfg(protocol: OscProtocol) -> OscConfig {
        OscConfig {
            enabled: true,
            protocol,
            exclusive: false,
        }
    }

    #[test]
    fn osc9_sequence_shape() {
        let sink = OscSink::new(cfg(OscProtocol::Osc9));
        let ev = Event::new("claude", EventKind::Attention, "needs you");
        let actions = sink.plan(&ev, &ctx());
        let Action::WriteTty { data } = &actions[0] else {
            panic!()
        };
        assert!(data.starts_with("\u{1b}]9;"));
        assert!(data.ends_with('\u{7}'));
        assert!(data.contains("needs you"));
    }

    #[test]
    fn osc777_sequence_shape() {
        let sink = OscSink::new(cfg(OscProtocol::Osc777));
        let ev = Event::new("codex", EventKind::TurnComplete, "done");
        let actions = sink.plan(&ev, &ctx());
        let Action::WriteTty { data } = &actions[0] else {
            panic!()
        };
        assert!(data.starts_with("\u{1b}]777;notify;"));
    }

    #[test]
    fn event_text_cannot_inject_escapes() {
        let sink = OscSink::new(cfg(OscProtocol::Osc9));
        let ev = Event::new("evil", EventKind::Attention, "x\u{1b}]0;pwned\u{7}y");
        let actions = sink.plan(&ev, &ctx());
        let Action::WriteTty { data } = &actions[0] else {
            panic!()
        };
        // exactly one ESC (ours) and one BEL (ours)
        assert_eq!(data.matches('\u{1b}').count(), 1);
        assert_eq!(data.matches('\u{7}').count(), 1);
        assert!(data.contains("xy") || data.contains("pwned"));
    }
}
