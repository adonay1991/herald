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
    pub exec: Vec<ExecSink>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct MacosNative {
    /// Path to the .app bundle whose Contents/MacOS binary presents banners.
    /// Default resolution: ~/Applications/Herald.app, else ~/Applications/ClaudeNotify.app.
    pub app_path: Option<PathBuf>,
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
        HerdrSink { enabled: true, report_state: false }
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
        assert_eq!(cfg.policy(EventKind::Attention, Urgency::Critical), Policy::Always);
        assert_eq!(cfg.policy(EventKind::TurnComplete, Urgency::Normal), Policy::Unfocused);
        assert_eq!(cfg.policy(EventKind::Info, Urgency::Low), Policy::Never);
    }

    #[test]
    fn kind_override_beats_urgency_default() {
        let cfg: Config = toml::from_str("[routing]\nturn-complete = \"never\"\n").unwrap();
        assert_eq!(cfg.policy(EventKind::TurnComplete, Urgency::Normal), Policy::Never);
        // urgency override still applies to non-overridden kinds
        assert_eq!(cfg.policy(EventKind::Info, Urgency::Critical), Policy::Always);
    }

    #[test]
    fn parses_full_config() {
        let text = r#"
[routing]
attention = "always"

[sinks.macos_native]
app_path = "/Users/x/Applications/ClaudeNotify.app"

[sinks.herdr]
enabled = true
report_state = false

[sinks.cmux]
enabled = false

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
    }

    #[test]
    fn unknown_keys_rejected() {
        assert!(toml::from_str::<Config>("[general]\nfoo = 1\n").is_err());
    }
}
