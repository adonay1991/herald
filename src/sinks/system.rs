//! System (desktop) notification sink. On macOS 26 the legacy notification
//! API (used by osascript and terminal-notifier) fails silently, so the
//! primary backend is a minimal .app bundle speaking UNUserNotificationCenter;
//! the rest of the cascade covers older Macs and foreign setups.

use super::{Action, Availability, Sink};
use crate::config::Config;
use crate::context::Context;
use crate::event::Event;
use std::path::{Path, PathBuf};

pub struct SystemSink {
    app_path: Option<PathBuf>,
}

impl SystemSink {
    pub fn from_config(cfg: &Config) -> Self {
        let app_path = cfg
            .sinks
            .macos_native
            .app_path
            .clone()
            .or_else(default_app_path);
        SystemSink { app_path }
    }

    /// First regular file inside Contents/MacOS — renaming app or binary
    /// must not break resolution.
    pub fn presenter_binary(&self) -> Option<PathBuf> {
        let app = self.app_path.as_ref()?;
        let dir = app.join("Contents").join("MacOS");
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        entries.into_iter().next()
    }
}

fn default_app_path() -> Option<PathBuf> {
    let apps = crate::config::home_dir().join("Applications");
    for name in ["Herald.app", "ClaudeNotify.app"] {
        let candidate = apps.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

impl Sink for SystemSink {
    fn name(&self) -> String {
        "system".into()
    }

    fn available(&self, _ctx: &Context) -> Availability {
        if self.presenter_binary().is_some() {
            return Availability::Yes;
        }
        if which("terminal-notifier").is_some()
            || Path::new("/opt/homebrew/bin/terminal-notifier").exists()
            || which("osascript").is_some()
        {
            return Availability::Yes;
        }
        Availability::No("no presenter app, terminal-notifier or osascript found".into())
    }

    fn plan(&self, ev: &Event, ctx: &Context) -> Vec<Action> {
        let title = ev.resolved_title();
        let body = if ev.body.is_empty() { title.clone() } else { ev.body.clone() };
        let sound = ev.sound();
        let mut steps = Vec::new();

        if let Some(bin) = self.presenter_binary() {
            let mut argv = vec![bin.to_string_lossy().into_owned(), title.clone(), body.clone()];
            if let Some(s) = sound {
                argv.push(s.to_string());
            }
            steps.push(Action::Spawn { argv });
        }

        // launchd jobs carry a minimal PATH: try PATH first, then brew's path.
        for tn in ["terminal-notifier", "/opt/homebrew/bin/terminal-notifier"] {
            let mut argv = vec![
                tn.to_string(),
                "-title".into(),
                title.clone(),
                "-message".into(),
                body.clone(),
            ];
            if let Some(s) = sound {
                argv.extend(["-sound".into(), s.to_string()]);
            }
            if let Some(bundle) = &ctx.terminal_bundle_id {
                argv.extend(["-activate".into(), bundle.clone()]);
            }
            steps.push(Action::Spawn { argv });
        }

        steps.push(Action::Spawn {
            argv: vec![
                "osascript".into(),
                "-e".into(),
                osascript_script(&title, &body, sound),
            ],
        });

        vec![Action::Cascade { steps }]
    }
}

/// AppleScript breaks on unescaped quotes and backslashes.
fn osascript_script(title: &str, body: &str, sound: Option<&str>) -> String {
    let esc = |s: &str| s.replace('\n', " ").replace('\\', "\\\\").replace('"', "\\\"");
    let mut script = format!(
        "display notification \"{}\" with title \"{}\"",
        esc(body),
        esc(title)
    );
    if let Some(s) = sound {
        script.push_str(&format!(" sound name \"{s}\""));
    }
    script
}

pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Context, Harness};
    use crate::event::EventKind;

    fn plain_ctx() -> Context {
        Context {
            harness: Harness::Plain,
            terminal_bundle_id: Some("com.apple.Terminal".into()),
            headless: false,
        }
    }

    #[test]
    fn plan_is_one_cascade_ending_in_osascript() {
        let sink = SystemSink { app_path: None };
        let ev = Event::new("claude", EventKind::Attention, "needs you");
        let actions = sink.plan(&ev, &plain_ctx());
        assert_eq!(actions.len(), 1);
        let Action::Cascade { steps } = &actions[0] else {
            panic!("expected cascade")
        };
        let Action::Spawn { argv } = steps.last().unwrap() else {
            panic!("expected spawn")
        };
        assert_eq!(argv[0], "osascript");
        assert!(argv[2].contains("Submarine"));
    }

    #[test]
    fn terminal_notifier_gets_activate_when_bundle_known() {
        let sink = SystemSink { app_path: None };
        let ev = Event::new("claude", EventKind::TurnComplete, "done");
        let actions = sink.plan(&ev, &plain_ctx());
        let Action::Cascade { steps } = &actions[0] else {
            panic!("expected cascade")
        };
        let Action::Spawn { argv } = &steps[0] else { panic!() };
        assert!(argv.contains(&"-activate".to_string()));
        assert!(argv.contains(&"com.apple.Terminal".to_string()));
    }

    #[test]
    fn osascript_escapes_quotes() {
        let script = osascript_script("t\"x", "b\\y", None);
        assert!(script.contains("t\\\"x"));
        assert!(script.contains("b\\\\y"));
    }

    #[test]
    fn low_urgency_has_no_sound() {
        let sink = SystemSink { app_path: None };
        let ev = Event::new("claude", EventKind::Info, "fyi");
        let actions = sink.plan(&ev, &plain_ctx());
        let Action::Cascade { steps } = &actions[0] else { panic!() };
        let Action::Spawn { argv } = steps.last().unwrap() else { panic!() };
        assert!(!argv[2].contains("sound name"));
    }
}
