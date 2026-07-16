//! System (desktop) notification sink.
//!
//! macOS: on macOS 26 the legacy notification API (used by osascript and
//! terminal-notifier) fails silently, so the primary backend is a minimal
//! .app bundle speaking UNUserNotificationCenter; the rest of the cascade
//! covers older Macs. The presenter receives HERALD_ACTIVATE_BUNDLE so a
//! click on the banner can activate the originating terminal.
//!
//! Linux: notify-send with a 1:1 urgency mapping.
//!
//! Both branches are plain argv construction, so `cfg!` (runtime-const)
//! keeps them type-checked on every platform.

use super::{Action, Availability, Sink};
use crate::config::{Config, Sounds};
use crate::context::Context;
use crate::event::{Event, Urgency};
use std::path::{Path, PathBuf};

pub struct SystemSink {
    app_path: Option<PathBuf>,
    sounds: Sounds,
}

impl SystemSink {
    pub fn from_config(cfg: &Config) -> Self {
        let app_path = cfg
            .sinks
            .macos_native
            .app_path
            .clone()
            .or_else(default_app_path);
        SystemSink {
            app_path,
            sounds: cfg.sinks.macos_native.sounds.clone(),
        }
    }

    #[cfg(test)]
    fn bare() -> Self {
        SystemSink {
            app_path: None,
            sounds: Sounds::default(),
        }
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

    /// Config override per urgency, defaulting to the historical mapping
    /// (Submarine / Glass / silent). Empty string = explicit silence.
    fn sound_for(&self, urgency: Urgency) -> Option<String> {
        let (configured, default) = match urgency {
            Urgency::Critical => (&self.sounds.critical, Some("Submarine")),
            Urgency::Normal => (&self.sounds.normal, Some("Glass")),
            Urgency::Low => (&self.sounds.low, None),
        };
        match configured {
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s.clone()),
            None => default.map(str::to_string),
        }
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
        if cfg!(target_os = "macos") {
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
        } else {
            match which("notify-send") {
                Some(_) => Availability::Yes,
                None => Availability::No("notify-send not on PATH".into()),
            }
        }
    }

    fn plan(&self, ev: &Event, ctx: &Context) -> Vec<Action> {
        let title = ev.resolved_title();
        let body = if ev.body.is_empty() {
            title.clone()
        } else {
            ev.body.clone()
        };

        if !cfg!(target_os = "macos") {
            let urgency = match ev.urgency() {
                Urgency::Critical => "critical",
                Urgency::Normal => "normal",
                Urgency::Low => "low",
            };
            return vec![Action::Spawn {
                argv: vec![
                    "notify-send".into(),
                    "-u".into(),
                    urgency.into(),
                    "-a".into(),
                    "herald".into(),
                    title,
                    body,
                ],
                env: vec![],
            }];
        }

        let sound = self.sound_for(ev.urgency());
        let mut steps = Vec::new();

        if let Some(bin) = self.presenter_binary() {
            let mut argv = vec![
                bin.to_string_lossy().into_owned(),
                title.clone(),
                body.clone(),
            ];
            if let Some(s) = &sound {
                argv.push(s.clone());
            }
            // A conforming presenter activates this app when the banner is
            // clicked; presenters that predate the contract ignore it.
            let env = ctx
                .terminal_bundle_id
                .iter()
                .map(|b| ("HERALD_ACTIVATE_BUNDLE".to_string(), b.clone()))
                .collect();
            steps.push(Action::Spawn { argv, env });
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
            if let Some(s) = &sound {
                argv.extend(["-sound".into(), s.clone()]);
            }
            if let Some(bundle) = &ctx.terminal_bundle_id {
                argv.extend(["-activate".into(), bundle.clone()]);
            }
            steps.push(Action::Spawn { argv, env: vec![] });
        }

        steps.push(Action::Spawn {
            argv: vec![
                "osascript".into(),
                "-e".into(),
                osascript_script(&title, &body, sound.as_deref()),
            ],
            env: vec![],
        });

        vec![Action::Cascade { steps }]
    }
}

/// AppleScript breaks on unescaped quotes and backslashes.
fn osascript_script(title: &str, body: &str, sound: Option<&str>) -> String {
    let esc = |s: &str| {
        s.replace('\n', " ")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    };
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
            tmux: false,
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn plan_is_one_cascade_ending_in_osascript() {
        let sink = SystemSink::bare();
        let ev = Event::new("claude", EventKind::Attention, "needs you");
        let actions = sink.plan(&ev, &plain_ctx());
        assert_eq!(actions.len(), 1);
        let Action::Cascade { steps } = &actions[0] else {
            panic!("expected cascade")
        };
        let Action::Spawn { argv, .. } = steps.last().unwrap() else {
            panic!("expected spawn")
        };
        assert_eq!(argv[0], "osascript");
        assert!(argv[2].contains("Submarine"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn terminal_notifier_gets_activate_when_bundle_known() {
        let sink = SystemSink::bare();
        let ev = Event::new("claude", EventKind::TurnComplete, "done");
        let actions = sink.plan(&ev, &plain_ctx());
        let Action::Cascade { steps } = &actions[0] else {
            panic!("expected cascade")
        };
        let Action::Spawn { argv, .. } = &steps[0] else {
            panic!()
        };
        assert!(argv.contains(&"-activate".to_string()));
        assert!(argv.contains(&"com.apple.Terminal".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn low_urgency_has_no_sound() {
        let sink = SystemSink::bare();
        let ev = Event::new("claude", EventKind::Info, "fyi");
        let actions = sink.plan(&ev, &plain_ctx());
        let Action::Cascade { steps } = &actions[0] else {
            panic!()
        };
        let Action::Spawn { argv, .. } = steps.last().unwrap() else {
            panic!()
        };
        assert!(!argv[2].contains("sound name"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn configured_sounds_override_defaults() {
        let cfg: Config = toml::from_str(
            "[sinks.macos_native]\nsounds = { critical = \"Sosumi\", normal = \"\" }\n",
        )
        .unwrap();
        let sink = SystemSink::from_config(&cfg);
        assert_eq!(sink.sound_for(Urgency::Critical).as_deref(), Some("Sosumi"));
        assert_eq!(sink.sound_for(Urgency::Normal), None); // "" = explicit silence
        assert_eq!(sink.sound_for(Urgency::Low), None);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn linux_plan_is_notify_send_with_urgency() {
        let sink = SystemSink::bare();
        let ev = Event::new("claude", EventKind::Attention, "needs you");
        let actions = sink.plan(&ev, &plain_ctx());
        let Action::Spawn { argv, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(argv[0], "notify-send");
        assert!(argv.contains(&"critical".to_string()));
    }

    #[test]
    fn osascript_escapes_quotes() {
        let script = osascript_script("t\"x", "b\\y", None);
        assert!(script.contains("t\\\"x"));
        assert!(script.contains("b\\\\y"));
    }
}
