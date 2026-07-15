//! `herald emit` — the canonical integration contract. Anything that can run
//! a command can notify: crons, unknown agents, shell one-liners.

use super::dispatch;
use crate::cli::EmitArgs;
use crate::config::Config;
use crate::event::Event;
use anyhow::{Context as _, Result, bail};
use std::io::Read as _;

pub fn run(args: EmitArgs, cfg: &Config, dry_run: bool) -> Result<()> {
    let ev = if args.json {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading event JSON from stdin")?;
        serde_json::from_str::<Event>(&buf).context("stdin is not a canonical Event (docs/CONTRACT.md)")?
    } else {
        let (Some(source), Some(kind), Some(body)) = (args.source, args.kind, args.body) else {
            bail!("--source, --kind and --body are required unless --json is used");
        };
        let mut ev = Event::new(source, kind, body);
        ev.title = args.title;
        ev.cwd = args.cwd;
        ev.session_id = args.session;
        ev.urgency = args.urgency;
        ev
    };

    let report = dispatch::dispatch(ev, cfg, dry_run)?;
    if !dry_run {
        println!("{}", report.summary());
    }
    Ok(())
}
