//! Sinks turn a canonical Event into `Action`s (data), and a single executor
//! runs them. Keeping effects as data is what makes --dry-run, tests and
//! doctor share one code path.

pub mod cmux;
pub mod exec;
pub mod herdr;
pub mod system;

use crate::context::Context;
use crate::event::Event;
use serde::Serialize;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Hooks must never hang an agent's turn: every spawned process gets this long.
pub const SPAWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "action")]
pub enum Action {
    Spawn { argv: Vec<String> },
    SpawnStdin { argv: Vec<String>, stdin: String },
    /// Try in order, stop at the first success. This is the notify-native.sh
    /// cascade generalized: native app → terminal-notifier → osascript.
    Cascade { steps: Vec<Action> },
}

#[derive(Debug, Clone)]
pub enum Availability {
    Yes,
    No(String),
}

pub trait Sink {
    fn name(&self) -> String;
    fn available(&self, ctx: &Context) -> Availability;
    /// Pure planning: no side effects beyond read-only lookups.
    fn plan(&self, ev: &Event, ctx: &Context) -> Vec<Action>;
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub ok: bool,
    /// Basename of the command that actually delivered (cascades pick one).
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub fn execute(actions: &[Action]) -> Outcome {
    let mut backend = None;
    let mut detail = None;
    let mut all_ok = true;
    for action in actions {
        match run_action(action) {
            Ok(name) => {
                backend.get_or_insert(name);
            }
            Err(err) => {
                all_ok = false;
                detail.get_or_insert(err);
            }
        }
    }
    Outcome { ok: all_ok, backend, detail }
}

fn run_action(action: &Action) -> Result<String, String> {
    match action {
        Action::Spawn { argv } => run_process(argv, None),
        Action::SpawnStdin { argv, stdin } => run_process(argv, Some(stdin)),
        Action::Cascade { steps } => {
            let mut last_err = "empty cascade".to_string();
            for step in steps {
                match run_action(step) {
                    Ok(name) => return Ok(name),
                    Err(err) => last_err = err,
                }
            }
            Err(last_err)
        }
    }
}

fn run_process(argv: &[String], stdin: Option<&str>) -> Result<String, String> {
    let (program, args) = argv.split_first().ok_or("empty argv")?;
    let label = basename(program);
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("{label}: spawn failed: {e}"))?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // Best-effort: a sink that closes stdin early must not fail the write.
        let _ = pipe.write_all(input.as_bytes());
    }
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(label),
            Ok(Some(status)) => return Err(format!("{label}: exit {status}")),
            Ok(None) => {
                if started.elapsed() > SPAWN_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{label}: timed out"));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("{label}: wait failed: {e}")),
        }
    }
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_stops_at_first_success() {
        let actions = [Action::Cascade {
            steps: vec![
                Action::Spawn { argv: vec!["/nonexistent/bin".into()] },
                Action::Spawn { argv: vec!["true".into()] },
                Action::Spawn { argv: vec!["/also/nonexistent".into()] },
            ],
        }];
        let out = execute(&actions);
        assert!(out.ok);
        assert_eq!(out.backend.as_deref(), Some("true"));
    }

    #[test]
    fn cascade_reports_last_error_when_all_fail() {
        let actions = [Action::Cascade {
            steps: vec![Action::Spawn { argv: vec!["false".into()] }],
        }];
        let out = execute(&actions);
        assert!(!out.ok);
        assert!(out.detail.unwrap().contains("false"));
    }

    #[test]
    fn spawn_stdin_feeds_child() {
        let out = execute(&[Action::SpawnStdin {
            argv: vec!["grep".into(), "-q".into(), "hello".into()],
            stdin: "hello world".into(),
        }]);
        assert!(out.ok);
    }
}
