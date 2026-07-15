//! Codex CLI adapter. Transport: `notify = ["herald", "hook", "codex"]` in
//! ~/.codex/config.toml — Codex execs the program with the payload JSON as
//! the final argv element. Only event today: agent-turn-complete, with
//! kebab-case keys.

use crate::event::{Event, EventKind, truncate_body};
use anyhow::{Context as _, Result};
use serde_json::Value;
use std::path::PathBuf;

pub fn parse(json: &str) -> Result<Option<Event>> {
    let v: Value = serde_json::from_str(json).context("codex notify payload is not JSON")?;
    if v["type"].as_str() != Some("agent-turn-complete") {
        return Ok(None);
    }
    let body = v["last-assistant-message"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(|s| truncate_body(s, 200))
        .unwrap_or_else(|| "Turn complete".to_string());
    let mut ev = Event::new("codex", EventKind::TurnComplete, body);
    ev.cwd = v["cwd"].as_str().map(PathBuf::from);
    ev.session_id = v["thread-id"].as_str().map(str::to_string);
    ev.raw = Some(v);
    Ok(Some(ev))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Urgency;

    const TURN_COMPLETE: &str = include_str!("../../tests/fixtures/codex_turn_complete.json");

    #[test]
    fn turn_complete_maps_kebab_case_keys() {
        let ev = parse(TURN_COMPLETE).unwrap().unwrap();
        assert_eq!(ev.kind, EventKind::TurnComplete);
        assert_eq!(ev.urgency(), Urgency::Normal);
        assert_eq!(ev.body, "Listo: he corregido los tests.");
        assert_eq!(ev.session_id.as_deref(), Some("thread-42"));
        assert_eq!(ev.resolved_title(), "Codex — acme");
    }

    #[test]
    fn unknown_type_is_none() {
        assert!(parse(r#"{"type":"something-else"}"#).unwrap().is_none());
    }
}
