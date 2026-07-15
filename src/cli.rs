use crate::event::{EventKind, Urgency};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "herald", version, about = "Agent- and terminal-agnostic notification dispatcher")]
pub struct Cli {
    /// Alternative config file (default: ~/.config/herald/config.toml)
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    /// Print the delivery plan without executing or logging anything
    #[arg(long, global = true)]
    pub dry_run: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Entry points for agent hook systems (always exit 0)
    #[command(subcommand)]
    Hook(HookAgent),
    /// Emit a canonical event (the integration contract for any agent)
    Emit(EmitArgs),
    /// Diagnose sinks, presenter authorization and detected context
    Doctor {
        /// Build and install Herald.app (~/Applications) as native presenter.
        /// Installs ALONGSIDE any existing presenter; never replaces one.
        #[arg(long)]
        install_app: bool,
    },
    /// Send a synthetic event through the real pipeline
    Test(TestArgs),
    /// Inspect the events log (~/.local/state/herald/events.jsonl)
    Log(LogArgs),
}

#[derive(Subcommand)]
pub enum HookAgent {
    /// Claude Code: hook JSON on stdin
    Claude {
        /// Matcher this hook was registered under (not part of the payload)
        #[arg(long)]
        matcher: Option<String>,
        /// Legacy notify-native.sh positional (accepted, unused: the payload
        /// carries hook_event_name)
        #[arg(value_name = "EVENT", hide = true)]
        legacy_event: Option<String>,
    },
    /// Codex CLI: payload JSON as the final argv element
    Codex {
        payload: Option<String>,
    },
    /// Gemini CLI: hook JSON on stdin (experimental)
    Gemini {
        #[arg(long)]
        matcher: Option<String>,
    },
}

#[derive(Args)]
pub struct EmitArgs {
    /// Read a canonical Event as JSON from stdin (see docs/CONTRACT.md)
    #[arg(long, conflicts_with_all = ["source", "kind", "body"])]
    pub json: bool,
    /// Who is emitting: "cron:backup", "my-agent", ...
    #[arg(long, required_unless_present = "json")]
    pub source: Option<String>,
    #[arg(long, value_enum, required_unless_present = "json")]
    pub kind: Option<EventKind>,
    #[arg(long, required_unless_present = "json")]
    pub body: Option<String>,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub session: Option<String>,
    #[arg(long, value_enum)]
    pub urgency: Option<Urgency>,
}

#[derive(Args)]
pub struct TestArgs {
    #[arg(long, value_enum, default_value = "attention")]
    pub kind: EventKind,
    /// Restrict delivery to one sink (by name) for isolation
    #[arg(long)]
    pub sink: Option<String>,
}

#[derive(Args)]
pub struct LogArgs {
    /// Number of entries to show
    #[arg(short = 'n', long, default_value_t = 20)]
    pub lines: usize,
    /// Keep watching for new entries
    #[arg(short = 'f', long)]
    pub follow: bool,
    /// Print raw JSONL instead of the compact rendering
    #[arg(long)]
    pub raw: bool,
}
