//! Gemini CLI adapter (experimental). Gemini hooks use the same shape as
//! Claude Code: JSON on stdin with hook_event_name. We reuse the Claude
//! mapping and relabel the source; revisit when Gemini's schema diverges.

use crate::event::Event;
use anyhow::Result;

pub fn parse(json: &str, matcher: Option<&str>) -> Result<Option<Event>> {
    Ok(super::claude::parse(json, matcher)?.map(|mut ev| {
        ev.source = "gemini".to_string();
        ev
    }))
}
