//! `herald test` — a synthetic event through the real pipeline, so what you
//! see is exactly what agents will produce.

use super::dispatch;
use crate::cli::TestArgs;
use crate::config::Config;
use crate::event::Event;
use anyhow::Result;

pub fn run(args: TestArgs, cfg: &Config, dry_run: bool) -> Result<()> {
    let mut ev = Event::new("herald", args.kind, "Test notification from herald");
    ev.cwd = std::env::current_dir().ok();
    let report = dispatch::dispatch_filtered(ev, cfg, dry_run, args.sink.as_deref())?;
    if !dry_run {
        println!("{}", report.summary());
    }
    Ok(())
}
