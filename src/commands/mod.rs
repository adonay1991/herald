pub mod dispatch;
pub mod doctor;
pub mod emit;
pub mod hook;
pub mod log_cmd;
pub mod test_cmd;

use crate::cli::{Cli, Cmd};
use crate::config;

pub fn run(cli: Cli) -> i32 {
    // A broken config must never break an agent's hook: hooks fall back to
    // defaults with a warning; explicit commands surface the error.
    let loaded = config::load(cli.config.as_deref());

    match cli.cmd {
        Cmd::Hook(agent) => {
            let cfg = match loaded {
                Ok((cfg, _)) => cfg,
                Err(err) => {
                    eprintln!("herald: config error, using defaults: {err:#}");
                    config::Config::default()
                }
            };
            hook::run(agent, &cfg, cli.dry_run)
        }
        Cmd::Emit(args) => fallible(loaded, |cfg| emit::run(args, cfg, cli.dry_run)),
        Cmd::Doctor { install_app, json } => {
            if install_app && let Err(err) = doctor::install_app() {
                eprintln!("herald: {err:#}");
                return 1;
            }
            doctor::run(cli.config.as_deref(), json)
        }
        Cmd::Test(args) => fallible(loaded, |cfg| test_cmd::run(args, cfg, cli.dry_run)),
        Cmd::Log(args) => match log_cmd::run(args) {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("herald: {err:#}");
                1
            }
        },
    }
}

fn fallible(
    loaded: anyhow::Result<(config::Config, std::path::PathBuf)>,
    f: impl FnOnce(&config::Config) -> anyhow::Result<()>,
) -> i32 {
    let result = loaded.and_then(|(cfg, _)| f(&cfg));
    match result {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("herald: {err:#}");
            1
        }
    }
}
