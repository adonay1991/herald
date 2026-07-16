//! End-to-end CLI tests. Real execution is confined to a tempdir HOME with a
//! synthetic environment; anything that would show UI runs under --dry-run.

use assert_cmd::Command;
use predicates::prelude::*;

fn herald() -> Command {
    let mut cmd = Command::cargo_bin("herald").unwrap();
    cmd.env_clear().env("PATH", "/usr/bin:/bin");
    cmd
}

const CLAUDE_NOTIFICATION: &str = include_str!("fixtures/claude_notification.json");
const CLAUDE_STOP: &str = include_str!("fixtures/claude_stop.json");
const CODEX_TURN: &str = include_str!("fixtures/codex_turn_complete.json");

#[test]
fn emit_dry_run_inside_herdr_routes_to_herdr_sink() {
    herald()
        .env("HERDR_ENV", "1")
        .args([
            "--dry-run",
            "emit",
            "--source",
            "test",
            "--kind",
            "attention",
            "--body",
            "hi",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"sink\": \"herdr\""))
        .stdout(predicate::str::contains("\"decision\": \"deliver\""));
}

#[test]
fn emit_dry_run_inside_cmux_routes_to_cmux_sink() {
    herald()
        .env("CMUX_SURFACE_ID", "s1")
        .args([
            "--dry-run",
            "emit",
            "--source",
            "test",
            "--kind",
            "turn-complete",
            "--body",
            "hi",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"sink\": \"cmux\""));
}

#[test]
fn emit_dry_run_under_orca_suppresses_turn_complete() {
    herald()
        .env("ORCA_AGENT_HOOK_PORT", "4242")
        .args([
            "--dry-run",
            "emit",
            "--source",
            "test",
            "--kind",
            "turn-complete",
            "--body",
            "hi",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("suppressed-harness"));
}

#[test]
fn hook_claude_notification_plans_system_banner() {
    herald()
        .args(["--dry-run", "hook", "claude"])
        .write_stdin(CLAUDE_NOTIFICATION)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"sink\": \"system\""))
        .stdout(predicate::str::contains("Claude Code — acme"));
}

#[test]
fn hook_claude_accepts_legacy_positional() {
    herald()
        .args(["--dry-run", "hook", "claude", "notification"])
        .write_stdin(CLAUDE_NOTIFICATION)
        .assert()
        .success();
}

#[test]
fn hook_claude_stop_headless_delivers() {
    herald()
        .args(["--dry-run", "hook", "claude"])
        .write_stdin(CLAUDE_STOP)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"headless\": true"))
        .stdout(predicate::str::contains("\"decision\": \"deliver\""));
}

#[test]
fn hook_never_fails_on_garbage() {
    herald()
        .args(["hook", "claude"])
        .write_stdin("this is not json")
        .assert()
        .success()
        .stderr(predicate::str::contains("herald:"));
}

#[test]
fn hook_ignores_irrelevant_events_silently() {
    herald()
        .args(["hook", "claude"])
        .write_stdin(r#"{"hook_event_name":"PreToolUse","cwd":"/tmp"}"#)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn hook_codex_takes_payload_as_argv() {
    herald()
        .args(["--dry-run", "hook", "codex", CODEX_TURN])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"source\": \"codex\""))
        .stdout(predicate::str::contains("turn-complete"));
}

#[test]
fn info_events_are_logged_but_not_delivered() {
    let home = tempfile::tempdir().unwrap();
    herald()
        .env("HOME", home.path())
        .args([
            "emit", "--source", "test", "--kind", "info", "--body", "fyi",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("suppressed (policy"));
    let log = home.path().join(".local/state/herald/events.jsonl");
    let text = std::fs::read_to_string(log).unwrap();
    assert!(text.contains("\"decision\":\"suppressed-policy\""));
    assert!(text.contains("\"source\":\"test\""));
}

#[test]
fn exec_sink_receives_canonical_event_json() {
    let home = tempfile::tempdir().unwrap();
    let out_file = home.path().join("captured.json");
    let config_dir = home.path().join(".config/herald");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"
[[sinks.exec]]
name = "capture"
when_env = "MYTERM_SOCKET"
command = ["/bin/sh", "-c", "cat > {}"]
min_urgency = "normal"
exclusive = true
"#,
            out_file.display()
        ),
    )
    .unwrap();

    herald()
        .env("HOME", home.path())
        .env("MYTERM_SOCKET", "/tmp/fake.sock")
        .args([
            "emit",
            "--source",
            "myagent",
            "--kind",
            "attention",
            "--body",
            "need input",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("exec:capture"));

    let captured = std::fs::read_to_string(out_file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&captured).unwrap();
    assert_eq!(v["source"], "myagent");
    assert_eq!(v["kind"], "attention");
}

#[test]
fn identical_event_within_window_is_coalesced() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config/herald");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        r#"
[[sinks.exec]]
name = "capture"
when_env = "MYTERM_SOCKET"
command = ["/usr/bin/true"]
min_urgency = "normal"
exclusive = true
"#,
    )
    .unwrap();
    let emit = |body: &str| {
        let mut cmd = herald();
        cmd.env("HOME", home.path())
            .env("MYTERM_SOCKET", "/tmp/fake.sock")
            .args([
                "emit",
                "--source",
                "burster",
                "--kind",
                "attention",
                "--body",
                body,
            ]);
        cmd
    };
    emit("first")
        .assert()
        .success()
        .stdout(predicate::str::contains("delivered"));
    emit("second, right after")
        .assert()
        .success()
        .stdout(predicate::str::contains("burst"));
}

#[test]
fn quiet_hours_suppress_via_dry_run_config() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config/herald");
    std::fs::create_dir_all(&config_dir).unwrap();
    // window covering the whole day → any run time falls inside
    std::fs::write(
        config_dir.join("config.toml"),
        "[routing]\nquiet-hours = \"00:00-23:59\"\n",
    )
    .unwrap();
    herald()
        .env("HOME", home.path())
        .args([
            "--dry-run",
            "emit",
            "--source",
            "t",
            "--kind",
            "turn-complete",
            "--body",
            "x",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("suppressed-quiet"));
    // critical pierces quiet hours
    herald()
        .env("HOME", home.path())
        .args([
            "--dry-run",
            "emit",
            "--source",
            "t",
            "--kind",
            "error",
            "--body",
            "x",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"decision\": \"deliver\""));
}

#[test]
fn emit_json_reads_canonical_event_from_stdin() {
    herald()
        .args(["--dry-run", "emit", "--json"])
        .write_stdin(r#"{"source":"my-agent","kind":"error","body":"backup failed"}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"resolved_urgency\": \"critical\"",
        ));
}

#[test]
fn emit_without_required_flags_fails() {
    herald().args(["emit", "--body", "x"]).assert().failure();
}

#[test]
fn log_command_renders_compact_lines() {
    let home = tempfile::tempdir().unwrap();
    herald()
        .env("HOME", home.path())
        .args([
            "emit", "--source", "test", "--kind", "info", "--body", "fyi",
        ])
        .assert()
        .success();
    herald()
        .env("HOME", home.path())
        .args(["log", "-n", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("info"))
        .stdout(predicate::str::contains("suppressed-policy"));
}
