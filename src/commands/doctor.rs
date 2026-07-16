//! `herald doctor` — one command that answers "why did/didn't I get a
//! notification?": config state, detected context, presenter authorization,
//! backend availability, log writability. `--json` emits the same report
//! machine-readable for scripting and monitoring.

use crate::config;
use crate::context;
use crate::platform;
use crate::sinks::system::{SystemSink, which};
use anyhow::{Context as _, Result, bail};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Serialize)]
struct Report {
    healthy: bool,
    config: ConfigReport,
    context: ContextReport,
    presenter: Option<PresenterReport>,
    fallbacks: Fallbacks,
    harnesses: HarnessClis,
    exec_sinks: Vec<ExecSinkReport>,
    log: LogReport,
}

#[derive(Serialize)]
struct ConfigReport {
    path: String,
    status: String,
}

#[derive(Serialize)]
struct ContextReport {
    harness: &'static str,
    terminal: Option<String>,
    headless: bool,
    tmux: bool,
    frontmost: Option<bool>,
}

#[derive(Serialize)]
struct PresenterReport {
    binary: String,
    status: String,
    authorized: bool,
}

#[derive(Serialize)]
struct Fallbacks {
    terminal_notifier: Option<String>,
    osascript: Option<String>,
    notify_send: Option<String>,
}

#[derive(Serialize)]
struct HarnessClis {
    herdr: Option<String>,
    cmux: Option<String>,
    orca_env: bool,
}

#[derive(Serialize)]
struct ExecSinkReport {
    name: String,
    active: bool,
}

#[derive(Serialize)]
struct LogReport {
    path: String,
    writable: bool,
}

pub fn run(explicit_config: Option<&Path>, json: bool) -> i32 {
    let report = collect(explicit_config);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into())
        );
    } else {
        render_text(&report);
    }
    if report.healthy { 0 } else { 1 }
}

fn collect(explicit_config: Option<&Path>) -> Report {
    let mut healthy = true;

    let loaded = config::load(explicit_config);
    let (cfg, config_report) = match loaded {
        Ok((cfg, path)) => {
            let status = if path.exists() {
                "ok"
            } else {
                "missing (defaults apply)"
            }
            .to_string();
            (
                cfg,
                ConfigReport {
                    path: path.display().to_string(),
                    status,
                },
            )
        }
        Err(err) => {
            healthy = false;
            (
                config::Config::default(),
                ConfigReport {
                    path: explicit_config
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| config::default_path().display().to_string()),
                    status: format!("PARSE ERROR — {err:#}"),
                },
            )
        }
    };

    let ctx = context::current();
    let context_report = ContextReport {
        harness: ctx.harness.name(),
        terminal: ctx.terminal_bundle_id.clone(),
        headless: ctx.headless,
        tmux: ctx.tmux,
        frontmost: platform::terminal_is_frontmost(&ctx),
    };

    let system = SystemSink::from_config(&cfg);
    let presenter = system.presenter_binary().map(|bin| {
        let status = Command::new(&bin)
            .arg("status")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "status probe failed".to_string());
        let authorized = status.contains("authorized");
        PresenterReport {
            binary: bin.display().to_string(),
            status,
            authorized,
        }
    });
    // On macOS 26+ the fallbacks fail silently: a working presenter is the
    // health bar. On Linux, notify-send is.
    if cfg!(target_os = "macos") {
        if !presenter.as_ref().is_some_and(|p| p.authorized) {
            healthy = false;
        }
    } else if which("notify-send").is_none() {
        healthy = false;
    }

    let display = |p: Option<std::path::PathBuf>| p.map(|p| p.display().to_string());
    let fallbacks = Fallbacks {
        terminal_notifier: display(which("terminal-notifier").or_else(|| {
            let brew = Path::new("/opt/homebrew/bin/terminal-notifier");
            brew.exists().then(|| brew.to_path_buf())
        })),
        osascript: display(which("osascript")),
        notify_send: display(which("notify-send")),
    };

    let harnesses = HarnessClis {
        herdr: display(which("herdr")),
        cmux: display(which("cmux")),
        orca_env: std::env::var("ORCA_AGENT_HOOK_PORT").is_ok(),
    };

    let exec_sinks = cfg
        .sinks
        .exec
        .iter()
        .map(|exec_cfg| {
            let sink = crate::sinks::exec::ExecSink::new(exec_cfg.clone());
            ExecSinkReport {
                name: exec_cfg.name.clone(),
                active: sink.active(),
            }
        })
        .collect();

    let log_path = config::log_path();
    let writable = log_path
        .parent()
        .map(|dir| std::fs::create_dir_all(dir).is_ok())
        .unwrap_or(false)
        && std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .is_ok();
    healthy &= writable;

    Report {
        healthy,
        config: config_report,
        context: context_report,
        presenter,
        fallbacks,
        harnesses,
        exec_sinks,
        log: LogReport {
            path: log_path.display().to_string(),
            writable,
        },
    }
}

