//! Claude Code hook adapter. Transport: JSON on stdin, one envelope per hook
//! event, discriminated by `hook_event_name`. The matcher is not part of the
//! payload — the hook wiring may pass it via --matcher for finer mapping.

use crate::event::{Event, EventKind, truncate_body};
use anyhow::{Context as _, Result};
use serde_json::Value;
use std::path::PathBuf;

pub fn parse(json: &str, matcher: Option<&str>) -> Result<Option<Event>> {
    let v: Value = serde_json::from_str(json).context("claude hook payload is not JSON")?;
    let hook_event = v["hook_event_name"].as_str().unwrap_or_default();

    let (kind, body) = match hook_event {
        "Notification" => {
            let kind = match matcher {
                Some("agent_completed") => EventKind::TurnComplete,
                _ => EventKind::Attention,
            };
            let msg = v["message"].as_str().unwrap_or("Needs your attention");
            (kind, msg.to_string())
        }
        "Stop" => {
            let msg = v["last_assistant_message"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(|s| truncate_body(s, 200))
                .unwrap_or_else(|| "Turn complete".to_string());
            (EventKind::TurnComplete, msg)
        }
        "SubagentStop" => (EventKind::Info, "Subagent finished".to_string()),
        "SessionStart" => (EventKind::SessionStart, String::new()),
        "SessionEnd" => (EventKind::SessionEnd, String::new()),
        "PermissionRequest" => {
            let tool = v["tool_name"].as_str().unwrap_or("a tool");
            (
                EventKind::Attention,
                format!("Permission requested: {tool}"),
            )
        }
        _ => return Ok(None),
    };

    let mut ev = Event::new("claude", kind, body);
    ev.cwd = v["cwd"].as_str().map(PathBuf::from);
    ev.session_id = v["session_id"].as_str().map(str::to_string);
    ev.agent_label = v["agent_type"]
        .as_str()
        .or_else(|| v["agent_id"].as_str())
        .map(str::to_string);
    ev.raw = Some(v);
    Ok(Some(ev))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Urgency;

    const NOTIFICATION: &str = include_str!("../../tests/fixtures/claude_notification.json");
    const STOP: &str = include_str!("../../tests/fixtures/claude_stop.json");
    const SUBAGENT_STOP: &str = include_str!("../../tests/fixtures/claude_subagent_stop.json");
    const SESSION_START: &str = include_str!("../../tests/fixtures/claude_session_start.json");

    #[test]
    fn notification_is_attention_with_message() {
        let ev = parse(NOTIFICATION, None).unwrap().unwrap();
        assert_eq!(ev.kind, EventKind::Attention);
        assert_eq!(ev.urgency(), Urgency::Critical);
        assert_eq!(ev.body, "Claude needs your permission to use Bash");
        assert_eq!(ev.resolved_title(), "Claude Code — acme");
        assert!(ev.raw.is_some());
    }

    #[test]
    fn agent_completed_matcher_maps_to_turn_complete() {
        let ev = parse(NOTIFICATION, Some("agent_completed"))
            .unwrap()
            .unwrap();
        assert_eq!(ev.kind, EventKind::TurnComplete);
    }

    #[test]
    fn stop_truncates_last_assistant_message() {
        let ev = parse(STOP, None).unwrap().unwrap();
        assert_eq!(ev.kind, EventKind::TurnComplete);
        assert!(ev.body.chars().count() <= 201, "body: {}", ev.body);
        assert!(ev.body.ends_with('…'));
        assert_eq!(ev.session_id.as_deref(), Some("abc123"));
    }

    #[test]
    fn subagent_stop_is_low_info() {
        let ev = parse(SUBAGENT_STOP, None).unwrap().unwrap();
        assert_eq!(ev.kind, EventKind::Info);
        assert_eq!(ev.urgency(), Urgency::Low);
        assert_eq!(ev.agent_label.as_deref(), Some("Explore"));
    }

    #[test]
    fn session_start_is_lifecycle() {
        let ev = parse(SESSION_START, None).unwrap().unwrap();
        assert_eq!(ev.kind, EventKind::SessionStart);
        assert_eq!(ev.urgency(), Urgency::Low);
    }

    #[test]
    fn unknown_event_is_none() {
        let ev = parse(r#"{"hook_event_name":"PreToolUse","cwd":"/tmp"}"#, None).unwrap();
        assert!(ev.is_none());
    }

    #[test]
    fn invalid_json_is_error() {
        assert!(parse("not json", None).is_err());
    }
}
