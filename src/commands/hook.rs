//! Hook entry points. Contract: a notification dispatcher must NEVER break an
//! agent's turn — these always exit 0, whatever happens. Errors go to stderr
//! (visible in hook debug output) and, when possible, to the logbook.

use super::dispatch;
use crate::adapters;
use crate::cli::HookAgent;
use crate::config::Config;
use anyhow::{Context as _, Result};
use std::io::Read as _;

pub fn run(agent: HookAgent, cfg: &Config, dry_run: bool) -> i32 {
    if let Err(err) = inner(agent, cfg, dry_run) {
        eprintln!("herald: {err:#}");
    }
    0
}

fn inner(agent: HookAgent, cfg: &Config, dry_run: bool) -> Result<()> {
    let parsed = match agent {
        HookAgent::Claude { matcher, .. } => {
            adapters::claude::parse(&read_stdin()?, matcher.as_deref())?
        }
        HookAgent::Codex { payload } => {
            let json = match payload {
                Some(p) => p,
                None => read_stdin()?,
            };
            adapters::codex::parse(&json)?
        }
        HookAgent::Gemini { matcher } => {
            adapters::gemini::parse(&read_stdin()?, matcher.as_deref())?
        }
    };
    // None = event not notification-worthy; stay silent.
    let Some(ev) = parsed else { return Ok(()) };
    dispatch::dispatch(ev, cfg, dry_run)?;
    Ok(())
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading hook payload from stdin")?;
    Ok(buf)
}