fn render_text(r: &Report) {
    println!("config     {} — {}", r.config.path, r.config.status);
    println!(
        "context    harness={} terminal={} headless={} tmux={} frontmost={}",
        r.context.harness,
        r.context.terminal.as_deref().unwrap_or("-"),
        r.context.headless,
        r.context.tmux,
        r.context
            .frontmost
            .map_or("unknown".to_string(), |b| b.to_string()),
    );
    match &r.presenter {
        Some(p) => println!("presenter  {} — {}", p.binary, p.status),
        None => println!(
            "presenter  NOT FOUND (no Herald.app or ClaudeNotify.app, and no app_path configured)"
        ),
    }
    let show = |o: &Option<String>| o.clone().unwrap_or_else(|| "not found".into());
    println!(
        "fallbacks  terminal-notifier={} osascript={} notify-send={}",
        show(&r.fallbacks.terminal_notifier),
        show(&r.fallbacks.osascript),
        show(&r.fallbacks.notify_send),
    );
    if cfg!(target_os = "macos") && !r.presenter.as_ref().is_some_and(|p| p.authorized) {
        println!(
            "           WARNING: on macOS 26+ the fallbacks fail silently; a working presenter app is required"
        );
    }
    println!(
        "harnesses  herdr={} cmux={} orca-env={}",
        show(&r.harnesses.herdr),
        show(&r.harnesses.cmux),
        r.harnesses.orca_env,
    );
    for sink in &r.exec_sinks {
        println!(
            "exec sink  {} — {}",
            sink.name,
            if sink.active {
                "active"
            } else {
                "inactive (env not set)"
            }
        );
    }
    println!(
        "log        {} — {}",
        r.log.path,
        if r.log.writable {
            "writable"
        } else {
            "NOT WRITABLE"
        }
    );
    println!();
    println!(
        "doctor: {}",
        if r.healthy { "ok" } else { "problems found" }
    );
}

/// Build Herald.app from the embedded sources into ~/Applications.
/// Deliberately installs alongside any existing presenter: on macOS 26+
/// Notification Center registration of new bundles can fail, so a working
/// presenter must never be replaced — only superseded after verification.
pub fn install_app() -> Result<()> {
    const MAIN_SWIFT: &str = include_str!("../../app/main.swift");
    const INFO_PLIST: &str = include_str!("../../app/Info.plist");

    if !cfg!(target_os = "macos") {
        bail!("--install-app is macOS-only (Linux uses notify-send, no presenter app needed)");
    }
    if which("swiftc").is_none() {
        bail!(
            "swiftc not found — install the Xcode Command Line Tools first (xcode-select --install)"
        );
    }
    let app = config::home_dir().join("Applications").join("Herald.app");
    if app.exists() {
        bail!(
            "{} already exists; remove it first if you want a rebuild",
            app.display()
        );
    }

    let build_dir = tempfile_dir()?;
    let src = build_dir.join("main.swift");
    std::fs::write(&src, MAIN_SWIFT)?;

    let macos_dir = app.join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos_dir)?;
    let binary = macos_dir.join("herald-notify");
    run_step(
        "swiftc",
        &[
            "-O",
            &src.to_string_lossy(),
            "-o",
            &binary.to_string_lossy(),
        ],
    )?;
    std::fs::write(app.join("Contents").join("Info.plist"), INFO_PLIST)?;
    run_step("codesign", &["--force", "-s", "-", &app.to_string_lossy()])?;
    let _ = std::fs::remove_dir_all(&build_dir);

    println!("installed: {}", app.display());
    println!("verify before switching app_path:");
    println!(
        "  1. '{}' 'Herald' 'authorization probe'   (must trigger the permission dialog or show a banner)",
        binary.display()
    );
    println!(
        "  2. '{}' status                            (must print: authorized)",
        binary.display()
    );
    println!(
        "  3. only then set [sinks.macos_native] app_path = \"{}\"",
        app.display()
    );
    Ok(())
}

fn tempfile_dir() -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("herald-build-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn run_step(program: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("running {program}"))?;
    if !out.status.success() {
        bail!("{program} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}
