//! Configuration: deliberately flat, no rule engine. Zero-config must work;
//! the TOML file only overrides defaults. Location: ~/.config/herald/config.toml
//! (or $XDG_CONFIG_HOME/herald/config.toml, or --config).

use crate::event::{EventKind, Urgency};
use anyhow::{Context as _, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Policy {
    /// Notify regardless of focus.
    Always,
    /// Notify only when the emitting terminal is not frontmost (or headless).
    Unfocused,
    /// Log only.
    Never,
}

impl Urgency {
    pub fn default_policy(self) -> Policy {
        match self {
            Urgency::Critical => Policy::Always,
            Urgency::Normal => Policy::Unfocused,
            Urgency::Low => Policy::Never,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub routing: Routing,
    #[serde(default)]
    pub sinks: Sinks,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Routing {
    pub attention: Option<Policy>,
    pub turn_complete: Option<Policy>,
    pub session_start: Option<Policy>,
    pub session_end: Option<Policy>,
    pub error: Option<Policy>,
    pub info: Option<Policy>,
    /// Coalesce identical (source, kind) deliveries inside this window.
    /// Default 2000 ms; 0 disables.
    pub burst_window_ms: Option<u64>,
    /// "HH:MM-HH:MM" local time; inside the window everything below
    /// critical is log-only. Wraps midnight ("23:00-08:00").
    pub quiet_hours: Option<String>,
}

impl Routing {
    fn override_for(&self, kind: EventKind) -> Option<Policy> {
        match kind {
            EventKind::Attention => self.attention,
            EventKind::TurnComplete => self.turn_complete,
            EventKind::SessionStart => self.session_start,
            EventKind::SessionEnd => self.session_end,
            EventKind::Error => self.error,
            EventKind::Info => self.info,
        }
    }
}

impl Config {
    /// Per-kind override, falling back to the urgency-derived default.
    pub fn policy(&self, kind: EventKind, urgency: Urgency) -> Policy {
        self.routing
            .override_for(kind)
            .unwrap_or_else(|| urgency.default_policy())
    }

    pub fn burst_window_ms(&self) -> u64 {
        self.routing.burst_window_ms.unwrap_or(2000)
    }

    /// Parsed quiet-hours window; None when absent or malformed
    /// (a malformed window must never make herald drop actionable alerts).
    pub fn quiet_hours(&self) -> Option<QuietHours> {
        QuietHours::parse(self.routing.quiet_hours.as_deref()?)
    }
}

/// Minutes-of-day window, possibly wrapping midnight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuietHours {
    start: u16,
    end: u16,
}

impl QuietHours {
    pub fn parse(text: &str) -> Option<Self> {
        let (a, b) = text.split_once('-')?;
        let start = parse_hhmm(a.trim())?;
        let end = parse_hhmm(b.trim())?;
        if start == end {
            return None; // zero-length window = disabled
        }
        Some(QuietHours { start, end })
    }

    pub fn contains(&self, minute_of_day: u16) -> bool {
        if self.start < self.end {
            minute_of_day >= self.start && minute_of_day < self.end
        } else {
            // wraps midnight
            minute_of_day >= self.start || minute_of_day < self.end
        }
    }
}

fn parse_hhmm(text: &str) -> Option<u16> {
    let (h, m) = text.split_once(':')?;
    let h: u16 = h.parse().ok()?;
    let m: u16 = m.parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Sinks {
    #[serde(default)]
    pub macos_native: MacosNative,
    #[serde(default)]
    pub herdr: HerdrSink,
    #[serde(default)]
    pub cmux: CmuxSink,
    #[serde(default)]
    pub osc: OscSink,
    #[serde(default)]
    pub exec: Vec<ExecSink>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct MacosNative {
    /// Path to the .app bundle whose Contents/MacOS binary presents banners.
    /// Default resolution: ~/Applications/Herald.app, else ~/Applications/ClaudeNotify.app.
    pub app_path: Option<PathBuf>,
    /// Per-urgency sound overrides (macOS system sound names).
    /// Defaults: critical = Submarine, normal = Glass, low = silent.
    #[serde(default)]
    pub sounds: Sounds,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Sounds {
    pub critical: Option<String>,
    pub normal: Option<String>,
    pub low: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HerdrSink {
    pub enabled: bool,
    /// Also push pane state via `herdr pane report-agent`. Off by default:
    /// herdr already paints agent state on its own; do not double-paint.
    pub report_state: bool,
}

impl Default for HerdrSink {
    fn default() -> Self {
        HerdrSink {
            enabled: true,
            report_state: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CmuxSink {
    pub enabled: bool,
}

impl Default for CmuxSink {
    fn default() -> Self {
        CmuxSink { enabled: true }
    }
}

/// In-terminal notifications via OSC escape sequences written to /dev/tty.
/// Opt-in: the terminal decides presentation and focus handling, which makes
/// this the most portable sink — but only for terminals that support it
/// (kitty, Ghostty, iTerm2, WezTerm, ...).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OscSink {
    pub enabled: bool,
    #[serde(default)]
    pub protocol: OscProtocol,
    /// Treat the terminal as owning the pane: suppress the system banner.
    #[serde(default)]
    pub exclusive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OscProtocol {
    /// OSC 9 — broadest support (iTerm2, WezTerm, Ghostty, Windows Terminal).
    #[default]
    Osc9,
    /// OSC 777 ;notify — urxvt lineage (Ghostty, Warp).
    Osc777,
}

/// Third-party terminal integration: when `when_env` is present in the
/// environment, run `command` with the canonical Event JSON on stdin.
/// See docs/SINKS.md.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecSink {
    pub name: String,
    pub when_env: String,
    pub command: Vec<String>,
    #[serde(default = "default_min_urgency")]
    pub min_urgency: Urgency,
    /// Exclusive sinks behave like a harness: they suppress the system banner.
    #[serde(default)]
    pub exclusive: bool,
}

fn default_min_urgency() -> Urgency {
    Urgency::Normal
}

pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"));
    base.join("herald").join("config.toml")
}

pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local").join("state"));
    base.join("herald")
}

pub fn log_path() -> PathBuf {
    state_dir().join("events.jsonl")
}

pub fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/".into()))
}

pub fn load(explicit: Option<&Path>) -> Result<(Config, PathBuf)> {
    let path = explicit.map(Path::to_path_buf).unwrap_or_else(default_path);
    if !path.exists() {
        return Ok((Config::default(), path));
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
    Ok((cfg, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_config_policies() {
        let cfg = Config::default();
        assert_eq!(
            cfg.policy(EventKind::Attention, Urgency::Critical),
            Policy::Always
        );
        assert_eq!(
            cfg.policy(EventKind::TurnComplete, Urgency::Normal),
            Policy::Unfocused
        );
        assert_eq!(cfg.policy(EventKind::Info, Urgency::Low), Policy::Never);
        assert_eq!(cfg.burst_window_ms(), 2000);
        assert!(cfg.quiet_hours().is_none());
    }

    #[test]
    fn kind_override_beats_urgency_default() {
        let cfg: Config = toml::from_str("[routing]\nturn-complete = \"never\"\n").unwrap();
        assert_eq!(
            cfg.policy(EventKind::TurnComplete, Urgency::Normal),
            Policy::Never
        );
        assert_eq!(
            cfg.policy(EventKind::Info, Urgency::Critical),
            Policy::Always
        );
    }

    #[test]
    fn parses_full_config() {
        let text = r#"
[routing]
attention = "always"
burst-window-ms = 0
quiet-hours = "23:00-08:00"

[sinks.macos_native]
app_path = "/Users/x/Applications/ClaudeNotify.app"
sounds = { critical = "Sosumi", normal = "Pop" }

[sinks.herdr]
enabled = true
report_state = false

[sinks.cmux]
enabled = false

[sinks.osc]
enabled = true
protocol = "osc777"
exclusive = true

[[sinks.exec]]
name = "myterm"
when_env = "MYTERM_SOCKET"
command = ["myterm-notify", "--stdin"]
min_urgency = "normal"
exclusive = true
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        assert!(!cfg.sinks.cmux.enabled);
        assert_eq!(cfg.sinks.exec.len(), 1);
        assert!(cfg.sinks.exec[0].exclusive);
        assert_eq!(cfg.burst_window_ms(), 0);
        assert!(cfg.quiet_hours().is_some());
        assert!(cfg.sinks.osc.enabled);
        assert_eq!(cfg.sinks.osc.protocol, OscProtocol::Osc777);
        assert_eq!(
            cfg.sinks.macos_native.sounds.critical.as_deref(),
            Some("Sosumi")
        );
    }

    #[test]
    fn unknown_keys_rejected() {
        assert!(toml::from_str::<Config>("[general]\nfoo = 1\n").is_err());
    }

    #[test]
    fn quiet_hours_plain_window() {
        let q = QuietHours::parse("09:30-17:00").unwrap();
        assert!(q.contains(9 * 60 + 30));
        assert!(q.contains(12 * 60));
        assert!(!q.contains(17 * 60));
        assert!(!q.contains(8 * 60));
    }

    #[test]
    fn quiet_hours_wraps_midnight() {
        let q = QuietHours::parse("23:00-08:00").unwrap();
        assert!(q.contains(23 * 60 + 30));
        assert!(q.contains(2 * 60));
        assert!(!q.contains(12 * 60));
        assert!(!q.contains(8 * 60));
    }

    #[test]
    fn quiet_hours_rejects_malformed() {
        for bad in [
            "",
            "23-08",
            "25:00-08:00",
            "23:00-08:61",
            "08:00-08:00",
            "aa:bb-cc:dd",
        ] {
            assert!(QuietHours::parse(bad).is_none(), "{bad}");
        }
    }
}
