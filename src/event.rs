//! Canonical notification event: the single shape every agent adapter
//! produces and every sink consumes. Stability contract: see docs/CONTRACT.md.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum EventKind {
    /// Agent is blocked waiting on a human (permission, idle, needs-input).
    Attention,
    /// A turn/response finished; the human may want to come back.
    TurnComplete,
    SessionStart,
    SessionEnd,
    /// Something failed (broken cron, crashed agent).
    Error,
    /// Generic FYI. Low urgency: logged, not shown, unless configured otherwise.
    Info,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

impl EventKind {
    pub fn default_urgency(self) -> Urgency {
        match self {
            EventKind::Attention | EventKind::Error => Urgency::Critical,
            EventKind::TurnComplete => Urgency::Normal,
            EventKind::SessionStart | EventKind::SessionEnd | EventKind::Info => Urgency::Low,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Who emitted this: "claude", "codex", "gemini", "cron:backup", or anything else.
    pub source: String,
    pub kind: EventKind,
    /// Explicit urgency override; when absent the kind's default applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<Urgency>,
    /// Explicit title; when absent it is derived from source and cwd.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Extra label (subagent type, model, ...) for harness state sinks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_label: Option<String>,
    /// Original payload, passed through untouched for sinks that want it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

impl Event {
    pub fn new(source: impl Into<String>, kind: EventKind, body: impl Into<String>) -> Self {
        Event {
            source: source.into(),
            kind,
            urgency: None,
            title: None,
            body: body.into(),
            cwd: None,
            session_id: None,
            agent_label: None,
            raw: None,
        }
    }

    pub fn urgency(&self) -> Urgency {
        self.urgency.unwrap_or_else(|| self.kind.default_urgency())
    }

    /// "{display source} — {basename(cwd)}" unless an explicit title was set.
    pub fn resolved_title(&self) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        let name = display_source(&self.source);
        match self.cwd.as_deref().and_then(basename) {
            Some(dir) => format!("{name} — {dir}"),
            None => name,
        }
    }

    /// macOS sound names, parity with the historical notify-native.sh mapping.
    pub fn sound(&self) -> Option<&'static str> {
        match self.urgency() {
            Urgency::Critical => Some("Submarine"),
            Urgency::Normal => Some("Glass"),
            Urgency::Low => None,
        }
    }
}

fn display_source(source: &str) -> String {
    match source {
        "claude" => "Claude Code".to_string(),
        "codex" => "Codex".to_string(),
        "gemini" => "Gemini CLI".to_string(),
        other => other.to_string(),
    }
}

fn basename(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}

pub fn truncate_body(text: &str, max: usize) -> String {
    let clean = text.replace('\n', " ");
    let trimmed = clean.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_urgency_per_kind() {
        assert_eq!(EventKind::Attention.default_urgency(), Urgency::Critical);
        assert_eq!(EventKind::Error.default_urgency(), Urgency::Critical);
        assert_eq!(EventKind::TurnComplete.default_urgency(), Urgency::Normal);
        assert_eq!(EventKind::Info.default_urgency(), Urgency::Low);
        assert_eq!(EventKind::SessionStart.default_urgency(), Urgency::Low);
    }

    #[test]
    fn title_derived_from_source_and_cwd() {
        let mut ev = Event::new("claude", EventKind::Attention, "needs you");
        ev.cwd = Some(PathBuf::from("/Users/x/projects/acme"));
        assert_eq!(ev.resolved_title(), "Claude Code — acme");
        ev.title = Some("custom".into());
        assert_eq!(ev.resolved_title(), "custom");
    }

    #[test]
    fn sounds_match_legacy_mapping() {
        let ev = Event::new("claude", EventKind::Attention, "");
        assert_eq!(ev.sound(), Some("Submarine"));
        let ev = Event::new("claude", EventKind::TurnComplete, "");
        assert_eq!(ev.sound(), Some("Glass"));
        let ev = Event::new("claude", EventKind::Info, "");
        assert_eq!(ev.sound(), None);
    }

    #[test]
    fn urgency_override_wins() {
        let mut ev = Event::new("cron:backup", EventKind::Info, "findings");
        ev.urgency = Some(Urgency::Critical);
        assert_eq!(ev.urgency(), Urgency::Critical);
    }

    #[test]
    fn truncate_collapses_newlines() {
        assert_eq!(truncate_body("a\nb", 10), "a b");
        assert_eq!(truncate_body(&"x".repeat(300), 5), "xxxxx…");
    }
}
